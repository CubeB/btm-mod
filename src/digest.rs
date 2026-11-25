//! SHA-256 digests.

use std::fmt;
use std::io::{self, Read};

use sha2::{Digest as _, Sha256};

/// A SHA-256 digest, written as 64 lowercase hex digits.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Digest(pub [u8; 32]);

impl Digest {
    /// The digest of `bytes`.
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// The digest of everything `r` yields.
    pub fn of_reader(mut r: impl Read) -> io::Result<Self> {
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = r.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(buf.get(..n).unwrap_or_default());
        }
        Ok(Self(h.finalize().into()))
    }

    /// Parses exactly 64 lowercase hex digits.
    pub fn from_hex(s: &str) -> Option<Self> {
        let bytes = s.as_bytes();
        if bytes.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
            let hi = nibble(pair[0])?;
            let lo = nibble(pair[1])?;
            *out.get_mut(i)? = (hi << 4) | lo;
        }
        Some(Self(out))
    }

    /// 64 lowercase hex digits.
    pub fn to_hex(&self) -> String {
        self.to_string()
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn known_vector_and_hex_round_trip() {
        let d = Digest::of(b"abc");
        assert_eq!(
            d.to_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(Digest::from_hex(&d.to_hex()), Some(d));
        assert_eq!(Digest::of_reader(&b"abc"[..]).unwrap(), d);
    }

    #[test]
    fn rejects_bad_hex() {
        assert!(Digest::from_hex("").is_none());
        assert!(Digest::from_hex(&"A".repeat(64)).is_none());
        assert!(Digest::from_hex(&"0".repeat(63)).is_none());
        assert!(Digest::from_hex(&"g".repeat(64)).is_none());
    }
}
