// INPUT:  alva_app_core::config, settings_cmd (kind default tables)
// OUTPUT: pub async fn run — `alva providers list [--output-format json]`
// POS:    The orchestrator-facing provider discovery surface, sibling of
//         `alva tools list`. `settings list` is the HUMAN view (stderr,
//         masked keys); this is the MACHINE view: stdout, JSON, effective
//         values (defaults resolved), and no key material at all — only a
//         `has_key` boolean, which is what an allowlist/dispatch decision
//         actually needs.

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
    let json_mode = matches!(
        args.iter().position(|a| a == "--output-format"),
        Some(i) if args.get(i + 1).map(String::as_str) == Some("json")
    );

    let cfg = config::load().unwrap_or_default();
    let active = cfg.active.as_deref();
    let mut names: Vec<&String> = cfg.providers.keys().collect();
    names.sort();

    // Effective values throughout: an orchestrator wants to know what a
    // dispatch WILL use, not which fields happen to be unset.
    let rows: Vec<serde_json::Value> = names
        .iter()
        .map(|name| {
            let entry = &cfg.providers[*name];
            let kind = entry.effective_kind(name);
            serde_json::json!({
                "name": name,
                "kind": kind,
                "model": entry
                    .model
                    .clone()
                    .unwrap_or_else(|| crate::settings_cmd::default_model_for_kind(kind).to_string()),
                "base_url": entry
                    .base_url
                    .clone()
                    .unwrap_or_else(|| crate::settings_cmd::default_base_url_for_kind(kind).to_string()),
                "has_key": !entry.api_key.is_empty(),
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
            "{star} {name:<20}  kind={kind:<16}  model={model:<24}  key={key}  {base}",
            star = if row["active"].as_bool() == Some(true) {
                "*"
            } else {
                " "
            },
            name = row["name"].as_str().unwrap_or(""),
            kind = row["kind"].as_str().unwrap_or(""),
            model = row["model"].as_str().unwrap_or(""),
            key = if row["has_key"].as_bool() == Some(true) {
                "yes"
            } else {
                "NO"
            },
            base = row["base_url"].as_str().unwrap_or(""),
        );
    }
    0
}
