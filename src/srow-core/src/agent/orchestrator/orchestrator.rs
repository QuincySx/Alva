//! Orchestrator — the central coordinator for the multi-Agent system.
//!
//! Manages three core Agents (brain, reviewer, explorer) and a pool of
//! execution Agent instances created dynamically from templates.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use tokio::sync::RwLock;

use crate::domain::agent::LLMConfig;
use crate::error::EngineError;

use super::communication::{AgentMessage, MessageBus};
use super::instance::{AgentInstance, AgentInstanceStatus};
use super::template::{predefined_templates, OrchestratorAgentTemplate};

/// The Orchestrator owns the core Agents, execution Agent pool, template library,
/// message bus, and shared workspace directory.
pub struct Orchestrator {
    /// Decision Agent — analyzes tasks, selects templates, dispatches work
    pub brain: RwLock<AgentInstance>,
    /// Quality gate Agent — reviews results, judges pass/fail
    pub reviewer: RwLock<AgentInstance>,
    /// Brainstorming Agent — explores alternative approaches when reviewer rejects
    pub explorer: RwLock<AgentInstance>,

    /// Execution Agent instance pool (instance_id -> AgentInstance)
    instances: RwLock<HashMap<String, AgentInstance>>,

    /// Template library (template_id -> OrchestratorAgentTemplate)
    templates: RwLock<HashMap<String, OrchestratorAgentTemplate>>,

    /// Inter-Agent message bus
    message_bus: Arc<MessageBus>,

    /// Shared workspace directory (all Agents can read/write here)
    pub workspace_dir: PathBuf,
}

impl Orchestrator {
    /// Create a new Orchestrator with predefined templates and core Agents.
    ///
    /// `default_llm` is used for all predefined templates and core Agents.
    /// `workspace_dir` is the shared directory accessible to all Agents.
    pub fn new(default_llm: &LLMConfig, workspace_dir: PathBuf) -> Self {
        // Initialize core Agents
        let brain = AgentInstance::new("brain", "Orchestration decision-making");
        let reviewer = AgentInstance::new("reviewer", "Result quality review");
        let explorer = AgentInstance::new("explorer", "Alternative approach exploration");

        // Load predefined templates
        let mut templates_map = HashMap::new();
        for t in predefined_templates(default_llm) {
            templates_map.insert(t.id.clone(), t);
        }

        let message_bus = Arc::new(MessageBus::new());

        Self {
            brain: RwLock::new(brain),
            reviewer: RwLock::new(reviewer),
            explorer: RwLock::new(explorer),
            instances: RwLock::new(HashMap::new()),
            templates: RwLock::new(templates_map),
            message_bus,
            workspace_dir,
        }
    }

    /// Register a custom Agent template at runtime.
    pub async fn register_template(&self, template: OrchestratorAgentTemplate) {
        let mut templates = self.templates.write().await;
        templates.insert(template.id.clone(), template);
    }

    /// Get a template by ID.
    pub async fn get_template(&self, template_id: &str) -> Option<OrchestratorAgentTemplate> {
        let templates = self.templates.read().await;
        templates.get(template_id).cloned()
    }

    /// List all available templates.
    pub async fn list_templates(&self) -> Vec<OrchestratorAgentTemplate> {
        let templates = self.templates.read().await;
        templates.values().cloned().collect()
    }

    /// Create a new execution Agent instance from a template.
    ///
    /// Returns the instance ID. The instance starts in Idle status.
    pub async fn create_agent(
        &self,
        template_id: &str,
        task: &str,
    ) -> Result<String, EngineError> {
        // Verify template exists
        let templates = self.templates.read().await;
        if !templates.contains_key(template_id) {
            return Err(EngineError::ToolExecution(format!(
                "Template '{}' not found. Available: {:?}",
                template_id,
                templates.keys().collect::<Vec<_>>()
            )));
        }
        drop(templates);

        let instance = AgentInstance::new(template_id, task);
        let instance_id = instance.id.clone();

        // Register mailbox
        self.message_bus.register(&instance_id).await;

        // Add to pool
        let mut instances = self.instances.write().await;
        instances.insert(instance_id.clone(), instance);

        tracing::info!(
            template_id = template_id,
            instance_id = %instance_id,
            task = task,
            "Created agent instance"
        );

        Ok(instance_id)
    }

    /// Get the current status of an Agent instance.
    pub async fn get_agent_status(&self, agent_id: &str) -> Result<AgentInstance, EngineError> {
        let instances = self.instances.read().await;
        instances
            .get(agent_id)
            .cloned()
            .ok_or_else(|| EngineError::ToolExecution(format!("Agent '{}' not found", agent_id)))
    }

    /// List all execution Agent instances.
    pub async fn list_agents(&self) -> Vec<AgentInstance> {
        let instances = self.instances.read().await;
        instances.values().cloned().collect()
    }

    /// Send a message from the orchestrator (brain) to an Agent instance.
    pub async fn send_to_agent(&self, agent_id: &str, content: &str) -> Result<(), EngineError> {
        // Verify agent exists
        {
            let instances = self.instances.read().await;
            if !instances.contains_key(agent_id) {
                return Err(EngineError::ToolExecution(format!(
                    "Agent '{}' not found",
                    agent_id
                )));
            }
        }

        let brain_id = self.brain.read().await.id.clone();
        let msg = AgentMessage::new(&brain_id, agent_id, content);
        self.message_bus.send(msg).await;

        tracing::info!(agent_id = agent_id, "Sent message to agent");
        Ok(())
    }

    /// Cancel a running Agent instance.
    pub async fn cancel_agent(&self, agent_id: &str) -> Result<(), EngineError> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(agent_id)
            .ok_or_else(|| EngineError::ToolExecution(format!("Agent '{}' not found", agent_id)))?;

        if instance.is_terminal() {
            return Err(EngineError::ToolExecution(format!(
                "Agent '{}' is already in terminal state: {:?}",
                agent_id, instance.status
            )));
        }

        instance.cancel();
        tracing::info!(agent_id = agent_id, "Cancelled agent");
        Ok(())
    }

    /// Update an Agent instance's status to Running.
    pub async fn start_agent(&self, agent_id: &str) -> Result<(), EngineError> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(agent_id)
            .ok_or_else(|| EngineError::ToolExecution(format!("Agent '{}' not found", agent_id)))?;
        instance.start();
        Ok(())
    }

    /// Complete an Agent instance with a result.
    pub async fn complete_agent(&self, agent_id: &str, result: String) -> Result<(), EngineError> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(agent_id)
            .ok_or_else(|| EngineError::ToolExecution(format!("Agent '{}' not found", agent_id)))?;
        instance.complete(result);
        tracing::info!(agent_id = agent_id, "Agent completed");
        Ok(())
    }

    /// Mark an Agent instance as failed.
    pub async fn fail_agent(&self, agent_id: &str, error: String) -> Result<(), EngineError> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(agent_id)
            .ok_or_else(|| EngineError::ToolExecution(format!("Agent '{}' not found", agent_id)))?;
        instance.fail(error);
        tracing::info!(agent_id = agent_id, "Agent failed");
        Ok(())
    }

    /// Submit an Agent's result to the reviewer for quality assessment.
    ///
    /// Currently returns a structured review placeholder. In full implementation,
    /// this will run the reviewer Agent's engine loop with the result as input.
    pub async fn review_result(&self, agent_id: &str) -> Result<String, EngineError> {
        let instances = self.instances.read().await;
        let instance = instances
            .get(agent_id)
            .ok_or_else(|| EngineError::ToolExecution(format!("Agent '{}' not found", agent_id)))?;

        if instance.status != AgentInstanceStatus::Completed {
            return Err(EngineError::ToolExecution(format!(
                "Agent '{}' is not completed (status: {:?}), cannot review",
                agent_id, instance.status
            )));
        }

        let result = instance
            .result
            .as_deref()
            .unwrap_or("(no result)");

        // In the full implementation, this will:
        // 1. Feed the result to the reviewer Agent's engine
        // 2. The reviewer Agent analyzes quality
        // 3. Return pass/fail with feedback
        //
        // For now, return a structured placeholder that the brain can work with.
        let review = json!({
            "agent_id": agent_id,
            "task": instance.task,
            "result_preview": if result.len() > 500 {
                format!("{}...", &result[..500])
            } else {
                result.to_string()
            },
            "review_status": "pending_reviewer_implementation",
            "note": "Reviewer Agent engine integration pending. The brain should assess the result directly for now."
        });

        Ok(review.to_string())
    }

    /// Read messages from an Agent's mailbox.
    pub async fn read_messages(&self, agent_id: &str) -> Vec<AgentMessage> {
        self.message_bus.read(agent_id).await
    }

    /// Drain (consume) messages from an Agent's mailbox.
    pub async fn drain_messages(&self, agent_id: &str) -> Vec<AgentMessage> {
        self.message_bus.drain(agent_id).await
    }

    /// Remove a terminated Agent instance from the pool.
    pub async fn remove_agent(&self, agent_id: &str) -> Result<(), EngineError> {
        let mut instances = self.instances.write().await;
        let instance = instances
            .get(agent_id)
            .ok_or_else(|| EngineError::ToolExecution(format!("Agent '{}' not found", agent_id)))?;

        if !instance.is_terminal() {
            return Err(EngineError::ToolExecution(format!(
                "Agent '{}' is not in terminal state, cannot remove",
                agent_id
            )));
        }

        instances.remove(agent_id);
        self.message_bus.unregister(agent_id).await;
        Ok(())
    }

    /// Get a reference to the message bus (for tools/external use).
    pub fn message_bus(&self) -> &Arc<MessageBus> {
        &self.message_bus
    }

    /// Build the system prompt for the brain Agent, informing it of available templates.
    pub async fn brain_system_prompt(&self) -> String {
        let templates = self.templates.read().await;
        let mut template_list = String::new();
        for t in templates.values() {
            template_list.push_str(&format!(
                "- **{}** ({}): {}\n",
                t.id, t.name, t.description
            ));
        }

        format!(
            concat!(
                "You are the Brain Agent — the central decision-maker of the Srow Agent system.\n",
                "\n",
                "Your role:\n",
                "1. Analyze the user's task\n",
                "2. Break it into subtasks if needed\n",
                "3. Select the right Agent template(s) for each subtask\n",
                "4. Create Agent instances and assign tasks\n",
                "5. Monitor execution progress\n",
                "6. Submit completed results for review\n",
                "7. If review fails, adjust strategy and retry\n",
                "\n",
                "## Available Agent Templates\n",
                "\n",
                "{}\n",
                "\n",
                "## Available Tools\n",
                "\n",
                "- `list_templates` — List all available Agent templates\n",
                "- `create_agent(template_id, task)` — Create a new Agent from a template\n",
                "- `send_to_agent(agent_id, message)` — Send a message to an Agent\n",
                "- `get_agent_status(agent_id)` — Check an Agent's status\n",
                "- `cancel_agent(agent_id)` — Cancel a running Agent\n",
                "- `review_result(agent_id)` — Submit a completed Agent's result for review\n",
                "- `list_agents` — List all active Agent instances\n",
                "\n",
                "## Guidelines\n",
                "\n",
                "- Always check agent status before sending follow-up messages\n",
                "- Review results before returning to the user\n",
                "- If a task can be handled directly without sub-Agents, do so\n",
                "- For complex tasks, break them down and assign to specialized Agents\n",
                "- If an Agent fails, analyze the error and try a different approach\n",
            ),
            template_list
        )
    }
}

/// Thread-safe handle for orchestration tools to interact with the Orchestrator.
///
/// This is a thin wrapper around `Arc<Orchestrator>` that the orchestration tools
/// hold to call into the Orchestrator's methods.
pub struct OrchestratorHandle {
    orchestrator: Arc<Orchestrator>,
}

impl OrchestratorHandle {
    pub fn new(orchestrator: Arc<Orchestrator>) -> Self {
        Self { orchestrator }
    }

    pub async fn create_agent(
        &self,
        template_id: &str,
        task: &str,
    ) -> Result<String, EngineError> {
        self.orchestrator.create_agent(template_id, task).await
    }

    pub async fn send_to_agent(&self, agent_id: &str, message: &str) -> Result<(), EngineError> {
        self.orchestrator.send_to_agent(agent_id, message).await
    }

    pub async fn get_agent_status(&self, agent_id: &str) -> Result<AgentInstance, EngineError> {
        self.orchestrator.get_agent_status(agent_id).await
    }

    pub async fn cancel_agent(&self, agent_id: &str) -> Result<(), EngineError> {
        self.orchestrator.cancel_agent(agent_id).await
    }

    pub async fn review_result(&self, agent_id: &str) -> Result<String, EngineError> {
        self.orchestrator.review_result(agent_id).await
    }

    pub async fn list_templates(&self) -> Vec<OrchestratorAgentTemplate> {
        self.orchestrator.list_templates().await
    }

    pub async fn list_agents(&self) -> Vec<AgentInstance> {
        self.orchestrator.list_agents().await
    }
}
