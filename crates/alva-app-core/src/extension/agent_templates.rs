// INPUT:  serde, toml, std::fs, std::path, crate::extension::skills::skill_domain::agent_template::AgentTemplate
// OUTPUT: builtin_agent_templates, load_agent_templates_file, resolve_agent_templates
// POS:    Source of named sub-agent templates: a built-in set (always present)
//         overlaid by user/project `agents.toml` files. Fed into SubAgentPlugin
//         so the `agent` tool can spawn them by `agent_type` (kimi-code's
//         subagent_type model).
//! agent_templates — built-in + config-file sub-agent templates

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::extension::skills::skill_domain::agent_template::AgentTemplate;

/// Built-in templates that are always available for spawning.
pub fn builtin_agent_templates() -> Vec<AgentTemplate> {
    vec![video_template()]
}

/// The `video` sub-agent: a vision-capable worker that uses the
/// `understand_video` tool to "watch" a local video and report back text.
fn video_template() -> AgentTemplate {
    AgentTemplate {
        id: "video".into(),
        name: "video".into(),
        description: "Watch and understand a local video file. Use when the task needs the \
            contents of a video described, summarized, or queried — this agent can 'see' video \
            via the understand_video tool (the parent model usually cannot)."
            .into(),
        system_prompt_base: "You are a video-understanding sub-agent. The parent agent has given \
            you a task about a video. Use the understand_video tool on the video path to obtain a \
            textual understanding, then return a clear, complete answer to the parent. You cannot \
            perceive video yourself except through that tool, so always call it before answering."
            .into(),
        skills: Default::default(),
        mcp_servers: Default::default(),
        allowed_tools: Some(vec!["understand_video".into()]),
        max_iterations: None,
        model: None,
    }
}

/// On-disk `agents.toml` shape: an array of `[[agent]]` tables, each a
/// (mostly) minimal [`AgentTemplate`] (non-essential fields default).
#[derive(Debug, Deserialize, Default)]
struct AgentTemplatesFile {
    #[serde(default)]
    agent: Vec<AgentTemplate>,
}

/// Load templates from a single TOML file. A missing file yields an empty
/// list; a malformed file logs a warning and yields empty (never fatal).
pub fn load_agent_templates_file(path: &Path) -> Vec<AgentTemplate> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match toml::from_str::<AgentTemplatesFile>(&content) {
        Ok(file) => file
            .agent
            .into_iter()
            .map(|mut t| {
                if t.id.is_empty() {
                    t.id = t.name.clone();
                }
                t
            })
            .collect(),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "failed to parse agents.toml");
            Vec::new()
        }
    }
}

/// Resolve the effective template set: built-ins first, then each config
/// file overlaid in order. A file template with the same `name` as an
/// existing one replaces it (later files win, config beats built-ins).
pub fn resolve_agent_templates(files: &[PathBuf]) -> Vec<AgentTemplate> {
    let mut out = builtin_agent_templates();
    for path in files {
        for t in load_agent_templates_file(path) {
            if let Some(existing) = out.iter_mut().find(|e| e.name == t.name) {
                *existing = t;
            } else {
                out.push(t);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_include_video_with_understand_video_tool() {
        let v = builtin_agent_templates();
        let video = v.iter().find(|t| t.name == "video").expect("video builtin");
        assert_eq!(
            video.allowed_tools.as_deref(),
            Some(["understand_video".to_string()].as_slice())
        );
        assert!(!video.description.is_empty());
        assert!(video.system_prompt_base.contains("understand_video"));
    }

    #[test]
    fn missing_file_yields_empty() {
        assert!(load_agent_templates_file(Path::new("/no/such/agents.toml")).is_empty());
    }

    #[test]
    fn loads_minimal_template_from_toml() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("agents.toml");
        std::fs::write(
            &path,
            r#"
[[agent]]
name = "reviewer"
description = "Use for code review."
system_prompt_base = "You review code."
allowed_tools = ["read_file", "grep_search"]
"#,
        )
        .unwrap();
        let got = load_agent_templates_file(&path);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "reviewer");
        assert_eq!(got[0].id, "reviewer", "id defaults to name");
        assert_eq!(got[0].allowed_tools.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn config_overrides_builtin_by_name() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("agents.toml");
        std::fs::write(
            &path,
            r#"
[[agent]]
name = "video"
description = "Custom video agent."
system_prompt_base = "custom"

[[agent]]
name = "planner"
description = "Plans work."
system_prompt_base = "You plan."
"#,
        )
        .unwrap();
        let got = resolve_agent_templates(&[path]);
        // built-in video replaced by the config one; planner added.
        let video = got.iter().find(|t| t.name == "video").unwrap();
        assert_eq!(video.description, "Custom video agent.");
        assert!(got.iter().any(|t| t.name == "planner"));
    }
}
