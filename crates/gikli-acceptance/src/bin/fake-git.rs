//! A stand-in for git that tests install to play a git installation the real one cannot.
//!
//! This is the one exception to "tests run the real git": the real git cannot play another
//! version (older than 2.34, unreadable, or just below or at what a capability needs), a git the
//! user installed off the PATH, or `wsl.exe` on a machine without WSL. Use it only for those
//! installation and version checks.
//!
//! It answers `--version` with the text in `fake-git-version` next to it, records every
//! invocation in `fake-git-invocations`, and fails anything else.

use std::fs::{self, OpenOptions};
use std::io::Write;

fn main() {
    let exe = std::env::current_exe().expect("locate fake git");
    let dir = exe.parent().expect("fake git has a folder");
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("fake-git-invocations"))
        .expect("open invocation log");
    writeln!(log, "{}", args.join("\u{1f}")).expect("record invocation");

    if args
        .iter()
        .any(|arg| arg == "--version" || arg == "version")
    {
        let version = fs::read_to_string(dir.join("fake-git-version")).expect("read fake version");
        print!("{version}");
    } else {
        eprintln!("fatal: fake git only reports its version");
        std::process::exit(128);
    }
}
