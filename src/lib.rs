//! A Deflate compression and decompression library.
//!
//! This crate provides an implementation of the Deflate
//! compression format from scratch.
pub mod io;
pub mod compression;

#[cfg(target_arch = "wasm32")]
mod wasm;