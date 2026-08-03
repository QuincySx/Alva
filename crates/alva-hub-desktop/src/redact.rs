// INPUT:  HandoffPackage text fields and transcript rows
// OUTPUT: redact_handoff — the single filter every cross-engine handoff passes
// POS:    Owner of the Hub sensitive-content policy; runs BEFORE persistence,
//         so stored packages equal what the target adapter actually receives.

use crate::store::HandoffPackage;

/// Filter one handoff in place. Applied before the package is persisted, so
/// "View handoff" and the target adapter always see the same filtered
/// content. Covers:
/// - credentials / tokens / key material in any free-text field,
/// - environment-style `NAME=value` assignments for sensitive names,
/// - adapter-private metadata (account/billing labels, team snapshot),
///   which no target protocol currently requires.
///
/// Hidden reasoning and approval tokens are excluded structurally: neither is
/// ever written to the store, so no handoff field can carry them.
pub fn redact_handoff(package: &mut HandoffPackage) {
    package.reason = redact_text(&package.reason);
    package.user_goal = package.user_goal.as_deref().map(redact_text);
    package.latest_instruction = package.latest_instruction.as_deref().map(redact_text);
    for decision in &mut package.accepted_decisions {
        *decision = redact_text(decision);
    }
    for work in &mut package.completed_work {
        work.prompt = redact_text(&work.prompt);
        work.result = redact_text(&work.result);
    }
    for work in &mut package.pending_work {
        work.prompt = redact_text(&work.prompt);
        work.reason = redact_text(&work.reason);
    }
    for command in &mut package.recent_commands {
        *command = redact_text(command);
    }
    for verification in &mut package.verification_results {
        *verification = redact_text(verification);
    }
    package.workspace_summary = redact_text(&package.workspace_summary);
    for turn in &mut package.transcript {
        turn.prompt = redact_text(&turn.prompt);
        turn.result = turn.result.as_deref().map(redact_text);
    }
    // Adapter-private metadata: another engine needs to know WHICH engine the
    // work came from, never whose account paid for it.
    package.source.account_source = String::new();
    package.source.billing_route = String::new();
    package.target.account_source = String::new();
    package.target.billing_route = String::new();
    package.team_snapshot = None;
}

const REDACTED: &str = "[redacted]";

/// Names whose assigned values are always sensitive.
const SENSITIVE_NAME_PARTS: [&str; 8] = [
    "key",
    "token",
    "secret",
    "password",
    "passwd",
    "credential",
    "auth",
    "bearer",
];

/// Replace credential-shaped substrings and sensitive assignments while
/// leaving surrounding prose intact. Handles PEM blocks (whole body),
/// `NAME=value`, `NAME: value`, `NAME = value`, quoted JSON pairs,
/// URL query parameters, tab separators, and `Bearer <token>` values.
pub fn redact_text(text: &str) -> String {
    let mut lines = Vec::new();
    let mut in_pem_block = false;
    for line in text.lines() {
        if line.contains("-----BEGIN") {
            in_pem_block = true;
            lines.push(REDACTED.to_string());
            continue;
        }
        if line.contains("-----END") {
            in_pem_block = false;
            lines.push(REDACTED.to_string());
            continue;
        }
        if in_pem_block {
            lines.push(REDACTED.to_string());
            continue;
        }
        lines.push(redact_line(line));
    }
    let mut output = lines.join("\n");
    if text.ends_with('\n') {
        output.push('\n');
    }
    output
}

/// Per-token expectation carried across a line.
enum Expect {
    /// Nothing pending.
    None,
    /// Previous token was a bare sensitive name; a separator may follow.
    SeparatorMaybe,
    /// The next non-separator token is a sensitive value.
    Value,
}

fn redact_line(line: &str) -> String {
    let mut expect = Expect::None;
    let tokens: Vec<String> = line
        .split(' ')
        .map(|token| redact_spaced_token(token, &mut expect))
        .collect();
    tokens.join(" ")
}

/// Tabs behave like spaces but are preserved in the output.
fn redact_spaced_token(token: &str, expect: &mut Expect) -> String {
    if token.contains('\t') {
        let parts: Vec<String> = token
            .split('\t')
            .map(|part| redact_token(part, expect))
            .collect();
        return parts.join("\t");
    }
    redact_token(token, expect)
}

fn redact_token(token: &str, expect: &mut Expect) -> String {
    if token.is_empty() {
        return String::new();
    }
    match std::mem::replace(expect, Expect::None) {
        Expect::Value => {
            // `Authorization: Bearer abc` — "Bearer" is a scheme, the value
            // is still coming.
            if bare_word(token).eq_ignore_ascii_case("bearer") {
                *expect = Expect::Value;
                return token.to_string();
            }
            return REDACTED.to_string();
        }
        Expect::SeparatorMaybe => {
            if token == "=" || token == ":" {
                *expect = Expect::Value;
                return token.to_string();
            }
            // No separator followed — the name was prose, fall through.
        }
        Expect::None => {}
    }

    let word = bare_word(token);
    if word.eq_ignore_ascii_case("bearer") {
        *expect = Expect::Value;
        return token.to_string();
    }
    // `password:` / `"api_key":` / `API_KEY=` — value arrives as next token.
    if let Some(stripped) = token.strip_suffix([':', '=']) {
        if is_sensitive_name(bare_word(stripped)) {
            *expect = Expect::Value;
            return token.to_string();
        }
    }
    // Bare sensitive name — sensitive only if a lone separator follows.
    if is_sensitive_name(word) && !token.contains(['=', ':']) {
        *expect = Expect::SeparatorMaybe;
        return token.to_string();
    }
    redact_inline(token)
}

/// Strip quoting/bracket noise from a token to get the word it carries.
fn bare_word(token: &str) -> &str {
    token.trim_matches(|character: char| {
        !character.is_ascii_alphanumeric() && character != '_' && character != '-'
    })
}

/// In-token assignments: scan EVERY `name<sep>value` occurrence (URL query
/// strings, JSON pairs, tab-free compounds), not just the first.
fn redact_inline(token: &str) -> String {
    if token.contains('&') {
        let parts: Vec<String> = token.split('&').map(redact_inline).collect();
        return parts.join("&");
    }
    let characters: Vec<char> = token.chars().collect();
    for (index, character) in characters.iter().enumerate() {
        if *character != '=' && *character != ':' {
            continue;
        }
        // Identifier chars immediately before the separator form the name;
        // closing quotes between name and separator ("api_key": …) are noise.
        let name: String = characters[..index]
            .iter()
            .rev()
            .skip_while(|candidate| **candidate == '"' || **candidate == '\'')
            .take_while(|candidate| candidate.is_ascii_alphanumeric() || **candidate == '_')
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        let value: String = characters[index + 1..].iter().collect();
        if !bare_word(&value).is_empty() && is_sensitive_name(&name) {
            let prefix: String = characters[..=index].iter().collect();
            return format!("{prefix}{REDACTED}");
        }
    }
    if is_secret_shaped(token) {
        return REDACTED.to_string();
    }
    token.to_string()
}

fn is_sensitive_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    SENSITIVE_NAME_PARTS.iter().any(|part| name.contains(part))
}

/// Well-known credential prefixes, anchored at the start of the word so
/// ordinary identifiers ("task-123", "risk-assessment") never match.
fn is_secret_shaped(token: &str) -> bool {
    let word = token.trim_start_matches(|character: char| !character.is_ascii_alphanumeric());
    const PREFIXES: [&str; 8] = [
        "sk-",
        "sk_live_",
        "ghp_",
        "gho_",
        "github_pat_",
        "AKIA",
        "xoxb-",
        "xoxp-",
    ];
    PREFIXES.iter().any(|prefix| {
        word.starts_with(prefix)
            && word[prefix.len()..]
                .chars()
                .take(8)
                .filter(char::is_ascii_alphanumeric)
                .count()
                >= 6
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{EngineTarget, HandoffKind, TranscriptTurn};

    #[test]
    fn credentials_and_env_values_are_redacted_from_text() {
        let cases = [
            (
                "run with API_KEY=sk-live-abc123 to reproduce",
                "run with API_KEY=[redacted] to reproduce",
            ),
            (
                "export GITHUB_TOKEN=ghp_16C7e42F292c6912E7710c8 && make",
                "export GITHUB_TOKEN=[redacted] && make",
            ),
            ("token: xoxb-2345678-secret", "token: [redacted]"),
            ("the key is sk-proj-AbCdEf123456", "the key is [redacted]"),
            ("aws id AKIAIOSFODNN7EXAMPLE done", "aws id [redacted] done"),
            ("-----BEGIN RSA PRIVATE KEY-----", "[redacted]"),
            (
                "plain text about running tests stays intact",
                "plain text about running tests stays intact",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(redact_text(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn mainstream_secret_shapes_are_all_caught() {
        // PEM body between markers is fully dropped.
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA7bq8+secretbody\n-----END RSA PRIVATE KEY-----";
        let redacted = redact_text(pem);
        assert!(!redacted.contains("MIIEpAIBAAKCAQEA7bq8"), "{redacted}");

        // `name: value` with a space — the universal log/YAML/header shape.
        let yaml = redact_text("password: hunter2secret");
        assert!(!yaml.contains("hunter2secret"), "{yaml}");
        let aws = redact_text("AWS_SECRET_ACCESS_KEY: wJalrXUtnFEMIK7MDENG");
        assert!(!aws.contains("wJalrXUtnFEMIK7MDENG"), "{aws}");

        // Bearer values, with and without a header name.
        let header = redact_text("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9abc");
        assert!(!header.contains("eyJhbGci"), "{header}");
        let bare = redact_text("use Bearer abc123def456 for the call");
        assert!(!bare.contains("abc123def456"), "{bare}");

        // TOML/INI spaced assignment.
        let toml = redact_text("api_key = \"hunter2secretvalue\"");
        assert!(!toml.contains("hunter2secretvalue"), "{toml}");

        // Pretty-printed JSON pair.
        let json = redact_text("{\"api_key\": \"hunter2secretvalue\"}");
        assert!(!json.contains("hunter2secretvalue"), "{json}");
        // Compact JSON pair.
        let compact = redact_text("{\"api_key\":\"hunter2secretvalue\"}");
        assert!(!compact.contains("hunter2secretvalue"), "{compact}");

        // URL query strings beyond the first parameter.
        let url = redact_text("GET /cb?page=1&access_token=abc123secret&x=2");
        assert!(!url.contains("abc123secret"), "{url}");
        assert!(url.contains("page=1"), "{url}");

        // Tab-separated assignments.
        let tabbed = redact_text("RUST_LOG=debug\tAPI_KEY=hunter2secret");
        assert!(!tabbed.contains("hunter2secret"), "{tabbed}");
        assert!(tabbed.contains("RUST_LOG=debug"), "{tabbed}");
    }

    #[test]
    fn ordinary_identifiers_and_paths_are_not_false_positives() {
        for text in [
            "task-1234567 is done",
            "risk-assessment complete",
            "/Users/x/desk-setup1/src/main.rs",
            "the password policy doc",
            "author John reviewed it",
        ] {
            assert_eq!(redact_text(text), text, "false positive on {text:?}");
        }
    }

    #[test]
    fn sensitive_names_cover_common_environment_variables() {
        for name in [
            "ALVA_API_KEY",
            "AUTH_TOKEN",
            "DB_PASSWORD",
            "MY_SECRET",
            "NPM_AUTH",
        ] {
            let text = format!("{name}=super-sensitive-value");
            let redacted = redact_text(&text);
            assert!(
                !redacted.contains("super-sensitive-value"),
                "{name} value leaked: {redacted}"
            );
        }
        // Non-sensitive assignments survive.
        assert_eq!(redact_text("RUST_LOG=debug"), "RUST_LOG=debug");
    }

    #[test]
    fn redact_handoff_filters_every_text_field_and_private_metadata() {
        let secret = "API_KEY=sk-live-canary9999";
        let mut package = HandoffPackage {
            handoff_id: "h-1".into(),
            product_session_id: "p-1".into(),
            source_branch_id: "b-1".into(),
            target_branch_id: "b-2".into(),
            source: EngineTarget::for_engine("alva").unwrap(),
            target: EngineTarget::for_engine("claude").unwrap(),
            kind: HandoffKind::Full,
            reason: secret.into(),
            user_goal: Some(secret.into()),
            latest_instruction: Some(secret.into()),
            accepted_decisions: vec![secret.into()],
            completed_work: vec![crate::store::HandoffCompletedWork {
                prompt: secret.into(),
                result: secret.into(),
            }],
            pending_work: vec![crate::store::HandoffPendingWork {
                prompt: secret.into(),
                reason: secret.into(),
            }],
            changed_files: vec!["src/main.rs".into()],
            recent_commands: vec![format!("curl -H {secret} https://api")],
            verification_results: vec![secret.into()],
            workspace_summary: secret.into(),
            known_side_effects: Vec::new(),
            team_snapshot: Some("private board".into()),
            transcript: vec![TranscriptTurn {
                prompt: secret.into(),
                result: Some(secret.into()),
            }],
            created_at: 0,
            used_at: None,
        };
        redact_handoff(&mut package);
        let encoded = serde_json::to_string(&package).unwrap();
        assert!(!encoded.contains("sk-live-canary9999"), "{encoded}");
        assert!(!encoded.contains("private board"));
        assert!(
            !encoded.contains("Workspace provider account")
                && !encoded.contains("Anthropic account via Claude CLI"),
            "billing labels leaked"
        );
        assert_eq!(package.source.account_source, "");
        assert_eq!(package.target.billing_route, "");
        assert!(package.team_snapshot.is_none());
        // Non-sensitive structure survives.
        assert_eq!(package.changed_files, vec!["src/main.rs".to_string()]);
    }
}
