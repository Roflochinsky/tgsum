use std::io::{self, Read};

/// Validate read chunks without retaining the document. At most three bytes
/// of an incomplete scalar cross a chunk boundary. Deliver a valid prefix
/// before a deferred error: callers may deliberately stop parsing before it.
pub(super) struct Utf8Read<R> {
    inner: R,
    partial: [u8; 4],
    partial_len: usize,
    failed: bool,
}

impl<R> Utf8Read<R> {
    pub(super) fn new(inner: R) -> Self {
        Self {
            inner,
            partial: [0; 4],
            partial_len: 0,
            failed: false,
        }
    }

    fn error(&mut self) -> io::Error {
        self.failed = true;
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid UTF-8 in Telegram export",
        )
    }
}

impl<R: Read> Read for Utf8Read<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.failed {
            return Err(self.error());
        }
        let n = self.inner.read(out)?;
        if n == 0 {
            return if self.partial_len == 0 {
                Ok(0)
            } else {
                Err(self.error())
            };
        }
        let mut offset = 0;
        if self.partial_len != 0 {
            // from_utf8 only reports an incomplete suffix for a valid leading
            // byte and continuation prefix; the total width is therefore known.
            let width = match self.partial[0] {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                _ => 4,
            };
            offset = n.min(width - self.partial_len);
            self.partial[self.partial_len..self.partial_len + offset]
                .copy_from_slice(&out[..offset]);
            self.partial_len += offset;
            match std::str::from_utf8(&self.partial[..self.partial_len]) {
                Ok(_) => self.partial_len = 0,
                Err(e) if e.error_len().is_none() => return Ok(n),
                Err(_) => return Err(self.error()),
            }
        }
        match std::str::from_utf8(&out[offset..n]) {
            Ok(_) => Ok(n),
            Err(error) => {
                let valid = offset + error.valid_up_to();
                if error.error_len().is_none() {
                    self.partial_len = n - valid;
                    self.partial[..self.partial_len].copy_from_slice(&out[valid..n]);
                    Ok(n)
                } else {
                    let error = self.error();
                    if valid == 0 {
                        Err(error)
                    } else {
                        Ok(valid)
                    }
                }
            }
        }
    }
}
