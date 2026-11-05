//! What git and ssh start to ask the user something, in the tests. The app is its own askpass
//! program: it starts as this does before doing anything else. On Windows it is a windowed program
//! too, as the app is in a release build, so the tests cover writing the answer from one.

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let code = gikli_core::answer_askpass_request().expect("started by the git gikli runs");
    std::process::exit(code);
}
