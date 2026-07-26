// INPUT:  process argv strings
// OUTPUT: strict top-level invocation grammar validation
// POS:    CLI parsing guard — rejects ambiguous flags before provider setup.

/// Validate the top-level invocation grammar before provider setup or agent
/// construction. The CLI intentionally keeps a small hand-written parser, so
/// this pass prevents unknown flags, duplicate singleton options, and missing
/// values from being silently reinterpreted as model prompt text.
pub(crate) fn validate_invocation_args(args: &[String]) -> Result<(), String> {
    let mut seen_singletons: Vec<&str> = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            return Ok(());
        }
        match arg {
            "-p" | "--print" | "--repl" | "--tui" | "--dangerously-allow-unsandboxed" => index += 1,
            "--grant" | "--allow-domain" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| format!("{arg} requires a value"))?;
                if value.starts_with('-') {
                    return Err(format!("{arg} requires a value, got flag {value:?}"));
                }
                index += 2;
            }
            "--sandbox"
            | "--provider"
            | "--system-prompt"
            | "--max-turns"
            | "--permission-mode"
            | "--output-format"
            | "--resume"
            | "--allowed-tools"
            | "--ui-mode"
            | "--wasm-wall-clock-secs"
            | "--wasm-fuel" => {
                if seen_singletons.contains(&arg) {
                    return Err(format!("{arg} may only be specified once"));
                }
                seen_singletons.push(arg);
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| format!("{arg} requires a value"))?;
                if value.starts_with('-') {
                    return Err(format!("{arg} requires a value, got flag {value:?}"));
                }
                index += 2;
            }
            value if value.starts_with('-') => {
                return Err(format!(
                    "unknown flag {value:?}; use `--` before prompt text that starts with `-`"
                ));
            }
            _ => index += 1,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn rejects_unknown_missing_and_duplicate_flags() {
        assert!(
            validate_invocation_args(&argv(&["alva", "-p", "--outpt-format", "json"]))
                .unwrap_err()
                .contains("unknown flag")
        );
        assert!(
            validate_invocation_args(&argv(&["alva", "-p", "--system-prompt"]))
                .unwrap_err()
                .contains("requires a value")
        );
        assert!(validate_invocation_args(&argv(&[
            "alva",
            "-p",
            "--provider",
            "fast",
            "--provider",
            "slow",
            "prompt",
        ]))
        .unwrap_err()
        .contains("only be specified once"));
        assert!(validate_invocation_args(&argv(&[
            "alva",
            "-p",
            "--max-turns",
            "--permission-mode",
            "ask",
            "prompt",
        ]))
        .unwrap_err()
        .contains("got flag"));
    }

    #[test]
    fn allows_repeated_capabilities_and_dash_prompt_after_delimiter() {
        validate_invocation_args(&argv(&[
            "alva",
            "-p",
            "--grant",
            "/one",
            "--grant",
            "/two",
            "--",
            "--literal-prompt",
        ]))
        .unwrap();
    }
}
