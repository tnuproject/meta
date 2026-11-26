//! Running a program in a guest as if it ran here.
//!
//! Standard streams and the exit code pass through ssh. Signals do not, so they are
//! delivered by a second connection to the process whose number the first one left
//! in a file with an unguessable name.

use std::io::Read;

use crate::config::VmError;
use crate::guest::{Connection, Options};

/// Waits for the program instead of becoming it, so that a death by signal turns
/// into the usual 128 plus its number, which ssh alone does not report.
const START: &str = "umask 077; f=$HOME/.meta-run-$0; cd \"$1\" 2>/dev/null || cd; shift; \
    sh -c 'echo $$ >\"$0\"; exec \"$@\"' \"$f\" \"$@\"; r=$?; rm -f \"$f\"; exit $r";
const DELIVER: &str = "f=$HOME/.meta-run-$1; for i in 1 2 3 4 5; do [ -s \"$f\" ] && break; sleep 0.2; done; \
    [ -s \"$f\" ] && kill -s \"$2\" -- \"$(cat \"$f\")\"";

/// Signals passed on, with the names the guest's kill(1) uses.
#[cfg(unix)]
const SIGNALS: &[(i32, &str)] = &[
    (signal_hook::consts::SIGINT, "INT"),
    (signal_hook::consts::SIGTERM, "TERM"),
    (signal_hook::consts::SIGHUP, "HUP"),
    (signal_hook::consts::SIGQUIT, "QUIT"),
    (signal_hook::consts::SIGUSR1, "USR1"),
    (signal_hook::consts::SIGUSR2, "USR2"),
];

/// 128 bits from the kernel, as hexadecimal.
fn token() -> Result<String, VmError> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|source| VmError::Io {
            path: "/dev/urandom".into(),
            source,
        })?;
    Ok(hex::encode(bytes))
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

/// The command that starts `command` in `directory`, known by `token`.
fn start(token: &str, directory: &str, command: &[String]) -> Vec<String> {
    let mut full = strings(&["sh", "-c", START, token, directory]);
    full.extend(command.iter().cloned());
    full
}

/// Runs a program and returns its exit code.
#[cfg(unix)]
pub fn run(
    connection: &Connection,
    options: Options,
    directory: Option<&str>,
    environment: &[(String, String)],
    command: &[String],
) -> Result<i32, VmError> {
    use std::os::unix::process::ExitStatusExt;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI32, Ordering};

    let token = token()?;
    let failure = |e: std::io::Error| VmError::Guest(format!("cannot start ssh: {e}"));
    let numbers = SIGNALS.iter().map(|(number, _)| *number);
    let mut signals = signal_hook::iterator::Signals::new(numbers).map_err(failure)?;
    let handle = signals.handle();
    let received = Arc::new(AtomicI32::new(0));
    let deliver = {
        let (connection, token) = (connection.clone(), token.clone());
        let received = Arc::clone(&received);
        std::thread::spawn(move || {
            for number in signals.forever() {
                received.store(number, Ordering::SeqCst);
                if let Some((_, name)) = SIGNALS.iter().find(|(n, _)| *n == number) {
                    let _ = connection.capture(5, &["sh", "-c", DELIVER, "sh", &token, name]);
                }
            }
        })
    };
    let full = start(&token, directory.unwrap_or("."), command);
    let status = connection.command(options, environment, &full).status();
    handle.close();
    let _ = deliver.join();
    let status = status.map_err(failure)?;
    Ok(match status.code() {
        Some(code) => code,
        // A terminal sends its signals to ssh as well.
        None => {
            128 + status
                .signal()
                .unwrap_or_else(|| received.load(Ordering::SeqCst))
        }
    })
}

#[cfg(not(unix))]
pub fn run(
    connection: &Connection,
    options: Options,
    directory: Option<&str>,
    environment: &[(String, String)],
    command: &[String],
) -> Result<i32, VmError> {
    let _ = (token()?, DELIVER);
    let full = start("", directory.unwrap_or("."), command);
    let status = connection
        .command(options, environment, &full)
        .status()
        .map_err(|e| VmError::Guest(format!("cannot start ssh: {e}")))?;
    Ok(status.code().unwrap_or(crate::guest::SSH_FAILURE))
}

// Two attributes rather than all(test, unix), so that clippy sees test code.
#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::process::Command;

    fn local(home: &std::path::Path, command: &[&str]) -> std::process::Output {
        let full = start("abc", "/", &strings(command));
        Command::new("/bin/sh")
            .args(&full[1..])
            .env("HOME", home)
            .output()
            .unwrap()
    }

    #[test]
    fn results_are_those_of_the_program() {
        let home = tempfile::tempdir().unwrap();
        let output = local(
            home.path(),
            &["sh", "-c", "pwd; echo \"$1|$2\"; exit 7", "sh", "a b", "$x"],
        );
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "/\na b|$x\n");
        let killed = local(home.path(), &["sh", "-c", "kill -TERM $$"]);
        assert_eq!(killed.status.code(), Some(143));
        // 127 by POSIX; the /bin/sh of macOS says 126.
        let missing = local(home.path(), &["/nonexistent"]).status.code();
        assert!(matches!(missing, Some(126 | 127)), "{missing:?}");
        assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
    }

    #[test]
    fn signals_reach_the_program() {
        let home = tempfile::tempdir().unwrap();
        let script = "trap 'echo got; exit 9' USR1; while :; do sleep 0.1; done";
        let full = start("abc", "/", &strings(&["sh", "-c", script]));
        let child = Command::new("/bin/sh")
            .args(&full[1..])
            .env("HOME", home.path())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let sent = Command::new("/bin/sh")
            .args(["-c", DELIVER, "sh", "abc", "USR1"])
            .env("HOME", home.path())
            .status()
            .unwrap();
        assert!(sent.success());
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(9));
        assert_eq!(output.stdout, b"got\n");
    }
}
