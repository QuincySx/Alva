// INPUT:  alva_app_core skill protocol/plugin APIs, alva_test recording mocks, std::{path, sync}, tempfile
// OUTPUT: (test functions, no library exports)
// POS:    Integration tests for skill parsing/loading plus invocation visibility, stable directory injection, and unified runtime invocation.
use std::path::PathBuf;
use std::sync::Arc;

use alva_app_core::extension::skills::skill_domain::{
    skill::{Skill, SkillKind, SkillMeta},
    skill_config::{InjectionPolicy, SkillRef},
};
use alva_app_core::extension::skills::skill_fs::FsSkillRepository;
use alva_app_core::extension::skills::skill_ports::skill_repository::SkillRepository;
use alva_app_core::extension::skills::{
    injector::SkillInjector, loader::SkillLoader, store::SkillStore,
};
use alva_app_core::extension::SkillsPlugin;
use alva_app_core::BaseAgent;
use alva_test::assertions::{collect_events, tool_result_for};
use alva_test::fixtures::{make_assistant_message, tool_use_message};
use alva_test::mock_provider::MockLanguageModel;

/// Test: parse SKILL.md frontmatter
#[test]
fn test_parse_frontmatter() {
    let content = r#"---
name: docx
description: "Parse and generate .docx Word documents"
license: MIT
allowed_tools:
  - execute_shell
  - read_file
metadata:
  version: "1.0.0"
  author: "srow-team"
---

# DOCX Skill

This skill handles Microsoft Word .docx files.

## Usage

Use this skill when you need to read or write Word documents.
"#;

    let meta = FsSkillRepository::parse_frontmatter(content).unwrap();
    assert_eq!(meta.name, "docx");
    assert_eq!(meta.description, "Parse and generate .docx Word documents");
    assert_eq!(meta.license, Some("MIT".to_string()));
    assert_eq!(
        meta.allowed_tools,
        Some(vec!["execute_shell".to_string(), "read_file".to_string()])
    );
    assert_eq!(meta.invocation, Default::default());
    let metadata = meta.metadata.unwrap();
    assert_eq!(
        metadata.get("version").unwrap(),
        &serde_yaml::Value::String("1.0.0".to_string())
    );
}

fn write_bundled_skill(
    primary: &std::path::Path,
    name: &str,
    invocation: &str,
    allowed_tools: Option<&[&str]>,
    body: &str,
) {
    let dir = primary.join("bundled").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let allowed = allowed_tools
        .map(|tools| {
            let lines = tools
                .iter()
                .map(|tool| format!("  - {tool}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!("allowed_tools:\n{lines}\n")
        })
        .unwrap_or_default();
    std::fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: Description for {name}\ninvocation: {invocation}\n{allowed}---\n\n{body}\n"
        ),
    )
    .unwrap();
}

#[tokio::test]
async fn auto_directory_is_always_in_system_prompt_for_chinese_input() {
    let tmp = tempfile::tempdir().unwrap();
    let primary = tmp.path().join("skills");
    write_bundled_skill(
        &primary,
        "auto-catalog-skill",
        "auto",
        None,
        "AUTO_BODY_MUST_NOT_BE_PRELOADED",
    );
    write_bundled_skill(
        &primary,
        "explicit-hidden-skill",
        "explicit",
        None,
        "EXPLICIT_BODY_MUST_NOT_BE_PRELOADED",
    );

    let model = MockLanguageModel::new().with_response(make_assistant_message("done"));
    let recorded = model.clone();
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .plugin(Box::new(SkillsPlugin::with_bundled(primary, None)))
        .build(Arc::new(model))
        .await
        .unwrap();

    let events = collect_events(agent.prompt_text("請幫我整理這份資料，不含任何英文關鍵詞")).await;
    assert!(events
        .iter()
        .any(|event| matches!(event, alva_app_core::AgentEvent::AgentEnd { .. })));

    let request = recorded.calls().into_iter().next().unwrap();
    let system_prompt = request
        .iter()
        .filter(|message| message.role == alva_app_core::alva_kernel_abi::MessageRole::System)
        .map(|message| message.text_content())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        system_prompt.contains("auto-catalog-skill"),
        "{system_prompt}"
    );
    assert!(
        system_prompt.contains("Description for auto-catalog-skill"),
        "{system_prompt}"
    );
    assert!(!system_prompt.contains("AUTO_BODY_MUST_NOT_BE_PRELOADED"));
    assert!(
        !system_prompt.contains("explicit-hidden-skill"),
        "{system_prompt}"
    );
    assert!(!system_prompt.contains("EXPLICIT_BODY_MUST_NOT_BE_PRELOADED"));
    assert!(
        !agent
            .middleware_names()
            .iter()
            .any(|name| name == "skill_injection"),
        "the lexical skill middleware must be retired"
    );
}

#[tokio::test]
async fn unified_skill_tool_loads_explicit_body_and_applies_strict_allowlist() {
    let tmp = tempfile::tempdir().unwrap();
    let primary = tmp.path().join("skills");
    write_bundled_skill(
        &primary,
        "explicit-hidden-skill",
        "explicit",
        Some(&["read_file"]),
        "EXPLICIT_RUNTIME_BODY",
    );

    let model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "skill-call",
            "skill",
            serde_json::json!({"skill": "explicit-hidden-skill", "args": "focus on tests"}),
        ))
        .with_response(make_assistant_message("done"));
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .plugin(Box::new(SkillsPlugin::with_bundled(primary, None)))
        .build(Arc::new(model))
        .await
        .unwrap();

    let events = collect_events(agent.prompt_text("invoke it by exact name")).await;
    let result = tool_result_for(&events, "skill");
    assert!(!result.is_error, "{}", result.model_text());
    assert!(result.model_text().contains("EXPLICIT_RUNTIME_BODY"));
    assert!(result
        .model_text()
        .contains("Strict mode: only use tools: read_file"));
    assert!(result.model_text().contains("focus on tests"));
}

#[tokio::test]
async fn unified_skill_tool_unknown_name_lists_auto_and_explicit_enabled_skills() {
    let tmp = tempfile::tempdir().unwrap();
    let primary = tmp.path().join("skills");
    write_bundled_skill(&primary, "visible-auto", "auto", None, "AUTO_BODY");
    write_bundled_skill(
        &primary,
        "explicit-by-name",
        "explicit",
        None,
        "EXPLICIT_BODY",
    );

    let model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "missing-skill",
            "skill",
            serde_json::json!({"skill": "does-not-exist"}),
        ))
        .with_response(make_assistant_message("done"));
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .plugin(Box::new(SkillsPlugin::with_bundled(primary, None)))
        .build(Arc::new(model))
        .await
        .unwrap();

    let events = collect_events(agent.prompt_text("invoke missing skill")).await;
    let result = tool_result_for(&events, "skill");
    assert!(result.is_error, "unknown skill must be a loud tool error");
    let text = result.model_text();
    assert!(text.contains("Unknown skill 'does-not-exist'"), "{text}");
    assert!(text.contains("visible-auto"), "{text}");
    assert!(text.contains("explicit-by-name"), "{text}");
}

/// Test: parse SKILL.md body (content after frontmatter)
#[test]
fn test_parse_body() {
    let content = r#"---
name: docx
description: "Parse and generate .docx Word documents"
---

# DOCX Skill

This skill handles Microsoft Word .docx files.
"#;

    let body = FsSkillRepository::parse_body(content);
    assert!(body.markdown.starts_with("# DOCX Skill"));
    assert!(body.markdown.contains("Microsoft Word"));
    assert!(body.estimated_tokens > 0);
}

/// Test: parse frontmatter with missing opening delimiter
#[test]
fn test_parse_frontmatter_missing_opening() {
    let content = "name: docx\ndescription: test\n---\n";
    let result = FsSkillRepository::parse_frontmatter(content);
    assert!(result.is_err());
}

/// Test: parse frontmatter with missing closing delimiter
#[test]
fn test_parse_frontmatter_missing_closing() {
    let content = "---\nname: docx\ndescription: test\n";
    let result = FsSkillRepository::parse_frontmatter(content);
    assert!(result.is_err());
}

/// Test: build_meta_summary with multiple skills
#[tokio::test]
async fn test_build_meta_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(FsSkillRepository::new(
        tmp.path().join("bundled"),
        tmp.path().join("mbb"),
        tmp.path().join("user"),
        tmp.path().join("state.json"),
    ));
    let loader = SkillLoader::new(repo);

    let skills = vec![
        Skill {
            meta: SkillMeta {
                name: "docx".to_string(),
                description: "Parse and generate .docx Word documents".to_string(),
                license: None,
                allowed_tools: None,
                invocation: Default::default(),
                metadata: None,
            },
            kind: SkillKind::Bundled,
            root_path: PathBuf::from("/tmp/docx"),
            enabled: true,
        },
        Skill {
            meta: SkillMeta {
                name: "pdf".to_string(),
                description: "Extract text and images from PDF files".to_string(),
                license: None,
                allowed_tools: None,
                invocation: Default::default(),
                metadata: None,
            },
            kind: SkillKind::Bundled,
            root_path: PathBuf::from("/tmp/pdf"),
            enabled: true,
        },
        Skill {
            meta: SkillMeta {
                name: "disabled-skill".to_string(),
                description: "This skill is disabled and should not appear".to_string(),
                license: None,
                allowed_tools: None,
                invocation: Default::default(),
                metadata: None,
            },
            kind: SkillKind::Bundled,
            root_path: PathBuf::from("/tmp/disabled"),
            enabled: false,
        },
    ];

    let summary = loader.build_meta_summary(&skills).await.unwrap();
    assert!(summary.contains("## Available Skills"));
    assert!(summary.contains("**docx**"));
    assert!(summary.contains("**pdf**"));
    // Disabled skill should not appear
    assert!(!summary.contains("disabled-skill"));
}

/// Test: build_meta_summary with empty skills
#[tokio::test]
async fn test_build_meta_summary_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(FsSkillRepository::new(
        tmp.path().join("bundled"),
        tmp.path().join("mbb"),
        tmp.path().join("user"),
        tmp.path().join("state.json"),
    ));
    let loader = SkillLoader::new(repo);

    let summary = loader.build_meta_summary(&[]).await.unwrap();
    assert!(summary.is_empty());
}

/// Test: FsSkillRepository scan with real file system
#[tokio::test]
async fn test_fs_skill_repository_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let bundled_dir = tmp.path().join("bundled");
    let mbb_dir = tmp.path().join("mbb");
    let user_dir = tmp.path().join("user");
    let state_file = tmp.path().join("state.json");

    // Create bundled skill directory
    let docx_dir = bundled_dir.join("docx");
    std::fs::create_dir_all(&docx_dir).unwrap();
    std::fs::write(
        docx_dir.join("SKILL.md"),
        r#"---
name: docx
description: "Parse and generate .docx Word documents"
---

# DOCX Skill

Handle Word documents.
"#,
    )
    .unwrap();

    // Create user skill directory
    let custom_dir = user_dir.join("my-custom");
    std::fs::create_dir_all(&custom_dir).unwrap();
    std::fs::write(
        custom_dir.join("SKILL.md"),
        r#"---
name: my-custom
description: "A custom user skill"
---

# My Custom Skill

Does custom things.
"#,
    )
    .unwrap();

    // Create state file: enable both skills
    std::fs::create_dir_all(tmp.path()).unwrap();
    std::fs::write(&state_file, r#"{"enabled": ["docx", "my-custom"]}"#).unwrap();

    // Create empty MBB dir
    std::fs::create_dir_all(&mbb_dir).unwrap();

    let repo = Arc::new(FsSkillRepository::new(
        &bundled_dir,
        &mbb_dir,
        &user_dir,
        &state_file,
    ));

    let skills = repo.list_skills().await.unwrap();
    assert_eq!(skills.len(), 2);

    let docx = skills.iter().find(|s| s.meta.name == "docx").unwrap();
    assert!(matches!(docx.kind, SkillKind::Bundled));
    assert!(docx.enabled);

    let custom = skills.iter().find(|s| s.meta.name == "my-custom").unwrap();
    assert!(matches!(custom.kind, SkillKind::UserInstalled));
    assert!(custom.enabled);

    // Test load_body
    let body = repo.load_body("docx").await.unwrap();
    assert!(body.markdown.contains("DOCX Skill"));
    assert!(body.markdown.contains("Handle Word documents"));
}

/// Test: SkillStore scan + find
#[tokio::test]
async fn test_skill_store_scan_and_find() {
    let tmp = tempfile::tempdir().unwrap();
    let bundled_dir = tmp.path().join("bundled");
    let docx_dir = bundled_dir.join("docx");
    std::fs::create_dir_all(&docx_dir).unwrap();
    std::fs::write(
        docx_dir.join("SKILL.md"),
        "---\nname: docx\ndescription: \"Word documents\"\n---\n\n# DOCX\n",
    )
    .unwrap();

    let state_file = tmp.path().join("state.json");
    std::fs::write(&state_file, r#"{"enabled": ["docx"]}"#).unwrap();
    std::fs::create_dir_all(tmp.path().join("mbb")).unwrap();
    std::fs::create_dir_all(tmp.path().join("user")).unwrap();

    let repo = Arc::new(FsSkillRepository::new(
        &bundled_dir,
        tmp.path().join("mbb"),
        tmp.path().join("user"),
        &state_file,
    ));

    let store = SkillStore::new(repo);
    store.scan().await.unwrap();

    let all = store.list().await;
    assert_eq!(all.len(), 1);

    let found = store.find_enabled("docx").await;
    assert!(found.is_some());
    assert_eq!(found.unwrap().meta.name, "docx");

    let not_found = store.find_enabled("nonexistent").await;
    assert!(not_found.is_none());
}

/// Test: SkillStore MBB domain routing
#[tokio::test]
async fn test_mbb_domain_routing() {
    let tmp = tempfile::tempdir().unwrap();
    let mbb_dir = tmp.path().join("mbb");

    // Create MBB skill
    let train_dir = mbb_dir.join("12306-train-query");
    std::fs::create_dir_all(&train_dir).unwrap();
    std::fs::write(
        train_dir.join("SKILL.md"),
        "---\nname: 12306-train-query\ndescription: \"Query train tickets on 12306\"\n---\n\n# 12306\n",
    )
    .unwrap();

    // Create manifest.json
    std::fs::write(
        mbb_dir.join("manifest.json"),
        r#"{"skills": [{"id": "12306-train-query", "domains": ["12306.cn"]}]}"#,
    )
    .unwrap();

    let state_file = tmp.path().join("state.json");
    std::fs::write(&state_file, r#"{"enabled": ["12306-train-query"]}"#).unwrap();
    std::fs::create_dir_all(tmp.path().join("bundled")).unwrap();
    std::fs::create_dir_all(tmp.path().join("user")).unwrap();

    let repo = Arc::new(FsSkillRepository::new(
        tmp.path().join("bundled"),
        &mbb_dir,
        tmp.path().join("user"),
        &state_file,
    ));

    let store = SkillStore::new(repo);
    store.scan().await.unwrap();

    // Exact domain match
    let found = store.find_mbb_by_domain("12306.cn").await;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].meta.name, "12306-train-query");

    // Suffix match (www.12306.cn ends with 12306.cn)
    let found = store.find_mbb_by_domain("www.12306.cn").await;
    assert_eq!(found.len(), 1);

    // No match
    let found = store.find_mbb_by_domain("example.com").await;
    assert!(found.is_empty());
}

/// Test: SkillInjector with mixed injection policies
#[tokio::test]
async fn test_skill_injector_mixed_policies() {
    let tmp = tempfile::tempdir().unwrap();
    let bundled_dir = tmp.path().join("bundled");

    // Create two skills
    let docx_dir = bundled_dir.join("docx");
    std::fs::create_dir_all(&docx_dir).unwrap();
    std::fs::write(
        docx_dir.join("SKILL.md"),
        "---\nname: docx\ndescription: \"Word documents\"\n---\n\n# DOCX Skill Instructions\n\nFull instructions here.\n",
    )
    .unwrap();

    let pdf_dir = bundled_dir.join("pdf");
    std::fs::create_dir_all(&pdf_dir).unwrap();
    std::fs::write(
        pdf_dir.join("SKILL.md"),
        "---\nname: pdf\ndescription: \"PDF files\"\n---\n\n# PDF Skill Instructions\n\nPDF handling here.\n",
    )
    .unwrap();

    let state_file = tmp.path().join("state.json");
    std::fs::write(&state_file, r#"{"enabled": ["docx", "pdf"]}"#).unwrap();
    std::fs::create_dir_all(tmp.path().join("mbb")).unwrap();
    std::fs::create_dir_all(tmp.path().join("user")).unwrap();

    let repo = Arc::new(FsSkillRepository::new(
        &bundled_dir,
        tmp.path().join("mbb"),
        tmp.path().join("user"),
        &state_file,
    ));

    let loader = SkillLoader::new(repo.clone());
    let injector = SkillInjector::new(loader);

    let skills = repo.list_skills().await.unwrap();

    // docx = Auto (only metadata in summary), pdf = Explicit (full body)
    let refs = vec![
        SkillRef {
            name: "docx".to_string(),
            injection: Some(InjectionPolicy::Auto),
        },
        SkillRef {
            name: "pdf".to_string(),
            injection: Some(InjectionPolicy::Explicit),
        },
    ];

    let injection = injector.build_injection(&refs, &skills).await.unwrap();

    // Auto mode: docx should appear in summary table
    assert!(injection.contains("**docx**"));
    assert!(injection.contains("Available Skills"));

    // Explicit mode: pdf should have full body
    assert!(injection.contains("## Skill: pdf"));
    assert!(injection.contains("PDF Skill Instructions"));
    assert!(injection.contains("PDF handling here"));
}

/// End-to-end test: load mock SKILL.md, parse frontmatter, generate system prompt fragment
#[tokio::test]
async fn test_end_to_end_skill_loading_and_injection() {
    let tmp = tempfile::tempdir().unwrap();
    let bundled_dir = tmp.path().join("bundled");

    // Create a mock skill
    let skill_dir = bundled_dir.join("browser-helper");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: browser-helper
description: "Assist with browser automation tasks including navigation, form filling, and data extraction"
allowed_tools:
  - browser_navigate
  - browser_click
  - browser_type
---

# Browser Helper

You are a browser automation assistant. When the user needs to interact with web pages:

1. Navigate to the target URL
2. Wait for the page to load
3. Interact with elements as needed
4. Extract required data

## Rules

- Always wait for page load before interacting
- Use CSS selectors for element targeting
- Handle errors gracefully
"#,
    )
    .unwrap();

    // Create references directory with a resource
    let refs_dir = skill_dir.join("references");
    std::fs::create_dir_all(&refs_dir).unwrap();
    std::fs::write(
        refs_dir.join("selectors.md"),
        "# Common Selectors\n\n- Login button: `#login-btn`\n",
    )
    .unwrap();

    let state_file = tmp.path().join("state.json");
    std::fs::write(&state_file, r#"{"enabled": ["browser-helper"]}"#).unwrap();
    std::fs::create_dir_all(tmp.path().join("mbb")).unwrap();
    std::fs::create_dir_all(tmp.path().join("user")).unwrap();

    let repo = Arc::new(FsSkillRepository::new(
        &bundled_dir,
        tmp.path().join("mbb"),
        tmp.path().join("user"),
        &state_file,
    ));

    // 1. Parse frontmatter
    let skills = repo.list_skills().await.unwrap();
    assert_eq!(skills.len(), 1);
    let skill = &skills[0];
    assert_eq!(skill.meta.name, "browser-helper");
    assert!(skill.meta.description.contains("browser automation"));
    assert_eq!(
        skill.meta.allowed_tools,
        Some(vec![
            "browser_navigate".to_string(),
            "browser_click".to_string(),
            "browser_type".to_string(),
        ])
    );

    // 2. Load body (Level 2)
    let body = repo.load_body("browser-helper").await.unwrap();
    assert!(body.markdown.contains("Browser Helper"));
    assert!(body.markdown.contains("CSS selectors"));
    assert!(body.estimated_tokens > 0);

    // 3. List resources (Level 3)
    let resources = repo.list_resources("browser-helper").await.unwrap();
    assert_eq!(resources.len(), 1);
    assert!(resources[0].contains("selectors.md"));

    // 4. Load resource
    let resource = repo
        .load_resource("browser-helper", &resources[0])
        .await
        .unwrap();
    let content = String::from_utf8(resource.content).unwrap();
    assert!(content.contains("Common Selectors"));

    // 5. Generate system prompt injection (Explicit mode)
    let loader = SkillLoader::new(repo.clone());
    let injector = SkillInjector::new(loader);

    let refs = vec![SkillRef {
        name: "browser-helper".to_string(),
        injection: Some(InjectionPolicy::Explicit),
    }];

    let injection = injector.build_injection(&refs, &skills).await.unwrap();
    assert!(injection.contains("## Skill: browser-helper"));
    assert!(injection.contains("Browser Helper"));
    assert!(injection.contains("CSS selectors"));

    // 6. Test Strict mode with tool constraints
    let refs_strict = vec![SkillRef {
        name: "browser-helper".to_string(),
        injection: Some(InjectionPolicy::Strict),
    }];

    let injection_strict = injector
        .build_injection(&refs_strict, &skills)
        .await
        .unwrap();
    assert!(injection_strict.contains("Strict mode: only use tools"));
    assert!(injection_strict.contains("browser_navigate"));
}
