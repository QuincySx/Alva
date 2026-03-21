// INPUT:  std::sync, crate::domain::tool, crate::error, crate::ports::tool, crate::skills::{loader, store}, async_trait, serde, serde_json
// OUTPUT: SearchSkillsTool, UseSkillTool
// POS:    Agent-facing meta-tools for discovering and activating Skills at runtime.
//! Skill meta-tools: search_skills and use_skill
//!
//! These tools allow the Agent to discover and activate Skills at runtime.

use std::sync::Arc;

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};
use crate::skills::loader::SkillLoader;
use crate::skills::store::SkillStore;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

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

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "search_skills".to_string(),
            description: "Search available skills by keyword. Returns a list of matching skill names and descriptions.".to_string(),
            parameters: json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search keyword (matched against skill name and description)"
                    }
                }
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: SearchSkillsInput =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();
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

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            tool_name: "search_skills".to_string(),
            output: serde_json::to_string_pretty(&output)
                .unwrap_or_else(|_| "[]".to_string()),
            is_error: false,
            duration_ms,
        })
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

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "use_skill".to_string(),
            description: "Activate a skill by name. Returns the skill's instruction content (SKILL.md body). Use level='full' to also list resource files.".to_string(),
            parameters: json!({
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
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: UseSkillInput =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();

        // Verify skill exists and is enabled
        let skill = self
            .store
            .find_enabled(&params.skill_name)
            .await
            .ok_or_else(|| {
                EngineError::ToolExecution(format!(
                    "Skill '{}' not found or not enabled",
                    params.skill_name
                ))
            })?;

        // Load SKILL.md body (Level 2)
        let body = self
            .loader
            .load_skill_body(&params.skill_name)
            .await
            .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

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

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            tool_name: "use_skill".to_string(),
            output: serde_json::to_string_pretty(&output)
                .unwrap_or_else(|_| "{}".to_string()),
            is_error: false,
            duration_ms,
        })
    }
}
