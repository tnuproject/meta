//! Running system commands behind a trait so parsers can be tested offline.

use std::collections::HashMap;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Exit code, or -1 when the process was terminated by a signal.
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.code == 0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("{program} is not available on this system")]
    NotFound { program: String },
    #[error("could not run {program}: {source}")]
    Io {
        program: String,
        source: std::io::Error,
    },
    #[error("{program} failed with exit code {code}: {stderr}")]
    Failed {
        program: String,
        code: i32,
        stderr: String,
    },
}

pub trait CommandRunner {
    /// Runs `program` and returns its output whatever the exit code.
    ///
    /// Callers pass absolute paths for system tools so that `PATH` cannot
    /// substitute a different program in privileged contexts.
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError>;

    /// Runs `program` and returns its standard output, failing on a non-zero exit.
    fn stdout(&self, program: &str, args: &[&str]) -> Result<String, CommandError> {
        let output = self.run(program, args)?;
        if output.success() {
            Ok(output.stdout)
        } else {
            Err(CommandError::Failed {
                program: program.to_owned(),
                code: output.code,
                stderr: output.stderr.trim().to_owned(),
            })
        }
    }

    /// Runs a command with `input` on its standard input, for secrets that must
    /// not appear in arguments. Runners without real processes do not support it.
    fn feed(&self, program: &str, args: &[&str], input: &[u8]) -> Result<(), CommandError> {
        let _ = (args, input);
        Err(CommandError::NotFound {
            program: program.to_owned(),
        })
    }

    /// Runs a long command with its output visible to the user.
    fn inherit(&self, program: &str, args: &[&str]) -> Result<(), CommandError> {
        self.stdout(program, args).map(drop)
    }
}

/// Runs real commands with a fixed locale.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
        let output = Command::new(program)
            .args(args)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output()
            .map_err(|source| match source.kind() {
                std::io::ErrorKind::NotFound => CommandError::NotFound {
                    program: program.to_owned(),
                },
                _ => CommandError::Io {
                    program: program.to_owned(),
                    source,
                },
            })?;
        Ok(CommandOutput {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn feed(&self, program: &str, args: &[&str], input: &[u8]) -> Result<(), CommandError> {
        use std::io::Write;
        let io = |source| CommandError::Io {
            program: program.to_owned(),
            source,
        };
        let mut child = Command::new(program)
            .args(args)
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(io)?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input).map_err(io)?;
        }
        let output = child.wait_with_output().map_err(io)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(CommandError::Failed {
                program: program.to_owned(),
                code: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }

    fn inherit(&self, program: &str, args: &[&str]) -> Result<(), CommandError> {
        let status = Command::new(program)
            .args(args)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .status()
            .map_err(|source| match source.kind() {
                std::io::ErrorKind::NotFound => CommandError::NotFound {
                    program: program.to_owned(),
                },
                _ => CommandError::Io {
                    program: program.to_owned(),
                    source,
                },
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(CommandError::Failed {
                program: program.to_owned(),
                code: status.code().unwrap_or(-1),
                stderr: "see its output above".to_owned(),
            })
        }
    }
}

/// Replays canned output; commands that were not registered are "not found".
#[derive(Debug, Clone, Default)]
pub struct FakeRunner {
    responses: HashMap<String, CommandOutput>,
}

impl FakeRunner {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, command_line: &str, stdout: &str) -> Self {
        self.responses.insert(
            command_line.to_owned(),
            CommandOutput {
                code: 0,
                stdout: stdout.to_owned(),
                stderr: String::new(),
            },
        );
        self
    }

    #[must_use]
    pub fn with_failure(mut self, command_line: &str, code: i32, stderr: &str) -> Self {
        self.responses.insert(
            command_line.to_owned(),
            CommandOutput {
                code,
                stdout: String::new(),
                stderr: stderr.to_owned(),
            },
        );
        self
    }
}

impl CommandRunner for FakeRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
        let line = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        self.responses
            .get(&line)
            .cloned()
            .ok_or_else(|| CommandError::NotFound {
                program: program.to_owned(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_program_is_reported_as_not_found() {
        let err = SystemRunner
            .run("/nonexistent/meta-test-program", &[])
            .unwrap_err();
        assert!(matches!(err, CommandError::NotFound { .. }));
    }

    #[test]
    fn stdout_rejects_failures() {
        let runner = FakeRunner::new().with_failure("tool -x", 2, "bad\n");
        let err = runner.stdout("tool", &["-x"]).unwrap_err();
        assert_eq!(err.to_string(), "tool failed with exit code 2: bad");
    }
}
