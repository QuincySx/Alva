// INPUT:  (none — standalone module)
// OUTPUT: BashClassifier, CommandClassification
// POS:    Classifies bash commands as read-only, destructive, or unknown to assist permission decisions.

/// Classifies bash commands as safe or potentially dangerous.
///
/// Used by the permission system to auto-approve read-only commands
/// and flag destructive commands for denial or elevated review.
pub struct BashClassifier;

impl BashClassifier {
    /// Classify a bash command.
    pub fn classify(command: &str) -> CommandClassification {
        let trimmed = command.trim();

        if trimmed.is_empty() {
            return CommandClassification::ReadOnly;
        }

        // Check for destructive patterns first
        if is_destructive(trimmed) {
            return CommandClassification::Destructive;
        }

        // Check for read-only commands
        if is_read_only(trimmed) {
            return CommandClassification::ReadOnly;
        }

        // Default: potentially unsafe
        CommandClassification::Unknown
    }
}

/// Classification of a bash command's safety level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClassification {
    /// Command only reads data, safe to auto-approve.
    ReadOnly,
    /// Command is destructive (rm -rf, git push --force, etc.).
    Destructive,
    /// Command classification unknown, requires user approval.
    Unknown,
}

/// Read-only commands that are safe to auto-approve.
const READ_ONLY_COMMANDS: &[&str] = &[
    "ls", "cat", "head", "tail", "less", "more", "wc", "file",
    "find", "grep", "rg", "ag", "ack", "fd",
    "which", "whereis", "type", "command",
    "echo", "printf", "date", "cal", "uptime",
    "pwd", "whoami", "hostname", "uname",
    "env", "printenv", "set",
    "df", "du", "free", "top", "ps", "lsof",
    "jq", "yq",
    "tree", "stat", "md5sum", "sha256sum",
];

/// Read-only compound commands (multi-word prefixes).
const READ_ONLY_COMPOUND: &[&str] = &[
    "git status",
    "git log",
    "git diff",
    "git show",
    "git branch",
    "git remote",
    "git tag",
    "git stash list",
    "cargo check",
    "cargo test",
    "cargo clippy",
    "cargo doc",
    "npm test",
    "npm run lint",
    "npx",
    "yarn test",
    "python -c",
    "node -e",
    "ruby -e",
    "curl",
    "wget",
    "ping",
    "dig",
    "nslookup",
    "host",
    "sed -n",
    "awk",
];

/// Destructive patterns that should be flagged.
const DESTRUCTIVE_PATTERNS: &[&str] = &[
    "rm -rf",
    "rm -fr",
    "rmdir",
    "git push --force",
    "git push -f",
    "git reset --hard",
    "git clean -f",
    "git checkout -- .",
    "git restore .",
    "drop database",
    "drop table",
    "truncate table",
    "kill -9",
    "killall",
    "pkill",
    "shutdown",
    "reboot",
    "mkfs",
    "dd if=",
    "chmod -r 777",
    "> /dev/",
    ":(){ :|:& };:",
];

fn is_read_only(cmd: &str) -> bool {
    let first_word = cmd.split_whitespace().next().unwrap_or("");

    // Check single-word read-only commands
    if READ_ONLY_COMMANDS.iter().any(|rc| first_word == *rc) {
        return true;
    }

    // Check compound read-only commands
    if READ_ONLY_COMPOUND.iter().any(|rc| cmd.starts_with(rc)) {
        return true;
    }

    false
}

fn is_destructive(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    DESTRUCTIVE_PATTERNS.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Read-only commands ----

    #[test]
    fn ls_is_read_only() {
        assert_eq!(BashClassifier::classify("ls"), CommandClassification::ReadOnly);
        assert_eq!(BashClassifier::classify("ls -la"), CommandClassification::ReadOnly);
        assert_eq!(BashClassifier::classify("ls /tmp"), CommandClassification::ReadOnly);
    }

    #[test]
    fn cat_is_read_only() {
        assert_eq!(BashClassifier::classify("cat file.txt"), CommandClassification::ReadOnly);
    }

    #[test]
    fn grep_is_read_only() {
        assert_eq!(BashClassifier::classify("grep -r pattern ."), CommandClassification::ReadOnly);
    }

    #[test]
    fn git_status_is_read_only() {
        assert_eq!(BashClassifier::classify("git status"), CommandClassification::ReadOnly);
    }

    #[test]
    fn git_log_is_read_only() {
        assert_eq!(BashClassifier::classify("git log --oneline"), CommandClassification::ReadOnly);
    }

    #[test]
    fn git_diff_is_read_only() {
        assert_eq!(BashClassifier::classify("git diff HEAD"), CommandClassification::ReadOnly);
    }

    #[test]
    fn cargo_check_is_read_only() {
        assert_eq!(BashClassifier::classify("cargo check"), CommandClassification::ReadOnly);
        assert_eq!(BashClassifier::classify("cargo test"), CommandClassification::ReadOnly);
        assert_eq!(BashClassifier::classify("cargo clippy"), CommandClassification::ReadOnly);
    }

    #[test]
    fn echo_is_read_only() {
        assert_eq!(BashClassifier::classify("echo hello"), CommandClassification::ReadOnly);
    }

    #[test]
    fn pwd_is_read_only() {
        assert_eq!(BashClassifier::classify("pwd"), CommandClassification::ReadOnly);
    }

    #[test]
    fn empty_command_is_read_only() {
        assert_eq!(BashClassifier::classify(""), CommandClassification::ReadOnly);
        assert_eq!(BashClassifier::classify("  "), CommandClassification::ReadOnly);
    }

    // ---- Destructive commands ----

    #[test]
    fn rm_rf_is_destructive() {
        assert_eq!(BashClassifier::classify("rm -rf /"), CommandClassification::Destructive);
        assert_eq!(BashClassifier::classify("rm -rf ."), CommandClassification::Destructive);
    }

    #[test]
    fn rm_fr_is_destructive() {
        assert_eq!(BashClassifier::classify("rm -fr /tmp/*"), CommandClassification::Destructive);
    }

    #[test]
    fn git_push_force_is_destructive() {
        assert_eq!(
            BashClassifier::classify("git push --force origin main"),
            CommandClassification::Destructive
        );
        assert_eq!(
            BashClassifier::classify("git push -f origin main"),
            CommandClassification::Destructive
        );
    }

    #[test]
    fn git_reset_hard_is_destructive() {
        assert_eq!(
            BashClassifier::classify("git reset --hard HEAD~1"),
            CommandClassification::Destructive
        );
    }

    #[test]
    fn git_clean_f_is_destructive() {
        assert_eq!(
            BashClassifier::classify("git clean -fd"),
            CommandClassification::Destructive
        );
    }

    #[test]
    fn kill_9_is_destructive() {
        assert_eq!(BashClassifier::classify("kill -9 1234"), CommandClassification::Destructive);
    }

    #[test]
    fn killall_is_destructive() {
        assert_eq!(BashClassifier::classify("killall node"), CommandClassification::Destructive);
    }

    #[test]
    fn drop_database_is_destructive() {
        assert_eq!(
            BashClassifier::classify("psql -c 'DROP DATABASE mydb'"),
            CommandClassification::Destructive
        );
    }

    #[test]
    fn dd_is_destructive() {
        assert_eq!(
            BashClassifier::classify("dd if=/dev/zero of=/dev/sda"),
            CommandClassification::Destructive
        );
    }

    #[test]
    fn chmod_777_is_destructive() {
        assert_eq!(
            BashClassifier::classify("chmod -R 777 /"),
            CommandClassification::Destructive
        );
    }

    // ---- Unknown commands ----

    #[test]
    fn unknown_commands() {
        assert_eq!(BashClassifier::classify("make build"), CommandClassification::Unknown);
        assert_eq!(BashClassifier::classify("npm install"), CommandClassification::Unknown);
        assert_eq!(BashClassifier::classify("docker build ."), CommandClassification::Unknown);
        assert_eq!(BashClassifier::classify("apt-get install vim"), CommandClassification::Unknown);
    }

    #[test]
    fn git_commit_is_unknown() {
        assert_eq!(
            BashClassifier::classify("git commit -m 'message'"),
            CommandClassification::Unknown
        );
    }

    #[test]
    fn git_push_without_force_is_unknown() {
        assert_eq!(
            BashClassifier::classify("git push origin main"),
            CommandClassification::Unknown
        );
    }

    // ---- Case sensitivity ----

    #[test]
    fn destructive_patterns_are_case_insensitive() {
        assert_eq!(
            BashClassifier::classify("RM -RF /"),
            CommandClassification::Destructive
        );
        assert_eq!(
            BashClassifier::classify("Git Push --Force"),
            CommandClassification::Destructive
        );
    }
}
