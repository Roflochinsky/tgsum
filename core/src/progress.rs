//! A reader wrapper that reports progress and supports cancellation.

use std::fmt;
use std::io::{self, Read};

/// Wraps a reader and calls `on_read(total_bytes_so_far)` after every read.
/// Returning an error from the callback aborts the read with that error;
/// return [`cancelled()`] to signal a user cancellation.
pub struct ProgressReader<R, F> {
    inner: R,
    read: u64,
    on_read: F,
}

impl<R, F> ProgressReader<R, F>
where
    R: Read,
    F: FnMut(u64) -> io::Result<()>,
{
    pub fn new(inner: R, on_read: F) -> Self {
        Self {
            inner,
            read: 0,
            on_read,
        }
    }
}

impl<R, F> Read for ProgressReader<R, F>
where
    R: Read,
    F: FnMut(u64) -> io::Result<()>,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        (self.on_read)(self.read)?;
        Ok(n)
    }
}

#[derive(Debug)]
struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("отменено")
    }
}

impl std::error::Error for Cancelled {}

/// The error a progress callback returns to cancel a pass. (Not
/// `ErrorKind::Interrupted`: `std::io` readers silently retry on that.)
pub fn cancelled() -> io::Error {
    io::Error::other(Cancelled)
}

/// Whether `err` is the cancellation from [`cancelled`].
pub fn is_cancelled(err: &io::Error) -> bool {
    err.get_ref().is_some_and(|e| e.is::<Cancelled>())
}
