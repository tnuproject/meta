//! Quoting for the guest's shell.
//!
//! SSH hands the remote shell one string, so arguments have to be quoted for it.
//! Inside single quotes a POSIX shell interprets nothing.

/// Quotes one argument.
pub fn quote(argument: &str) -> String {
    let plain = !argument.is_empty()
        && argument
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-+:@%".contains(&b));
    if plain {
        argument.to_owned()
    } else {
        format!("'{}'", argument.replace('\'', "'\\''"))
    }
}

/// Names that can be variables.
pub fn valid_variable(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Builds the command line the guest's shell runs.
pub fn command_line(
    directory: Option<&str>,
    environment: &[(String, String)],
    command: &[String],
) -> String {
    let mut line = String::new();
    if let Some(directory) = directory {
        // Runs anyway when the directory does not exist in the guest.
        line.push_str(&format!("cd {} 2>/dev/null || cd; ", quote(directory)));
    }
    line.push_str("exec");
    let environment: Vec<_> = environment
        .iter()
        .filter(|(n, _)| valid_variable(n))
        .collect();
    if !environment.is_empty() {
        line.push_str(" env");
        for (name, value) in environment {
            line.push_str(&format!(" {name}={}", quote(value)));
        }
    }
    for argument in command {
        line.push(' ');
        line.push_str(&quote(argument));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_reach_a_real_shell_unchanged() {
        // The host's own shell stands in for the guest's.
        if cfg!(windows) {
            return;
        }
        let arguments = [
            "",
            "a b",
            "$(reboot)",
            "`id`",
            "it's",
            "\"q\"",
            "a\nb",
            "*",
            "\\",
            "é",
            "-n",
            "x;y",
        ];
        for argument in arguments {
            let command = vec!["printf".to_owned(), "%s".to_owned(), argument.to_owned()];
            let line = command_line(None, &[], &command);
            let output = std::process::Command::new("/bin/sh")
                .args(["-c", &line])
                .output()
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&output.stdout), argument, "{line}");
        }
    }

    #[test]
    fn environment_and_directory_are_quoted() {
        let environment = vec![
            ("A".to_owned(), "x y".to_owned()),
            ("B;reboot".to_owned(), "z".to_owned()),
        ];
        let line = command_line(Some("/a b'c"), &environment, &["p".to_owned()]);
        assert_eq!(
            line,
            "cd '/a b'\\''c' 2>/dev/null || cd; exec env A='x y' p"
        );
    }
}
