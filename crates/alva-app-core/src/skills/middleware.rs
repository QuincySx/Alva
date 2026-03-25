// INPUT:  alva_agent_core::middleware, alva_types::Message, crate::skills::{store, injector, skill_domain}
// OUTPUT: SkillInjectionMiddleware
// POS:    Middleware that dynamically injects relevant skills into the LLM context based on conversation content.

//! Skill injection middleware — dynamically loads skill content into the
//! system prompt based on conversation context.
//!
//! Instead of statically injecting all skills at agent creation time, this
//! middleware analyzes the most recent user message before each LLM call
//! and injects only the skills relevant to the current intent.
//!
//! This avoids context window explosion while ensuring the agent always
//! has access to the most relevant skills.

use std::sync::Arc;

use alva_agent_core::middleware::{Middleware, MiddlewareContext, MiddlewareError};
use alva_types::{Message, MessageRole};
use async_trait::async_trait;

use crate::skills::injector::SkillInjector;
use crate::skills::skill_domain::skill::Skill;
use crate::skills::skill_domain::skill_config::{InjectionPolicy, SkillRef};
use crate::skills::store::SkillStore;

/// Configuration for the skill injection middleware.
pub struct SkillInjectionConfig {
    /// Maximum number of skills to inject per LLM call.
    pub max_skills: usize,
    /// Default injection policy for dynamically discovered skills.
    pub default_policy: InjectionPolicy,
    /// Skills that are always injected (regardless of intent detection).
    pub always_inject: Vec<SkillRef>,
    /// Minimum query length to trigger search (avoids noise from short messages).
    pub min_query_len: usize,
}

impl Default for SkillInjectionConfig {
    fn default() -> Self {
        Self {
            max_skills: 5,
            default_policy: InjectionPolicy::Auto,
            always_inject: Vec::new(),
            min_query_len: 3,
        }
    }
}

/// Tracks which skills have already been injected to avoid re-injection.
struct InjectedSkills {
    names: std::collections::HashSet<String>,
}

/// Middleware that dynamically injects relevant skills into the LLM context.
///
/// On each `before_llm_call`, it:
/// 1. Extracts keywords from the most recent user message
/// 2. Searches the `SkillStore` for matching skills
/// 3. Uses `SkillInjector` to build injection blocks
/// 4. Inserts a system message with the skill content into the message list
///
/// Previously injected skills are tracked in `Extensions` to avoid duplicate injection.
///
/// # Example
///
/// ```rust,ignore
/// use alva_app_core::{SkillInjectionMiddleware, SkillInjectionConfig, SkillStore, SkillInjector};
///
/// let middleware = SkillInjectionMiddleware::new(
///     skill_store,
///     skill_injector,
///     SkillInjectionConfig::default(),
/// );
///
/// let mut hooks = AgentHooks::new(convert_fn);
/// hooks.middleware.push(Arc::new(middleware));
/// ```
pub struct SkillInjectionMiddleware {
    store: Arc<SkillStore>,
    injector: Arc<SkillInjector>,
    config: SkillInjectionConfig,
}

impl SkillInjectionMiddleware {
    pub fn new(
        store: Arc<SkillStore>,
        injector: Arc<SkillInjector>,
        config: SkillInjectionConfig,
    ) -> Self {
        Self {
            store,
            injector,
            config,
        }
    }

    pub fn with_defaults(store: Arc<SkillStore>, injector: Arc<SkillInjector>) -> Self {
        Self::new(store, injector, SkillInjectionConfig::default())
    }

    /// Extract the most recent user message text for intent detection.
    fn extract_latest_user_text(messages: &[Message]) -> Option<String> {
        messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::User)
            .map(|m| {
                m.content
                    .iter()
                    .filter_map(|b| b.as_text())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
    }

    /// Extract simple keywords from user text for skill search.
    ///
    /// This is intentionally simple — split on whitespace and filter short tokens.
    /// For more sophisticated intent detection, override this or use a separate NLP step.
    fn extract_keywords(text: &str) -> Vec<String> {
        text.split_whitespace()
            .filter(|w| w.len() >= 3)
            .map(|w| w.to_lowercase())
            // Remove common stop words
            .filter(|w| {
                !matches!(
                    w.as_str(),
                    "the" | "and" | "for" | "are" | "but" | "not"
                    | "you" | "all" | "can" | "had" | "her"
                    | "was" | "one" | "our" | "out" | "has"
                    | "this" | "that" | "with" | "have" | "from"
                    | "they" | "been" | "said" | "each" | "which"
                    | "will" | "what" | "there" | "their" | "about"
                    | "would" | "make" | "like" | "just" | "please"
                    | "could" | "help" | "want"
                )
            })
            .take(5) // Limit keywords to avoid over-searching
            .collect()
    }
}

#[async_trait]
impl Middleware for SkillInjectionMiddleware {
    async fn on_agent_start(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        // Initialize the injected-skills tracker in Extensions
        ctx.extensions.insert(InjectedSkills {
            names: std::collections::HashSet::new(),
        });
        Ok(())
    }

    async fn before_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        // 1. Extract latest user message for intent detection
        let user_text = match Self::extract_latest_user_text(messages) {
            Some(text) if text.len() >= self.config.min_query_len => text,
            _ => {
                // No user message or too short — only inject always_inject skills
                if self.config.always_inject.is_empty() {
                    return Ok(());
                }
                String::new()
            }
        };

        // 2. Get already-injected skills to avoid duplicates
        let already_injected = ctx
            .extensions
            .get::<InjectedSkills>()
            .map(|s| s.names.clone())
            .unwrap_or_default();

        // 3. Search for relevant skills based on keywords
        let mut skill_refs: Vec<SkillRef> = Vec::new();
        let mut matched_skills: Vec<Skill> = Vec::new();

        // Always-inject skills first
        for always_ref in &self.config.always_inject {
            if !already_injected.contains(&always_ref.name) {
                if let Some(skill) = self.store.find_enabled(&always_ref.name).await {
                    skill_refs.push(always_ref.clone());
                    matched_skills.push(skill);
                }
            }
        }

        // Keyword-based search
        if !user_text.is_empty() {
            let keywords = Self::extract_keywords(&user_text);
            let mut seen = std::collections::HashSet::new();

            for keyword in &keywords {
                let results = self.store.search(keyword).await;
                for skill in results {
                    if !already_injected.contains(&skill.meta.name)
                        && !seen.contains(&skill.meta.name)
                        && matched_skills.len() < self.config.max_skills
                    {
                        seen.insert(skill.meta.name.clone());
                        skill_refs.push(SkillRef {
                            name: skill.meta.name.clone(),
                            injection: Some(self.config.default_policy.clone()),
                        });
                        matched_skills.push(skill);
                    }
                }
            }
        }

        if skill_refs.is_empty() {
            return Ok(());
        }

        // 4. Build injection block via SkillInjector
        let injection = self
            .injector
            .build_injection(&skill_refs, &matched_skills)
            .await
            .map_err(|e| MiddlewareError::Other(format!("skill injection failed: {e}")))?;

        if injection.is_empty() {
            return Ok(());
        }

        // 5. Insert as a system message after the first system prompt
        let insert_pos = messages
            .iter()
            .position(|m| m.role != MessageRole::System)
            .unwrap_or(messages.len());

        messages.insert(insert_pos, Message::system(&injection));

        // 6. Track injected skills in Extensions
        if let Some(tracker) = ctx.extensions.get_mut::<InjectedSkills>() {
            for sr in &skill_refs {
                tracker.names.insert(sr.name.clone());
            }
        }

        tracing::debug!(
            skills = ?skill_refs.iter().map(|s| &s.name).collect::<Vec<_>>(),
            "injected skills into context"
        );

        Ok(())
    }

    fn name(&self) -> &str {
        "skill_injection"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::ContentBlock;
    use crate::skills::loader::SkillLoader;
    use crate::skills::skill_domain::skill::{SkillKind, SkillMeta};
    use crate::skills::skill_fs::FsSkillRepository;
    use alva_agent_core::middleware::Extensions;

    #[test]
    fn extract_keywords_filters_stop_words() {
        let keywords = SkillInjectionMiddleware::extract_keywords(
            "please help me convert this PDF file to markdown",
        );
        // Should keep: "convert", "pdf", "file", "markdown" (not "please", "help", "this")
        assert!(keywords.contains(&"convert".to_string()));
        assert!(keywords.contains(&"pdf".to_string()));
        assert!(keywords.contains(&"markdown".to_string()));
        assert!(!keywords.contains(&"please".to_string()));
        assert!(!keywords.contains(&"help".to_string()));
        assert!(!keywords.contains(&"this".to_string()));
    }

    #[test]
    fn extract_keywords_limits_to_5() {
        let keywords = SkillInjectionMiddleware::extract_keywords(
            "alpha bravo charlie delta echo foxtrot golf hotel india juliet",
        );
        assert!(keywords.len() <= 5);
    }

    #[test]
    fn extract_latest_user_text_finds_last_user_message() {
        let messages = vec![
            Message::system("system prompt"),
            Message::user("first question"),
            Message {
                id: "1".into(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: "answer".into(),
                }],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            },
            Message::user("second question about PDF conversion"),
        ];
        let text = SkillInjectionMiddleware::extract_latest_user_text(&messages);
        assert_eq!(text, Some("second question about PDF conversion".to_string()));
    }

    #[test]
    fn extract_latest_user_text_returns_none_when_no_user() {
        let messages = vec![Message::system("system prompt")];
        let text = SkillInjectionMiddleware::extract_latest_user_text(&messages);
        assert!(text.is_none());
    }

    #[test]
    fn default_config_values() {
        let config = SkillInjectionConfig::default();
        assert_eq!(config.max_skills, 5);
        assert_eq!(config.min_query_len, 3);
        assert!(config.always_inject.is_empty());
    }
}
