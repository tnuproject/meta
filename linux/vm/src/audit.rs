//! A record of what was done to machines.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Appends one line. Failing to record is reported but does not stop the operation:
/// refusing to stop a machine because a disk is full would be worse.
pub fn record(directory: &Path, user: &str, action: &str, subject: &str, success: bool) {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let clean = |text: &str| -> String {
        text.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .take(200)
            .collect()
    };
    let outcome = if success { "ok" } else { "failed" };
    let line = format!(
        "{seconds}\t{}\t{}\t{}\t{outcome}\n",
        clean(user),
        clean(action),
        clean(subject)
    );
    let path = directory.join("audit.log");
    let result = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(line.as_bytes()));
    if let Err(error) = result {
        eprintln!("warning: {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_per_action_whatever_the_text() {
        let dir = tempfile::tempdir().unwrap();
        record(dir.path(), "alice", "start", "a\nforged\tline", true);
        record(dir.path(), "alice", "stop", "b", false);
        let text = std::fs::read_to_string(dir.path().join("audit.log")).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.lines().last().unwrap().ends_with("failed"));
    }
}
