//! Only examines the initial cleartext ClientHello. No TLS session keys,
//! decrypted payload, certificate validation or HTTP path inspection here.
use std::collections::BTreeSet;

use super::Failure;

struct Cursor<'a>(&'a [u8]);
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Failure> {
        if n > self.0.len() {
            return Err(Failure::ClientHello);
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn byte(&mut self) -> Result<usize, Failure> {
        Ok(self.take(1)?[0] as usize)
    }
    fn word(&mut self) -> Result<usize, Failure> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]) as usize)
    }
    fn vector8(&mut self) -> Result<&'a [u8], Failure> {
        let n = self.byte()?;
        self.take(n)
    }
    fn vector16(&mut self) -> Result<&'a [u8], Failure> {
        let n = self.word()?;
        self.take(n)
    }
    fn end(&self) -> Result<(), Failure> {
        self.0.is_empty().then_some(()).ok_or(Failure::ClientHello)
    }
}

pub(super) fn validate(handshake: &[u8], host: &str) -> Result<(), Failure> {
    let mut hello = Cursor(handshake);
    if hello.take(2)? != [3, 3] {
        return Err(Failure::ClientHello);
    }
    hello.take(32)?;
    if hello.vector8()?.len() > 32 {
        return Err(Failure::ClientHello);
    }
    let suites = hello.vector16()?;
    if suites.is_empty() || suites.len() % 2 != 0 || hello.vector8()? != [0] {
        return Err(Failure::ClientHello);
    }
    let mut extensions = Cursor(hello.vector16()?);
    hello.end()?;
    let mut seen = BTreeSet::new();
    let mut matched = false;
    while !extensions.0.is_empty() {
        let kind = extensions.word()?;
        let body = extensions.vector16()?;
        if !seen.insert(kind) {
            return Err(Failure::ClientHello);
        }
        match kind {
            0xfe0d => return Err(Failure::EncryptedHello), // ECH, including GREASE.
            0 => {
                let mut names = Cursor(body);
                let mut name = Cursor(names.vector16()?);
                names.end()?;
                if name.byte()? != 0 || name.vector16()? != host.as_bytes() {
                    return Err(Failure::ServerName);
                }
                name.end()?; // Exactly one host_name, no alternate name types.
                matched = true;
            }
            _ => {}
        }
    }
    matched.then_some(()).ok_or(Failure::ServerName)
}
