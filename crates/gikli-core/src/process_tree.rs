//! Ending one run of git: git and every process it started, directly or not, as killing git alone
//! leaves the program a hook started holding git's output open.
//!
//! On Windows the run lives in a Job Object, and on Unix in the process group of the session git
//! runs in. A program that leaves that group on purpose, with a session of its own, stays alive,
//! as nothing tells gikli about it. gikli stops waiting for its output after a bounded time.

use std::io;
use std::process::{Child, Command, ExitStatus};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::lock;

/// How long a cancelled run gets to be gone. Processes end within milliseconds of being killed,
/// so this only runs out when one escaped.
pub(crate) const GRACE: Duration = Duration::from_secs(5);

/// How often a run that ended is checked for what it takes to report it.
const POLL: Duration = Duration::from_millis(10);

/// The request to cancel one long operation, and the run of git it reaches.
#[derive(Default)]
pub(crate) struct Cancellation {
    state: Mutex<CancellationState>,
}

#[derive(Default)]
struct CancellationState {
    requested: bool,
    /// The processes of the run in progress. It goes before the run's pid can be used again.
    tree: Option<ProcessTree>,
}

impl Cancellation {
    /// Ends the run in progress, and any run that starts from now on.
    pub fn request(&self) {
        let mut state = lock(&self.state);
        state.requested = true;
        if let Some(tree) = &state.tree {
            tree.end();
        }
    }

    pub fn is_requested(&self) -> bool {
        lock(&self.state).requested
    }

    /// Makes `tree` the run this ends.
    pub fn reach(&self, tree: ProcessTree) {
        let mut state = lock(&self.state);
        if state.requested {
            tree.end();
        }
        state.tree = Some(tree);
    }

    /// Waits for `child`, the process of the run this reaches. When the run was cancelled, it also
    /// waits for the rest of its processes, for a bounded time, so that nothing the run started is
    /// left once this returns.
    pub fn wait_for(&self, child: &mut Child) -> io::Result<ExitStatus> {
        let (status, tree) = loop {
            // Locked, so the request never reaches a pid that was already given back.
            let mut state = lock(&self.state);
            if let Some(status) = child.try_wait()? {
                let tree = state.tree.take().filter(|_| state.requested);
                break (status, tree);
            }
            drop(state);
            std::thread::sleep(POLL);
        };
        if let Some(tree) = tree {
            let give_up = Instant::now() + GRACE;
            while !tree.is_gone() && Instant::now() < give_up {
                std::thread::sleep(POLL);
            }
        }
        Ok(status)
    }
}

pub(crate) use platform::{ProcessTree, start_contained};

#[cfg(windows)]
mod platform {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        JobObjectBasicAccountingInformation, QueryInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
    };

    /// The exit code of a process ended by a cancel.
    const CANCELLED: u32 = 1;

    /// A Job Object holding one run. The processes in it can neither leave it nor start one
    /// outside it, as it allows no breakaway. Closing it ends nothing: gikli kills only on request.
    pub(crate) struct ProcessTree {
        job: HANDLE,
    }

    // SAFETY: a job handle is valid from any thread, and every call on it here is thread-safe.
    unsafe impl Send for ProcessTree {}

    /// Makes `command` start without running, so that it starts nothing before it is in a job,
    /// and without a console window, as every run of git.
    pub(crate) fn start_contained(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }

    impl ProcessTree {
        /// Puts `child`, started by a command given to [`start_contained`], in a job of its own,
        /// and lets it run.
        pub fn of(child: &Child) -> io::Result<Self> {
            // SAFETY: both arguments may be null, for a job with no name and default security.
            let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let tree = Self { job };
            // SAFETY: both handles are open: the job right above, and the process in `child`.
            if unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            resume_threads_of(child.id())?;
            Ok(tree)
        }

        pub fn end(&self) {
            // SAFETY: the handle is open until this is dropped. A job with no process left in it
            // only gives an error back, and so does one that is ending, which is the same result.
            unsafe { TerminateJobObject(self.job, CANCELLED) };
        }

        pub fn is_gone(&self) -> bool {
            // SAFETY: the structure is zeroed plain data.
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
            // SAFETY: the buffer is as big as the size given, and the returned length may be null.
            let queried = unsafe {
                QueryInformationJobObject(
                    self.job,
                    JobObjectBasicAccountingInformation,
                    (&raw mut info).cast(),
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    ptr::null_mut(),
                )
            };
            queried == 0 || info.ActiveProcesses == 0
        }
    }

    impl Drop for ProcessTree {
        fn drop(&mut self) {
            // SAFETY: the handle is open, and nothing uses it after this.
            unsafe { CloseHandle(self.job) };
        }
    }

    /// Lets the suspended process `pid` run. The standard library does not hand out the handle of
    /// its main thread, so this finds the thread in a list of all threads.
    fn resume_threads_of(pid: u32) -> io::Result<()> {
        // SAFETY: the snapshot of threads ignores the process given.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the structure is zeroed plain data, with its size set as the calls ask.
        let mut thread: THREADENTRY32 = unsafe { std::mem::zeroed() };
        thread.dwSize = size_of::<THREADENTRY32>() as u32;
        let mut resumed = false;
        // SAFETY: the snapshot is open and `thread` is valid for both calls.
        let mut more = unsafe { Thread32First(snapshot, &mut thread) } != 0;
        while more {
            if thread.th32OwnerProcessID == pid {
                resumed |= resume_thread(thread.th32ThreadID);
            }
            // SAFETY: as above.
            more = unsafe { Thread32Next(snapshot, &mut thread) } != 0;
        }
        let error = io::Error::last_os_error();
        // SAFETY: the snapshot is open, and nothing uses it after this.
        unsafe { CloseHandle(snapshot) };
        if resumed { Ok(()) } else { Err(error) }
    }

    fn resume_thread(id: u32) -> bool {
        // SAFETY: a thread that is gone only gives a null handle back.
        let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, id) };
        if thread.is_null() {
            return false;
        }
        // SAFETY: the handle was opened right above with the access this needs.
        let resumed = unsafe { ResumeThread(thread) } != u32::MAX;
        // SAFETY: the handle is open, and nothing uses it after this.
        unsafe { CloseHandle(thread) };
        resumed
    }
}

#[cfg(unix)]
mod platform {
    use std::io;
    use std::process::{Child, Command};

    /// The process group of one run. git leads it, as it runs in a session of its own.
    pub(crate) struct ProcessTree {
        group: libc::pid_t,
    }

    /// The group is there already: every run of git starts a session.
    pub(crate) fn start_contained(_command: &mut Command) {}

    impl ProcessTree {
        pub fn of(child: &Child) -> io::Result<Self> {
            Ok(Self {
                group: child.id() as libc::pid_t,
            })
        }

        pub fn end(&self) {
            // SAFETY: this takes no pointers. A group with no process left only gives an error.
            unsafe { libc::killpg(self.group, libc::SIGKILL) };
        }

        pub fn is_gone(&self) -> bool {
            // SAFETY: as above. Signal 0 only checks that the group is there.
            unsafe { libc::killpg(self.group, 0) != 0 }
        }
    }
}

/// Starts `command`, contained by [`start_contained`], as a process tree that `cancellation` ends.
pub(crate) fn spawn_contained(
    command: &mut Command,
    cancellation: &Cancellation,
) -> io::Result<Child> {
    let mut child = command.spawn()?;
    match ProcessTree::of(&child) {
        Ok(tree) => {
            cancellation.reach(tree);
            Ok(child)
        }
        Err(error) => {
            // It never ran, as it started suspended.
            let _ = child.kill();
            let _ = child.wait();
            Err(error)
        }
    }
}
