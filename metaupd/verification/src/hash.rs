//! Artifact hashing.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use metaupd_metadata::{Artifact, Sha256Digest};
use sha2::{Digest, Sha256};

pub fn sha256(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest(Sha256::digest(bytes).into())
}

/// Reads through to the end while hashing and counting bytes.
#[derive(Debug)]
pub struct HashingReader<R> {
    inner: R,
}

impl<R: Read> HashingReader<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }

    /// Returns the digest and length of the remaining input.
    pub fn finish(mut self) -> io::Result<(Sha256Digest, u64)> {
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            match self.inner.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    hasher.update(&buffer[..n]);
                    total += n as u64;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok((Sha256Digest(hasher.finalize().into()), total))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HashMismatch {
    #[error("cannot read {name}: {source}")]
    Io { name: String, source: io::Error },
    #[error("{name} is {actual} bytes, expected {expected}")]
    Size {
        name: String,
        expected: u64,
        actual: u64,
    },
    #[error("{name} has SHA-256 {actual}, expected {expected}")]
    Digest {
        name: String,
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
}

/// Checks a downloaded file against the size and digest in a verified manifest.
pub fn verify_file(path: &Path, artifact: &Artifact) -> Result<(), HashMismatch> {
    let name = artifact.name.to_string();
    let io = |source| HashMismatch::Io {
        name: name.clone(),
        source,
    };
    let file = File::open(path).map_err(io)?;
    // Reads one byte past the expected size so oversized files fail cheaply.
    let limited = file.take(artifact.size.saturating_add(1));
    let (digest, actual) = HashingReader::new(limited).finish().map_err(io)?;
    if actual != artifact.size {
        return Err(HashMismatch::Size {
            name,
            expected: artifact.size,
            actual,
        });
    }
    if digest != artifact.sha256 {
        return Err(HashMismatch::Digest {
            name,
            expected: artifact.sha256,
            actual: digest,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        assert_eq!(
            sha256(b"abc").to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let (digest, length) = HashingReader::new(&b"abc"[..]).finish().unwrap();
        assert_eq!((digest, length), (sha256(b"abc"), 3));
    }
}
