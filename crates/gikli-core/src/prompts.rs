//! The askpass bridge: it brings what git and ssh ask the user to the user, inside gikli.
//!
//! git and ssh ask through an askpass program, which gets the question as its argument and writes
//! the answer on its output. gikli's askpass program ([`answer_askpass_request`]) passes the question
//! over a local connection to the core, which raises a [`Prompt`], and waits for the answer. A token
//! that only the processes git starts get in their environment keeps other programs on this machine
//! from asking, or from reading an answer.
//!
//! The answer goes straight from [`Gikli::answer_prompt`](crate::Gikli::answer_prompt) to that
//! connection. gikli keeps no answer: remembering one is the job of the user's credential helper.

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::events::{Event, EventBus};
use crate::host::HostEnvironment;
use crate::{Failure, lock};

/// Where the askpass program finds the core.
const ADDRESS_VARIABLE: &str = "GIKLI_ASKPASS_ADDRESS";
/// What the askpass program shows the core to be let in.
const TOKEN_VARIABLE: &str = "GIKLI_ASKPASS_TOKEN";

/// Variables that cross into WSL, and back to the askpass program, a Windows program, that git
/// inside WSL starts: `/p` translates a path.
pub(crate) const WSL_SHARED_VARIABLES: &[&str] = &[
    "GIT_ASKPASS/p",
    "SSH_ASKPASS/p",
    "SSH_ASKPASS_REQUIRE",
    "GIKLI_ASKPASS_ADDRESS",
    "GIKLI_ASKPASS_TOKEN",
];

/// How long a connection gets to say what it wants.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// More than any question takes.
const REQUEST_LIMIT: u64 = 1024 * 1024;

/// Something git or ssh asks the user. It stays until the user answers it or cancels it, or what
/// asked goes away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: u64,
    pub kind: PromptKind,
    /// The question, as git or ssh words it.
    pub text: String,
}

/// What a prompt asks for. Only a username and the answer about a host key show as they are typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PromptKind {
    Username,
    Password,
    /// The passphrase of a key, to connect with it or to sign with it.
    Passphrase,
    /// Whether to trust the host key of a server ssh does not know. The answer is `yes` or `no`.
    HostKey,
    /// Anything else, such as a PIN or a one-time code.
    Other,
}

impl PromptKind {
    /// What `text` asks for. git writes these words in English in every language, and so does
    /// OpenSSH. What is not recognized counts as a secret.
    fn of(text: &str) -> Self {
        let text = text.trim_start();
        if text.starts_with("Username") {
            Self::Username
        } else if text.starts_with("Password") {
            Self::Password
        } else if text.contains("passphrase") {
            Self::Passphrase
        } else if text.contains("(yes/no") {
            Self::HostKey
        } else {
            Self::Other
        }
    }
}

/// What the askpass program sends the core.
#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    prompt: String,
}

/// The core's side of the bridge: it listens until it is dropped.
pub(crate) struct PromptBridge {
    askpass: PathBuf,
    address: SocketAddr,
    token: String,
    state: Arc<BridgeState>,
}

struct BridgeState {
    /// The prompts waiting for the user, each with the connection its answer goes to.
    pending: Mutex<BTreeMap<u64, (Prompt, TcpStream)>>,
    next_id: AtomicU64,
    stopping: AtomicBool,
    events: Arc<EventBus>,
}

impl PromptBridge {
    /// Starts listening for what the program `askpass` asks.
    pub fn start(askpass: PathBuf, events: Arc<EventBus>) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let address = listener.local_addr()?;
        let token = new_token()?;
        let state = Arc::new(BridgeState {
            pending: Mutex::default(),
            next_id: AtomicU64::new(1),
            stopping: AtomicBool::new(false),
            events,
        });
        let accepting = state.clone();
        let expected = token.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if accepting.stopping.load(Ordering::Relaxed) {
                    return;
                }
                let Ok(stream) = stream else { continue };
                let state = accepting.clone();
                let expected = expected.clone();
                std::thread::spawn(move || state.serve(stream, &expected));
            }
        });
        Ok(Self {
            askpass,
            address,
            token,
            state,
        })
    }

    /// Makes what runs in `host` ask through this bridge. It replaces any askpass program the user
    /// configured, as gikli runs git without a terminal and the prompt belongs inside gikli. It
    /// reaches every program git starts, so hooks and signing programs ask here too.
    pub fn expose_to(&self, host: &mut HostEnvironment) {
        host.set("GIT_ASKPASS", &self.askpass);
        host.set("SSH_ASKPASS", &self.askpass);
        // ssh asks the askpass program without looking for a display. OpenSSH before 8.4 ignores
        // this, and asks it only when `DISPLAY` is set. gikli does not make one up for it, as the
        // variable would reach every program git starts, and such an ssh fails with a message.
        host.set("SSH_ASKPASS_REQUIRE", "force");
        host.set(ADDRESS_VARIABLE, self.address.to_string());
        host.set(TOKEN_VARIABLE, &self.token);
    }

    pub fn pending(&self) -> Vec<Prompt> {
        lock(&self.state.pending)
            .values()
            .map(|(prompt, _)| prompt.clone())
            .collect()
    }

    /// Gives `answer` to what asked prompt `id`, or None to tell it the user declined.
    pub fn respond(&self, id: u64, answer: Option<&str>) -> Result<(), Failure> {
        let (_, mut stream) = lock(&self.state.pending)
            .remove(&id)
            .ok_or(Failure::PromptGone { id })?;
        if let Some(answer) = answer {
            // When what asked is already gone, nobody needs the answer.
            let _ = writeln!(stream, "{answer}");
        }
        let _ = stream.shutdown(Shutdown::Write);
        self.state.events.send(Event::PromptClosed { id });
        Ok(())
    }
}

impl Drop for PromptBridge {
    fn drop(&mut self) {
        self.state.stopping.store(true, Ordering::Relaxed);
        // Wakes the listener up, so it sees it has to stop.
        let _ = TcpStream::connect(self.address);
        for (_, (_, stream)) in std::mem::take(&mut *lock(&self.state.pending)) {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl BridgeState {
    /// Serves one connection from start to end: it raises the prompt, then waits for the askpass
    /// program to go away. A program that goes away before the user answers leaves nothing to
    /// answer, as when ssh no longer needs the touch of a security key it asked for.
    fn serve(&self, stream: TcpStream, token: &str) {
        let Some(text) = read_request(&stream, token) else {
            return;
        };
        let Ok(answer_to) = stream.try_clone() else {
            return;
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let prompt = Prompt {
            id,
            kind: PromptKind::of(&text),
            text,
        };
        lock(&self.pending).insert(id, (prompt.clone(), answer_to));
        self.events.send(Event::PromptRaised(prompt));
        let _ = (&stream).read(&mut [0; 1]);
        if lock(&self.pending).remove(&id).is_some() {
            self.events.send(Event::PromptClosed { id });
        }
    }
}

/// The question on the connection `stream`, when it comes from gikli's askpass program.
fn read_request(stream: &TcpStream, token: &str) -> Option<String> {
    stream.set_read_timeout(Some(REQUEST_TIMEOUT)).ok()?;
    let mut line = String::new();
    BufReader::new(stream.take(REQUEST_LIMIT))
        .read_line(&mut line)
        .ok()?;
    stream.set_read_timeout(None).ok()?;
    let request: Request = serde_json::from_str(&line).ok()?;
    (request.token == token).then_some(request.prompt)
}

/// A token nobody can guess.
fn new_token() -> io::Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Plays the askpass program, when git or ssh started this process as one: it asks the core the
/// question in the first argument and writes the answer on its output. Returns the code to exit
/// with, or None when nothing started this process for that. The app calls this first thing.
pub fn answer_askpass_request() -> Option<i32> {
    let address = std::env::var(ADDRESS_VARIABLE).ok()?;
    let token = std::env::var(TOKEN_VARIABLE).ok()?;
    let prompt = std::env::args_os()
        .nth(1)
        .map(|prompt| prompt.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some(ask_core(&address, token, prompt).unwrap_or(1))
}

fn ask_core(address: &str, token: String, prompt: String) -> io::Result<i32> {
    let mut stream = TcpStream::connect(address)?;
    let mut request = serde_json::to_vec(&Request { token, prompt }).map_err(io::Error::other)?;
    request.push(b'\n');
    stream.write_all(&request)?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer)?;
    if answer.is_empty() {
        // The user declined.
        return Ok(1);
    }
    let mut output = io::stdout();
    output.write_all(&answer)?;
    output.flush()?;
    Ok(0)
}
