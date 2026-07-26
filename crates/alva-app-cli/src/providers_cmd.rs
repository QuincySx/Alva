// INPUT:  alva_app_core::config, settings_cmd (kind default tables)
// OUTPUT: pub async fn run — `alva providers list [--output-format json]`
// POS:    The orchestrator-facing provider discovery surface, sibling of
//         `alva tools list`. `settings list`/`get` are the HUMAN views
//         (config detail, masked keys); this is the MACHINE view and its
//         fields are a strict whitelist: name + model + active. Endpoints
//         and anything key-shaped stay out of the machine channel entirely
//         (user ruling) — picking a name is all a dispatch decision needs.

use alva_app_core::config;

pub async fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        None => list(&[]),
        Some("list") => list(&args[1..]),
        Some("help") | Some("-h") | Some("--help") => {
            eprintln!("usage: alva providers list [--output-format json]");
            eprintln!("Lists configured provider profiles (names for `--provider <name>`).");
            0
        }
        Some(other) => {
            eprintln!("alva providers: unknown subcommand `{other}`");
            eprintln!("usage: alva providers list [--output-format json]");
            2
        }
    }
}

fn list(args: &[String]) -> i32 {
    let json_mode = match parse_output_format(args) {
        Ok(mode) => mode,
        Err(error) => {
            eprintln!("Error: {error}");
            return 2;
        }
    };

    let cfg = config::load().unwrap_or_default();
    let active = cfg.active.as_deref();
    let mut names: Vec<&String> = cfg.providers.keys().collect();
    names.sort();

    // The effective model (entry value or kind default) — what a dispatch
    // WILL use. Nothing else crosses into the machine channel.
    let rows: Vec<serde_json::Value> = names
        .iter()
        .map(|name| {
            let entry = &cfg.providers[*name];
            let kind = entry.effective_kind(name);
            serde_json::json!({
                "name": name,
                "model": entry
                    .model
                    .clone()
                    .unwrap_or_else(|| crate::settings_cmd::default_model_for_kind(kind).to_string()),
                "active": active == Some(name.as_str()),
            })
        })
        .collect();

    if json_mode {
        println!("{}", serde_json::json!(rows));
        return 0;
    }

    if rows.is_empty() {
        eprintln!("No provider profiles configured.");
        eprintln!(
            "Add one: alva settings set deepseek --kind openai-chat --api-key ... --base-url ..."
        );
        return 0;
    }
    for row in &rows {
        println!(
            "{star} {name:<20}  {model}",
            star = if row["active"].as_bool() == Some(true) {
                "*"
            } else {
                " "
            },
            name = row["name"].as_str().unwrap_or(""),
            model = row["model"].as_str().unwrap_or(""),
        );
    }
    0
}

fn parse_output_format(args: &[String]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        [flag, value] if flag == "--output-format" && value == "json" => Ok(true),
        [flag, value] if flag == "--output-format" && value == "text" => Ok(false),
        [flag, value] if flag == "--output-format" => Err(format!(
            "--output-format expects `text` or `json`, got {value:?}"
        )),
        _ => Err("usage: alva providers list [--output-format <text|json>]".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn output_format_parser_is_strict() {
        assert_eq!(parse_output_format(&[]), Ok(false));
        assert_eq!(
            parse_output_format(&args(&["--output-format", "json"])),
            Ok(true)
        );
        assert!(parse_output_format(&args(&["--output-format"])).is_err());
        assert!(parse_output_format(&args(&["--output-format", "yaml"])).is_err());
        assert!(parse_output_format(&args(&["--typo"])).is_err());
    }
}
