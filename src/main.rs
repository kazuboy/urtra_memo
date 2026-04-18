#![cfg_attr(
    all(
        target_os = "windows",
        not(debug_assertions),
        not(target_arch = "wasm32")
    ),
    windows_subsystem = "windows"
)]

#[cfg(not(target_arch = "wasm32"))]
mod native_main;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    native_main::main();
}

#[cfg(target_arch = "wasm32")]
fn main() {
    ultra_memo::run_web();
}
