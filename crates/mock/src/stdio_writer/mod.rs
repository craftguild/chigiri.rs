//! Isolate blocking single-line output in a dedicated child process.

mod pipe;
mod worker;

pub use pipe::Pipe;
pub use worker::serve_writer;

/// Public writer handle backed by a private worker in the same process.
pub const COMMAND: &str = "__chigiri-stdio-writer-v1";
/// Maximum byte length of one transferred line.
pub const LINE_BYTES: usize = 16 * 1024;
