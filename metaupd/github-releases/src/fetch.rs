//! HTTPS transport using the platform's own client.
//!
//! FreeBSD ships fetch(1) with TLS and a CA bundle in the base system, so
//! metaupd does not carry a second TLS stack. Development hosts use curl.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::SourceError;

pub trait Fetcher {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, SourceError>;
    fn download(&self, url: &str, destination: &Path, limit: u64) -> Result<(), SourceError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CommandFetcher;

const USER_AGENT: &str = "metaupd";
const TIMEOUT_SECONDS: &str = "60";
/// How often a running download is checked against its size limit.
const POLL: Duration = Duration::from_millis(100);

impl CommandFetcher {
    fn command(resume_into: Option<&Path>, url: &str) -> Command {
        if cfg!(target_os = "freebsd") {
            let mut command = Command::new("/usr/bin/fetch");
            command.arg("-q").arg("-T").arg(TIMEOUT_SECONDS);
            command.arg(format!("--user-agent={USER_AGENT}"));
            match resume_into {
                Some(path) => command.arg("-r").arg("-o").arg(path),
                None => command.arg("-o").arg("-"),
            };
            command.arg(url);
            // Settings that would add credentials or disable TLS checks.
            for name in [
                "HTTP_AUTH",
                "NETRC",
                "SSL_NO_VERIFY_PEER",
                "SSL_NO_VERIFY_HOSTNAME",
            ] {
                command.env_remove(name);
            }
            command
        } else {
            let mut command = Command::new("curl");
            command.args([
                "-q",
                "-fsSL",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
            ]);
            command.args([
                "--connect-timeout",
                TIMEOUT_SECONDS,
                "--user-agent",
                USER_AGENT,
            ]);
            if let Some(path) = resume_into {
                command.args(["-C", "-", "-o"]).arg(path);
            }
            command.arg("--").arg(url);
            command
        }
    }
}

fn require_https(url: &str) -> Result<(), SourceError> {
    if url.starts_with("https://") && url.bytes().all(|b| b.is_ascii_graphic()) {
        Ok(())
    } else {
        Err(SourceError::Transport(format!(
            "refusing non-HTTPS URL {url:?}"
        )))
    }
}

fn spawn_error(error: std::io::Error) -> SourceError {
    SourceError::Transport(format!("could not start the HTTPS client: {error}"))
}

impl Fetcher for CommandFetcher {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, SourceError> {
        require_https(url)?;
        let mut child = Self::command(None, url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(spawn_error)?;
        let mut body = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            stdout
                .take(limit.saturating_add(1))
                .read_to_end(&mut body)?;
        }
        if body.len() as u64 > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SourceError::TooLarge {
                what: url.to_owned(),
                limit,
            });
        }
        let output = child.wait_with_output()?;
        if output.status.success() {
            Ok(body)
        } else {
            Err(transport_failure(url, &output.stderr))
        }
    }

    /// Stops the transfer once the file outgrows `limit`, so a hostile server
    /// cannot fill the disk whatever length it announces.
    fn download(&self, url: &str, destination: &Path, limit: u64) -> Result<(), SourceError> {
        require_https(url)?;
        let mut child = Self::command(Some(destination), url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(spawn_error)?;
        let finished = loop {
            let finished = child.try_wait()?.is_some();
            if fs::metadata(destination).is_ok_and(|m| m.len() > limit) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(destination);
                return Err(SourceError::TooLarge {
                    what: url.to_owned(),
                    limit,
                });
            }
            if finished {
                break child.wait_with_output()?;
            }
            std::thread::sleep(POLL);
        };
        if finished.status.success() {
            Ok(())
        } else {
            Err(transport_failure(url, &finished.stderr))
        }
    }
}

fn transport_failure(url: &str, stderr: &[u8]) -> SourceError {
    let detail = String::from_utf8_lossy(stderr);
    let detail = detail.trim();
    let detail = if detail.is_empty() {
        "request failed"
    } else {
        detail
    };
    SourceError::Transport(format!("{url}: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_other_schemes() {
        for url in [
            "http://github.com/x",
            "file:///etc/passwd",
            "-o/tmp/x",
            "https://a b",
        ] {
            assert!(CommandFetcher.get(url, 10).is_err(), "{url}");
            assert!(
                CommandFetcher
                    .download(url, Path::new("unused"), 10)
                    .is_err(),
                "{url}"
            );
        }
    }
}
