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
/// or a few hundred on a busy machine, so this only runs out when one escaped.
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
        if let Some(tree) = &mut state.tree {
            tree.end();
        }
    }

    pub fn is_requested(&self) -> bool {
        lock(&self.state).requested
    }

    /// Whether this is what ended a run with `status`. A run that ended by itself first, which it
    /// can do long before its output closes when a hook left a program behind, has its own result.
    pub fn ended_run_with(&self, status: ExitStatus) -> bool {
        self.is_requested() && platform::was_ended(status)
    }

    /// Makes `tree` the run this ends.
    fn attach(&self, mut tree: ProcessTree) {
        let mut state = lock(&self.state);
        if state.requested {
            tree.end();
        }
        state.tree = Some(tree);
    }

    /// Waits for `child`, the process of the run this reaches. When the run was cancelled, it also
    /// ends the rest of its processes and waits for them, for a bounded time, so that nothing the
    /// run started is left once this returns.
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
        if let Some(mut tree) = tree {
            let give_up = Instant::now() + GRACE;
            while !tree.is_gone() && Instant::now() < give_up {
                // Again, as the request can miss a process. The run's pid is not given back while
                // anything of the run is left, which is what was just seen.
                tree.end();
                std::thread::sleep(POLL);
            }
        }
        Ok(status)
    }
}

use platform::ProcessTree;

#[cfg(windows)]
mod platform {
    use std::io;
    use std::mem::offset_of;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, ExitStatus};
    use std::ptr;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_MORE_DATA, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
        JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
        QueryInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenProcess, OpenThread,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, ResumeThread,
        THREAD_SUSPEND_RESUME, WaitForSingleObject,
    };

    /// The exit code of a process ended by a cancel, which no program gives by itself: `GIKL`.
    const CANCELLED: u32 = 0x4749_4B4C;

    /// A Job Object holding one run. The processes in it can neither leave it nor start one
    /// outside it, as it allows no breakaway. Closing it ends nothing: gikli kills only on request.
    pub(crate) struct ProcessTree {
        job: HANDLE,
        /// The processes that were in the job when it was ended. The job stops counting a process
        /// it ended before that process has exited, which on a busy machine takes a while.
        held: Vec<HeldProcess>,
    }

    /// A process of the job, held open so that its pid goes to no other process meanwhile.
    struct HeldProcess {
        pid: u32,
        handle: HANDLE,
    }

    impl HeldProcess {
        /// Opens `pid`, when it is still a process in `job`.
        fn open(pid: u32, job: HANDLE) -> Option<Self> {
            // SAFETY: a process that is gone only gives a null handle back.
            let handle = unsafe {
                OpenProcess(
                    PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    pid,
                )
            };
            if handle.is_null() {
                return None;
            }
            let process = Self { pid, handle };
            // The pid may have gone to another process since the job listed it.
            let mut in_job = 0;
            // SAFETY: both handles are open, and `in_job` is valid for the call.
            let asked = unsafe { IsProcessInJob(handle, job, &mut in_job) } != 0;
            (asked && in_job != 0).then_some(process)
        }

        fn has_exited(&self) -> bool {
            // SAFETY: the handle is open until this is dropped, and a wait of no time only looks.
            unsafe { WaitForSingleObject(self.handle, 0) == WAIT_OBJECT_0 }
        }
    }

    impl Drop for HeldProcess {
        fn drop(&mut self) {
            // SAFETY: the handle is open, and nothing uses it after this.
            unsafe { CloseHandle(self.handle) };
        }
    }

    // SAFETY: job and process handles are valid from any thread, and every call on them here is
    // thread-safe.
    unsafe impl Send for ProcessTree {}

    /// Makes `command` start without running, so that it starts nothing before it is in a job,
    /// and without a console window, as every run of git.
    pub(super) fn prepare(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }

    pub(super) fn was_ended(status: ExitStatus) -> bool {
        status.code() == Some(CANCELLED as i32)
    }

    impl ProcessTree {
        /// Puts `child`, started by a command given to [`prepare`], in a job of its own, and lets
        /// it run.
        pub fn of(child: &Child) -> io::Result<Self> {
            // SAFETY: both arguments may be null, for a job with no name and default security.
            let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let tree = Self {
                job,
                held: Vec::new(),
            };
            // SAFETY: both handles are open: the job right above, and the process in `child`.
            if unsafe { AssignProcessToJobObject(job, child.as_raw_handle()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            resume_threads_of(child.id())?;
            Ok(tree)
        }

        pub fn end(&mut self) {
            // Before, as the job can stop listing a process as soon as it is ended, and after,
            // for one started in between.
            self.hold_processes();
            // SAFETY: the handle is open until this is dropped. A job with no process left in it
            // only gives an error back, and so does one that is ending, which is the same result.
            unsafe { TerminateJobObject(self.job, CANCELLED) };
            self.hold_processes();
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
            let none_counted = queried == 0 || info.ActiveProcesses == 0;
            none_counted && self.held.iter().all(HeldProcess::has_exited)
        }

        /// Opens the processes in the job that are not held yet, so that [`Self::is_gone`] sees
        /// when they have exited.
        fn hold_processes(&mut self) {
            for pid in self.process_ids() {
                if self.held.iter().any(|held| held.pid == pid) {
                    continue;
                }
                if let Some(process) = HeldProcess::open(pid, self.job) {
                    self.held.push(process);
                }
            }
        }

        /// The pids of the processes in the job, or none when the job cannot say.
        fn process_ids(&self) -> Vec<u32> {
            let header =
                offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList) / size_of::<usize>();
            let mut capacity = 16;
            loop {
                // In words, so that the list is aligned as it needs.
                let mut buffer = vec![0usize; header + capacity];
                // SAFETY: the buffer is as big as the size given, and the returned length may be
                // null.
                let queried = unsafe {
                    QueryInformationJobObject(
                        self.job,
                        JobObjectBasicProcessIdList,
                        buffer.as_mut_ptr().cast(),
                        size_of_val(buffer.as_slice()) as u32,
                        ptr::null_mut(),
                    )
                } != 0;
                let too_small = !queried
                    && io::Error::last_os_error().raw_os_error() == Some(ERROR_MORE_DATA as i32);
                // SAFETY: the buffer is aligned for the list and holds at least its header.
                let list = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
                if queried {
                    let listed = (list.NumberOfProcessIdsInList as usize).min(capacity);
                    return buffer[header..header + listed]
                        .iter()
                        .map(|&pid| pid as u32)
                        .collect();
                }
                if !too_small {
                    return Vec::new();
                }
                capacity = (list.NumberOfAssignedProcesses as usize).max(capacity * 2);
            }
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
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Child, Command, ExitStatus};

    /// The process group of one run. git leads it, as it runs in a session of its own.
    pub(crate) struct ProcessTree {
        group: libc::pid_t,
    }

    /// The group is there already: every run of git starts a session.
    pub(super) fn prepare(_command: &mut Command) {}

    pub(super) fn was_ended(status: ExitStatus) -> bool {
        status.signal() == Some(libc::SIGKILL)
    }

    impl ProcessTree {
        pub fn of(child: &Child) -> io::Result<Self> {
            Ok(Self {
                group: child.id() as libc::pid_t,
            })
        }

        /// Reaches the processes in the group at this moment, so it can miss one being forked.
        pub fn end(&mut self) {
            // SAFETY: this takes no pointers. A group with no process left only gives an error.
            unsafe { libc::killpg(self.group, libc::SIGKILL) };
        }

        pub fn is_gone(&self) -> bool {
            // SAFETY: as above. Signal 0 only checks that the group is there.
            unsafe { libc::killpg(self.group, 0) != 0 }
        }
    }
}

/// Starts `command` as a process tree that `cancellation` ends.
pub(crate) fn spawn_contained(
    command: &mut Command,
    cancellation: &Cancellation,
) -> io::Result<Child> {
    platform::prepare(command);
    let mut child = command.spawn()?;
    match ProcessTree::of(&child) {
        Ok(tree) => {
            cancellation.attach(tree);
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

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::process::{CommandExt, ExitStatusExt};

    use super::*;

    fn blocking_command() -> Command {
        let mut command = Command::new("sleep");
        command.arg("3600");
        command
    }

    /// Joining the group after the kill plays a process being forked while the kill went by, which
    /// no test can make happen on demand.
    #[test]
    fn a_process_the_cancel_missed_is_ended_too() {
        let cancellation = Cancellation::default();
        let mut run =
            spawn_contained(blocking_command().process_group(0), &cancellation).expect("start");
        cancellation.request();
        let mut missed = blocking_command()
            .process_group(run.id() as libc::pid_t)
            .spawn()
            .expect("join the group of the run");

        let ended = std::thread::scope(|scope| {
            let waiting = scope.spawn(|| cancellation.wait_for(&mut run));
            loop {
                // In this order: once the wait is over, what is missed has ended if it ever will.
                // The test is its parent, so on Linux the group goes when this collects it.
                let wait_is_over = waiting.is_finished();
                let status = missed.try_wait().expect("look at the process");
                if wait_is_over || status.is_some() {
                    break status;
                }
                std::thread::sleep(POLL);
            }
        });

        let _ = missed.kill();
        let _ = missed.wait();
        assert_eq!(
            ended.and_then(|status| status.signal()),
            Some(libc::SIGKILL)
        );
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::sync::mpsc;

    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    use super::*;

    /// What the test binary plays when a test starts it again: see [`play_a_process_of_the_run`].
    const ROLE: &str = "GIKLI_TEST_PROCESS_TREE_ROLE";
    /// How a slow process says it is ready, followed by its pid.
    const READY: &str = "slow process ready:";
    /// How many slow processes one run starts.
    const SLOW_PROCESSES: usize = 2;
    /// The memory each slow process holds, which Windows takes a while to give back once it ends.
    /// With less, the processes are often taken down before the cancel looks.
    const HELD: usize = 512 << 20;
    /// How long a slow process gets to say it is ready.
    const READY_WITHIN: Duration = Duration::from_secs(60);

    fn play(role: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().expect("find the test binary"));
        command
            .args([
                "--exact",
                "process_tree::windows_tests::play_a_process_of_the_run",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(ROLE, role);
        command
    }

    /// Cancels the run when a test stops before it does, which leaves nothing running.
    struct EndOnDrop<'a>(&'a Cancellation);

    impl Drop for EndOnDrop<'_> {
        fn drop(&mut self) {
            self.0.request();
        }
    }

    /// Not a test: the test binary, started again by a test, plays a process of the run there.
    #[test]
    #[ignore = "not a test: it plays a process of a run that another test starts"]
    fn play_a_process_of_the_run() {
        let _held;
        match std::env::var(ROLE).as_deref() {
            Ok("starter") => {
                let slow: Vec<_> = (0..SLOW_PROCESSES)
                    .map(|_| play("slow").spawn().expect("start a slow process"))
                    .collect();
                for mut process in slow {
                    let _ = process.wait();
                }
            }
            Ok("slow") => {
                _held = std::hint::black_box(vec![1u8; HELD]);
                println!("{READY} {}", std::process::id());
            }
            _ => return,
        }
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }

    /// Processes the cancel ended, but that Windows has not finished taking down, which the job
    /// already counts as gone.
    #[test]
    fn a_cancelled_run_is_over_only_once_its_processes_have_exited() {
        let cancellation = Cancellation::default();
        let mut run = spawn_contained(play("starter").stdout(Stdio::piped()), &cancellation)
            .expect("start the run");
        let _ended = EndOnDrop(&cancellation);
        let output = BufReader::new(run.stdout.take().expect("stdout is piped"));
        let (ready, pids) = mpsc::channel();
        // On a thread of its own, so that a slow process that never gets ready fails the test
        // rather than hold it up.
        std::thread::spawn(move || {
            for line in output.lines().map_while(Result::ok) {
                // The test harness writes on the same line first.
                if let Some((_, pid)) = line.rsplit_once(READY) {
                    let _ = ready.send(pid.trim().parse::<u32>().expect("a pid"));
                }
            }
        });
        let slow: Vec<_> = (0..SLOW_PROCESSES)
            .map(|_| {
                pids.recv_timeout(READY_WITHIN)
                    .expect("a slow process ready")
            })
            .map(|pid| {
                // SAFETY: a process that is gone only gives a null handle back.
                let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                assert!(!process.is_null(), "open the slow process {pid}");
                process
            })
            .collect();

        cancellation.request();
        cancellation.wait_for(&mut run).expect("wait for the run");

        let left = slow
            .into_iter()
            .filter(|&process| {
                // SAFETY: the handle was opened above, and nothing uses it after this.
                unsafe {
                    let exited = WaitForSingleObject(process, 0) == WAIT_OBJECT_0;
                    CloseHandle(process);
                    !exited
                }
            })
            .count();
        assert_eq!(left, 0, "processes of the run still exiting");
    }
}
