#![cfg_attr(
    all(windows, not(debug_assertions), not(test)),
    windows_subsystem = "windows"
)]

#[cfg(windows)]
#[path = "windows.rs"]
mod windows_runtime;
#[cfg(windows)]
pub(crate) use windows_runtime::*;

#[cfg(windows)]
fn main() {
    windows_runtime::main();
}

#[cfg(unix)]
mod native;

#[cfg(unix)]
fn main() {
    native::main();
}
