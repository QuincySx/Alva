use serde::{Deserialize, Serialize};

use crate::domain::agent::LLMConfig;

/// Agent template — defines a class of Agent's capabilities and configuration.
///
/// Templates are the blueprints from which `AgentInstance`s are created.
/// The brain Agent picks a template based on task analysis, then the
/// Orchestrator instantiates it into a running Agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorAgentTemplate {
    /// Unique template ID (kebab-case), e.g. "browser-agent"
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description used by brain Agent to decide which template to use
    pub description: String,
    /// System prompt for instances created from this template
    pub system_prompt: String,
    /// Skill IDs to load for this Agent type
    pub skills: Vec<String>,
    /// MCP Server IDs to load for this Agent type
    pub mcp_servers: Vec<String>,
    /// Allowed tool names (None = all tools available)
    pub tools: Option<Vec<String>>,
    /// LLM configuration for this Agent type
    pub llm_config: LLMConfig,
}

/// Predefined template catalog — returns the built-in Agent templates.
///
/// These templates cover the four common Agent archetypes:
/// - browser-agent: web browsing and scraping
/// - coding-agent: code generation via ACP (Claude Code, etc.)
/// - system-agent: shell commands, file operations, search
/// - research-agent: search + browse + summarize
pub fn predefined_templates(default_llm: &LLMConfig) -> Vec<OrchestratorAgentTemplate> {
    vec![
        OrchestratorAgentTemplate {
            id: "browser-agent".to_string(),
            name: "Browser Agent".to_string(),
            description: "Specializes in web browsing, page interaction, data extraction, \
                          and form filling. Use for any task that requires visiting URLs, \
                          scraping content, or automating browser workflows."
                .to_string(),
            system_prompt: concat!(
                "You are a browser automation specialist. Your capabilities include:\n",
                "- Navigating to URLs and interacting with web pages\n",
                "- Extracting data from web pages (text, tables, links)\n",
                "- Filling forms and clicking buttons\n",
                "- Taking screenshots and analyzing page content\n",
                "\n",
                "Always report your findings in a structured format. ",
                "If a page fails to load, try alternative approaches before reporting failure.",
            )
            .to_string(),
            skills: vec!["browser-automation".to_string()],
            mcp_servers: vec![],
            tools: None,
            llm_config: default_llm.clone(),
        },
        OrchestratorAgentTemplate {
            id: "coding-agent".to_string(),
            name: "Coding Agent".to_string(),
            description: "Specializes in code generation, editing, refactoring, and debugging. \
                          Delegates to external coding Agents (Claude Code, Qwen Code, Codex) \
                          via ACP protocol for high-quality code output."
                .to_string(),
            system_prompt: concat!(
                "You are a coding specialist. Your capabilities include:\n",
                "- Writing new code from specifications\n",
                "- Editing and refactoring existing code\n",
                "- Debugging and fixing issues\n",
                "- Code review and quality analysis\n",
                "\n",
                "When delegating to external coding Agents via ACP, provide clear context: \n",
                "the task description, relevant file paths, and expected outcomes.",
            )
            .to_string(),
            skills: vec![],
            mcp_servers: vec![],
            tools: None,
            llm_config: default_llm.clone(),
        },
        OrchestratorAgentTemplate {
            id: "system-agent".to_string(),
            name: "System Agent".to_string(),
            description: "Specializes in system operations: running shell commands, \
                          file management (create/read/write/move/delete), searching files, \
                          and managing processes. Use for any OS-level task."
                .to_string(),
            system_prompt: concat!(
                "You are a system operations specialist. Your capabilities include:\n",
                "- Running shell commands and scripts\n",
                "- File operations (create, read, write, move, delete)\n",
                "- Searching files by content or name\n",
                "- Managing processes\n",
                "\n",
                "Always validate paths before destructive operations. ",
                "Report command outputs clearly. If a command fails, explain why and suggest alternatives.",
            )
            .to_string(),
            skills: vec![],
            mcp_servers: vec![],
            tools: None,
            llm_config: default_llm.clone(),
        },
        OrchestratorAgentTemplate {
            id: "research-agent".to_string(),
            name: "Research Agent".to_string(),
            description: "Specializes in information gathering: web search, browsing results, \
                          reading documents, and synthesizing findings into structured summaries. \
                          Use for research, fact-checking, and information synthesis tasks."
                .to_string(),
            system_prompt: concat!(
                "You are a research specialist. Your capabilities include:\n",
                "- Web searching for information\n",
                "- Browsing and reading web pages\n",
                "- Analyzing and comparing multiple sources\n",
                "- Synthesizing findings into clear, structured reports\n",
                "\n",
                "Always cite your sources. Cross-reference information from multiple sources. ",
                "Present findings in a structured format with key takeaways.",
            )
            .to_string(),
            skills: vec!["browser-automation".to_string()],
            mcp_servers: vec![],
            tools: None,
            llm_config: default_llm.clone(),
        },
    ]
}
