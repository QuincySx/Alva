// INPUT:  std::sync, alva_kernel_abi, crate::extension::skills::{loader, store}, async_trait, serde, serde_json
// OUTPUT: SearchSkillsTool, UseSkillTool
// POS:    Agent-facing meta-tools for discovering and activating Skills at runtime.
//! Skill meta-tools: search_skills and use_skill
//!
//! These tools allow the Agent to discover and activate Skills at runtime.

use std::sync::Arc;

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use crate::extension::skills::loader::SkillLoader;
use crate::extension::skills::store::SkillStore;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// search_skills
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SearchSkillsInput {
    query: String,
}

/// Search available Skills by keyword (name + description substring match).
pub struct SearchSkillsTool {
    pub store: Arc<SkillStore>,
}

#[async_trait]
impl Tool for SearchSkillsTool {
    fn name(&self) -> &str {
        "search_skills"
    }

    fn description(&self) -> &str {
        "Search available skills by keyword. Returns a list of matching skill names and descriptions."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search keyword (matched against skill name and description)"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: SearchSkillsInput =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "search_skills".into(), message: e.to_string() })?;

        let results = self.store.search(&params.query).await;

        let output: Vec<Value> = results
            .iter()
            .map(|s| {
                json!({
                    "id": s.meta.name,
                    "description": s.meta.description,
                    "kind": s.kind,
                    "enabled": s.enabled,
                })
            })
            .collect();

        Ok(ToolOutput::text(serde_json::to_string_pretty(&output)
            .unwrap_or_else(|_| "[]".to_string())))
    }
}

// ---------------------------------------------------------------------------
// use_skill
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UseSkillInput {
    skill_name: String,
    /// "preview" returns SKILL.md body; "full" returns body + resource list
    #[serde(default = "default_level")]
    level: String,
}

fn default_level() -> String {
    "preview".to_string()
}

/// Activate a Skill by name, returning its SKILL.md content and optionally resource list.
pub struct UseSkillTool {
    pub store: Arc<SkillStore>,
    pub loader: Arc<SkillLoader>,
}

#[async_trait]
impl Tool for UseSkillTool {
    fn name(&self) -> &str {
        "use_skill"
    }

    fn description(&self) -> &str {
        "Activate a skill by name. Returns the skill's instruction content (SKILL.md body). Use level='full' to also list resource files."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["skill_name"],
            "properties": {
                "skill_name": {
                    "type": "string",
                    "description": "The skill name (id) to activate"
                },
                "level": {
                    "type": "string",
                    "enum": ["preview", "full"],
                    "description": "Level of detail: 'preview' returns SKILL.md body, 'full' also includes resource file list. Default: 'preview'"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: UseSkillInput =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "use_skill".into(), message: e.to_string() })?;

        // Verify skill exists and is enabled
        let skill = self
            .store
            .find_enabled(&params.skill_name)
            .await
            .ok_or_else(|| {
                AgentError::ToolError {
                    tool_name: "use_skill".into(),
                    message: format!("Skill '{}' not found or not enabled", params.skill_name),
                }
            })?;

        // Load SKILL.md body (Level 2)
        let body = self
            .loader
            .load_skill_body(&params.skill_name)
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "use_skill".into(), message: e.to_string() })?;

        let mut output = json!({
            "skill_name": skill.meta.name,
            "description": skill.meta.description,
            "body": body.markdown,
            "estimated_tokens": body.estimated_tokens,
        });

        // Level "full": also include resource file list
        if params.level == "full" {
            let resources = self
                .loader
                .list_resources(&params.skill_name)
                .await
                .unwrap_or_default();
            output["resources"] = json!(resources);
        }

        Ok(ToolOutput::text(serde_json::to_string_pretty(&output)
            .unwrap_or_else(|_| "{}".to_string())))
    }
}
