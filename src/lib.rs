pub mod io;
pub mod compression;

#[cfg(target_arch = "wasm32")]
mod wasm;

pub fn hello() -> String {
    String::from("Hello from Rust!")
}
