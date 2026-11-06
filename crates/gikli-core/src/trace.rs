//! git's trace of one run, read as git writes it and kept in memory.
//!
//! The trace is the only place that tells which hook runs or failed, in every language. git also
//! writes every command line to it, which can hold a password in a URL that git does not hide
//! (an older git hides none, and no git hides one that reads as a port). So no file ever holds
//! the trace, and git writes it to something that keeps no data.
//!
//! A git that a hook starts inherits the target, and git keeps quiet when it cannot reach its
//! target: it runs without a trace, with its own output untouched and no file created. On macOS and
//! Linux the target is a socket whose path goes away once git connected, and in WSL a FIFO whose
//! path goes away just the same, so only the git gikli started connects. On Windows it is a named
//! pipe with a few instances: any git after those finds the pipe busy, and what one that connected
//! writes goes unused, as its session is nested under a hook's. A pipe that is gone stays gone: git
//! cannot create a file at the name of a pipe.
//!
//! The processes a hook started can inherit the connection of the git gikli started, so the end of
//! that connection says nothing. The reader stops when told the run ended, with what git left.

use std::ffi::OsString;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::lock;
use crate::prompts::new_token;

/// What tells git to write no trace, whatever the user's configuration says.
pub(crate) const NO_TRACE: &str = "0";

/// How often the reader looks for more of the trace. git waits only if a whole pipe fills up
/// meanwhile, which takes far more than a run writes in this time.
const READ_INTERVAL: Duration = Duration::from_millis(5);

/// Tells apart what the runs of this process leave in the temporary folder.
static NEXT_RUN: AtomicU64 = AtomicU64::new(0);

/// A name no other run has.
pub(crate) fn new_run_name() -> String {
    let number = NEXT_RUN.fetch_add(1, Ordering::Relaxed);
    format!("gikli-trace-{}-{number}", std::process::id())
}

/// The hooks the git gikli started has run so far. Of the trace, gikli keeps only this: the rest
/// holds command lines.
#[derive(Default)]
pub(crate) struct HookRuns(Vec<HookRun>);

struct HookRun {
    child_id: i64,
    name: String,
    exit_code: Option<i64>,
}

impl HookRuns {
    /// Takes in one line of the trace. A git a hook started would write under a session nested with
    /// `/`, should it have connected before the path went away.
    fn add_event(&mut self, line: &[u8]) {
        let Ok(event) = serde_json::from_slice::<serde_json::Value>(line) else {
            return;
        };
        if event["sid"].as_str().is_none_or(|sid| sid.contains('/')) {
            return;
        }
        let Some(child_id) = event["child_id"].as_i64() else {
            return;
        };
        match event["event"].as_str() {
            Some("child_start") => {
                if let Some(name) = event["hook_name"].as_str() {
                    self.0.push(HookRun {
                        child_id,
                        name: name.to_owned(),
                        exit_code: None,
                    });
                }
            }
            Some("child_exit") => {
                if let Some(hook) = self.0.iter_mut().find(|hook| hook.child_id == child_id) {
                    hook.exit_code = event["code"].as_i64();
                }
            }
            _ => {}
        }
    }

    /// The hook that ended with an error.
    pub fn failed(&self) -> Option<String> {
        let failed = |hook: &&HookRun| hook.exit_code.is_some_and(|code| code != 0);
        self.0.iter().find(failed).map(|hook| hook.name.clone())
    }

    /// The hook running now: the last one started that has not ended.
    pub fn running(&self) -> Option<String> {
        let running = |hook: &&HookRun| hook.exit_code.is_none();
        self.0
            .iter()
            .rev()
            .find(running)
            .map(|hook| hook.name.clone())
    }

    /// The last hook started, running or not.
    pub fn last(&self) -> Option<String> {
        self.0.last().map(|hook| hook.name.clone())
    }
}

/// Reads the trace of a git on this machine, on a thread of its own, into [`HookRuns`].
pub(crate) struct TraceReader {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl TraceReader {
    /// A reader ready for a git to connect, with where that git must write its trace.
    pub fn start(hooks: Arc<Mutex<HookRuns>>) -> io::Result<(Self, OsString)> {
        let (mut pipe, target) = platform::TracePipe::open(&new_run_name())?;
        let (stop, stopped) = mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("git trace".to_owned())
            .spawn(move || {
                let mut last_pass = false;
                loop {
                    pipe.take_events(&hooks);
                    if last_pass {
                        return;
                    }
                    last_pass = !matches!(
                        stopped.recv_timeout(READ_INTERVAL),
                        Err(RecvTimeoutError::Timeout)
                    );
                }
            })?;
        let reader = Self {
            stop: Some(stop),
            thread: Some(thread),
        };
        Ok((reader, target))
    }

    /// Takes what git left and stops. Call it once git ended: all it wrote is in the pipe by then.
    pub fn finish(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for TraceReader {
    fn drop(&mut self) {
        self.finish();
    }
}

/// The line a git is still writing to one connection, which goes unread until it is whole.
#[derive(Default)]
struct UnfinishedLine(Vec<u8>);

impl UnfinishedLine {
    /// Takes in `bytes`, the next ones read, and hands `hooks` the lines they finish.
    fn add(&mut self, bytes: &[u8], hooks: &Mutex<HookRuns>) {
        self.0.extend_from_slice(bytes);
        let Some(end) = self.0.iter().rposition(|byte| *byte == b'\n') else {
            return;
        };
        let mut hooks = lock(hooks);
        for line in self.0[..end].split(|byte| *byte == b'\n') {
            hooks.add_event(line);
        }
        drop(hooks);
        self.0.drain(..=end);
    }
}

/// How much of the trace is read at a time.
const CHUNK_SIZE: usize = 16 * 1024;

/// Runs inside WSL in front of git, as `sh -c <this> sh <marker> git <arguments>`, as nothing
/// inside WSL can reach a pipe of Windows, and the error output is what reaches gikli. It forwards
/// there only what names a hook of the git gikli started, each with the marker in front, and never
/// a command line. It runs git without a trace when it cannot make the FIFO.
///
/// The folder goes as soon as git has connected, which it does before it starts anything, so a git
/// a hook starts can neither connect nor create a file there. It moves first, as removing it leaves
/// a moment with the folder there and the FIFO gone, in which git would create a file. The marker
/// on the FIFO tells the forwarder that git ended, as a process a hook started can keep the FIFO
/// open.
///
/// A trace whose fields came in another order would lose the name of a hook here, or fail to parse,
/// and never carry a command line.
pub(crate) const WSL_FORWARDER: &str = r#"marker=$1; shift
dir=$(mktemp -d 2>/dev/null) && mkfifo "$dir/trace" 2>/dev/null && exec 3<>"$dir/trace" || {
  [ -z "$dir" ] || rm -rf "$dir"
  exec "$@"
}
(
  connected=
  while IFS= read -r line; do
    [ -n "$connected" ] || { mv "$dir" "$dir.gone"; rm -rf "$dir" "$dir.gone"; connected=1; }
    [ "$line" != "$marker" ] || break
    sid=${line#*'"sid":"'}
    case ${sid%%'"'*} in */*) continue ;; esac
    case $line in
      '{"event":"child_start"'*'"hook_name":"'*)
        event=${line%%,\"argv\":*}
        printf '%s%s}\n' "$marker" "$event" >&2 ;;
      '{"event":"child_exit"'*)
        printf '%s%s\n' "$marker" "$line" >&2 ;;
    esac
  done
) <&3 3>&- &
GIT_TRACE2_EVENT=$dir/trace "$@" 3>&-
status=$?
printf '%s\n' "$marker" >&3
wait
rm -rf "$dir" "$dir.gone"
exit $status
"#;

/// Takes the events of [`WSL_FORWARDER`] out of the error output of a run, before anything else
/// reads it. Progress can leave a line unfinished there, so an event can start anywhere in a line.
pub(crate) struct ForwardedEvents {
    marker: String,
    /// What may still turn out to be an event, as output comes in pieces.
    held: Vec<u8>,
    hooks: Arc<Mutex<HookRuns>>,
}

impl ForwardedEvents {
    /// The marker is new for each run, so what a hook writes cannot read as an event.
    pub fn new(hooks: Arc<Mutex<HookRuns>>) -> io::Result<Self> {
        Ok(Self {
            marker: format!("gikli-trace-{}:", new_token()?),
            held: Vec::new(),
            hooks,
        })
    }

    pub fn marker(&self) -> &str {
        &self.marker
    }

    /// What is left of `output` without events, as far as can be told yet.
    pub fn take_from(&mut self, output: &[u8]) -> Vec<u8> {
        self.held.extend_from_slice(output);
        let marker = self.marker.as_bytes();
        let mut kept = Vec::new();
        loop {
            let Some(start) = self
                .held
                .windows(marker.len())
                .position(|part| part == marker)
            else {
                let certain = self.held.len() - unfinished_marker_at_end(&self.held, marker);
                kept.extend(self.held.drain(..certain));
                return kept;
            };
            kept.extend(self.held.drain(..start));
            let Some(end) = self.held.iter().position(|byte| *byte == b'\n') else {
                return kept;
            };
            lock(&self.hooks).add_event(&self.held[marker.len()..end]);
            self.held.drain(..=end);
        }
    }

    /// What is left to keep when the output has ended: an event cut short is still an event.
    pub fn finish(&mut self) -> Vec<u8> {
        let held = std::mem::take(&mut self.held);
        if held.starts_with(self.marker.as_bytes()) {
            Vec::new()
        } else {
            held
        }
    }
}

/// How many bytes at the end of `output` could be the start of `marker`.
fn unfinished_marker_at_end(output: &[u8], marker: &[u8]) -> usize {
    (1..marker.len().min(output.len() + 1))
        .rev()
        .find(|length| output.ends_with(&marker[..*length]))
        .unwrap_or(0)
}

#[cfg(windows)]
mod platform {
    use std::ffi::OsString;
    use std::fs::File;
    use std::io::{self, Read};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::ptr;
    use std::sync::Mutex;

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND,
    };
    use windows_sys::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        PIPE_WAIT, PeekNamedPipe,
    };

    use super::{CHUNK_SIZE, HookRuns, UnfinishedLine};

    /// How many instances of the pipe there are. git 2.51 uses up two: it asks whether its target
    /// is a folder, which connects and leaves that instance unusable, and then connects for good.
    /// It starts nothing before that, so the git gikli started always finds these free. The rest
    /// are a margin, as another version may ask once more, and running out costs only the name of
    /// a failed hook. A git that a hook starts and that connects to one does no harm: it writes
    /// under a nested session, and when the pipe goes away its next write fails, which ends its
    /// trace and nothing else.
    const INSTANCES: usize = 4;

    /// What one instance holds before git waits for gikli to read.
    const BUFFER_SIZE: u32 = 64 * 1024;

    /// A named pipe, which holds what git writes only until gikli reads it. Once it is gone, a git
    /// finds no pipe by that name, and git cannot create a file there.
    pub struct TracePipe(Vec<Instance>);

    struct Instance {
        pipe: File,
        line: UnfinishedLine,
    }

    impl TracePipe {
        pub fn open(name: &str) -> io::Result<(Self, OsString)> {
            let path: Vec<u16> = std::ffi::OsStr::new(&format!(r"\\.\pipe\{name}"))
                .encode_wide()
                .chain([0])
                .collect();
            let mut instances = Vec::with_capacity(INSTANCES);
            for number in 0..INSTANCES {
                // Fails when anything else already holds the name.
                let first = if number == 0 {
                    FILE_FLAG_FIRST_PIPE_INSTANCE
                } else {
                    0
                };
                // SAFETY: `path` ends with a NUL and outlives the call. The handle it returns, not
                // inheritable, is owned from here on.
                let pipe = unsafe {
                    let handle = CreateNamedPipeW(
                        path.as_ptr(),
                        PIPE_ACCESS_INBOUND | first,
                        PIPE_TYPE_BYTE
                            | PIPE_READMODE_BYTE
                            | PIPE_WAIT
                            | PIPE_REJECT_REMOTE_CLIENTS,
                        INSTANCES as u32,
                        0,
                        BUFFER_SIZE,
                        0,
                        ptr::null(),
                    );
                    if handle == INVALID_HANDLE_VALUE {
                        return Err(io::Error::last_os_error());
                    }
                    File::from_raw_handle(handle)
                };
                instances.push(Instance {
                    pipe,
                    line: UnfinishedLine::default(),
                });
            }
            Ok((Self(instances), format!("//./pipe/{name}").into()))
        }

        /// Takes in all the pipe holds now, without waiting.
        pub fn take_events(&mut self, hooks: &Mutex<HookRuns>) {
            let mut chunk = [0; CHUNK_SIZE];
            for instance in &mut self.0 {
                loop {
                    let length = chunk.len().min(available(&instance.pipe));
                    if length == 0 {
                        break;
                    }
                    let Ok(read) = instance.pipe.read(&mut chunk[..length]) else {
                        break;
                    };
                    instance.line.add(&chunk[..read], hooks);
                }
            }
        }
    }

    /// How many bytes `pipe` holds now. None, for an instance nobody has connected to or that its
    /// git has left.
    fn available(pipe: &File) -> usize {
        let mut available = 0;
        // SAFETY: the handle is open, and only the count of bytes available is asked for.
        let peeked = unsafe {
            PeekNamedPipe(
                pipe.as_raw_handle(),
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                &mut available,
                ptr::null_mut(),
            )
        };
        if peeked == 0 { 0 } else { available as usize }
    }
}

#[cfg(unix)]
mod platform {
    use std::ffi::OsString;
    use std::io::{self, Read};
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::{CHUNK_SIZE, HookRuns, UnfinishedLine};

    /// A socket in a folder only this user can enter. It goes as soon as a git has connected, and
    /// that is the git gikli started, so any other finds no path and creates nothing.
    pub struct TracePipe {
        folder: PathBuf,
        listener: Option<UnixListener>,
        connection: Option<UnixStream>,
        line: UnfinishedLine,
    }

    impl TracePipe {
        pub fn open(name: &str) -> io::Result<(Self, OsString)> {
            let folder = std::env::temp_dir().join(name);
            std::fs::DirBuilder::new().mode(0o700).create(&folder)?;
            let mut pipe = Self {
                folder,
                listener: None,
                connection: None,
                line: UnfinishedLine::default(),
            };
            let socket = pipe.folder.join("trace");
            let listener = UnixListener::bind(&socket)?;
            listener.set_nonblocking(true)?;
            pipe.listener = Some(listener);
            let mut target = OsString::from("af_unix:stream:");
            target.push(&socket);
            Ok((pipe, target))
        }

        /// Takes in all the socket holds now, without waiting.
        pub fn take_events(&mut self, hooks: &Mutex<HookRuns>) {
            if self.connection.is_none() {
                self.take_connection();
            }
            let Some(connection) = &mut self.connection else {
                return;
            };
            let mut chunk = [0; CHUNK_SIZE];
            while let Ok(read) = connection.read(&mut chunk)
                && read > 0
            {
                self.line.add(&chunk[..read], hooks);
            }
        }

        /// Takes the connection of the git gikli started, the first to come, and closes the way
        /// for others.
        fn take_connection(&mut self) {
            let Some(listener) = &self.listener else {
                return;
            };
            let Ok((connection, _)) = listener.accept() else {
                return;
            };
            if connection.set_nonblocking(true).is_err() {
                return;
            }
            self.connection = Some(connection);
            let _ = std::fs::remove_dir_all(&self.folder);
            // A git a hook started can connect in the moment before the path goes. Dropping its
            // connection would end it with SIGPIPE when it next writes.
            while let Ok((late, _)) = listener.accept() {
                std::thread::spawn(move || {
                    let _ = late.set_nonblocking(false);
                    let _ = io::copy(&mut &late, &mut io::sink());
                });
            }
            self.listener = None;
        }
    }

    impl Drop for TracePipe {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.folder);
        }
    }
}
