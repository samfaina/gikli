//! A local HTTP server in front of a bare repository that requires a username and a password, as a
//! hosting service does. It only checks the credentials: `git http-backend`, git's own server,
//! does the rest.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The server, which runs until this is dropped.
pub struct HttpRemote {
    address: SocketAddr,
    repository: String,
    stopping: Arc<AtomicBool>,
}

/// Who the server lets in.
pub(crate) enum Access<'a> {
    Credentials {
        username: &'a str,
        password: &'a str,
    },
    /// Nobody: every request gets this status, such as `403 Forbidden`.
    Refused { status: &'a str },
}

impl HttpRemote {
    pub(crate) fn serve(root: PathBuf, env: Vec<(OsString, OsString)>, access: Access) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listen on a local port");
        let address = listener.local_addr().expect("the listener has an address");
        let stopping = Arc::new(AtomicBool::new(false));
        let server = Server {
            project_root: root.parent().expect("remote has a folder").to_path_buf(),
            env,
            refusal: match access {
                Access::Credentials { username, password } => Refusal::UnlessAuthorized(format!(
                    "Basic {}",
                    base64(format!("{username}:{password}").as_bytes())
                )),
                Access::Refused { status } => Refusal::Always(status.to_owned()),
            },
        };
        let stop = stopping.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Ok(stream) = stream {
                    // A client that gave up is no reason to stop serving the next one.
                    let _ = server.answer(stream);
                }
            }
        });
        let repository = root
            .file_name()
            .expect("remote has a name")
            .to_string_lossy()
            .into_owned();
        Self {
            address,
            repository,
            stopping,
        }
    }

    /// The URL to clone from.
    pub fn url(&self) -> String {
        format!("http://{}/{}", self.address, self.repository)
    }
}

impl Drop for HttpRemote {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        // Wakes the server up, so it sees it has to stop.
        let _ = TcpStream::connect(self.address);
    }
}

struct Server {
    project_root: PathBuf,
    env: Vec<(OsString, OsString)>,
    refusal: Refusal,
}

enum Refusal {
    /// Requests that do not carry this `Authorization` header are asked for credentials.
    UnlessAuthorized(String),
    /// Every request gets this status.
    Always(String),
}

/// One request, as far as the server needs it.
struct Request {
    method: String,
    path: String,
    query: String,
    content_type: Option<String>,
    content_encoding: Option<String>,
    authorization: Option<String>,
    body: Vec<u8>,
}

impl Server {
    fn answer(&self, mut stream: TcpStream) -> std::io::Result<()> {
        let request = read_request(&stream)?;
        match &self.refusal {
            Refusal::Always(status) => stream.write_all(
                format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            ),
            Refusal::UnlessAuthorized(expected)
                if request.authorization.as_deref() != Some(expected) =>
            {
                stream.write_all(
                    b"HTTP/1.1 401 Unauthorized\r\n\
                      WWW-Authenticate: Basic realm=\"gikli test\"\r\n\
                      Content-Length: 0\r\n\
                      Connection: close\r\n\r\n",
                )
            }
            Refusal::UnlessAuthorized(_) => stream.write_all(&self.run_backend(&request)?),
        }
    }

    /// The answer of `git http-backend`, a CGI program, turned into an HTTP response.
    fn run_backend(&self, request: &Request) -> std::io::Result<Vec<u8>> {
        let mut backend = Command::new("git");
        backend
            .arg("http-backend")
            .envs(self.env.iter().map(|(name, value)| (name, value)))
            .env("GIT_PROJECT_ROOT", &self.project_root)
            .env("GIT_HTTP_EXPORT_ALL", "1")
            .env("REQUEST_METHOD", &request.method)
            .env("PATH_INFO", &request.path)
            .env("QUERY_STRING", &request.query)
            .env("CONTENT_LENGTH", request.body.len().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(content_type) = &request.content_type {
            backend.env("CONTENT_TYPE", content_type);
        }
        if let Some(content_encoding) = &request.content_encoding {
            backend.env("HTTP_CONTENT_ENCODING", content_encoding);
        }
        let mut backend = backend.spawn()?;
        let mut input = backend.stdin.take().expect("stdin is piped");
        input.write_all(&request.body)?;
        drop(input);
        let output = backend.wait_with_output()?;

        let split = output
            .stdout
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| std::io::Error::other("http-backend wrote no headers"))?;
        let (headers, body) = output.stdout.split_at(split + 4);
        let headers = String::from_utf8_lossy(headers);
        let status = headers
            .lines()
            .find_map(|line| line.strip_prefix("Status: "))
            .unwrap_or("200 OK");
        let mut response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{}",
            body.len(),
            headers
        )
        .into_bytes();
        response.extend_from_slice(body);
        Ok(response)
    }
}

fn read_request(stream: &TcpStream) -> std::io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut request = Request {
        method,
        path: path.to_owned(),
        query: query.to_owned(),
        content_type: None,
        content_encoding: None,
        authorization: None,
        body: Vec::new(),
    };
    let mut content_length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        let Some((name, value)) = header.trim_end().split_once(": ") else {
            break;
        };
        let value = value.to_owned();
        match name.to_ascii_lowercase().as_str() {
            "content-type" => request.content_type = Some(value),
            "content-encoding" => request.content_encoding = Some(value),
            "authorization" => request.authorization = Some(value),
            "content-length" => content_length = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    (&mut reader)
        .take(content_length)
        .read_to_end(&mut request.body)?;
    Ok(request)
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for chunk in bytes.chunks(3) {
        let group = chunk
            .iter()
            .fold(0u32, |group, byte| (group << 8) | u32::from(*byte))
            << (8 * (3 - chunk.len()));
        for position in 0..4 {
            if position <= chunk.len() {
                encoded.push(char::from(
                    ALPHABET[(group >> (18 - 6 * position)) as usize & 63],
                ));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}
