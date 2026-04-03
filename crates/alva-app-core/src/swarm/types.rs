// INPUT:  serde, std::collections::HashMap, std::path
// OUTPUT: TeamDefinition, TeamMember, TeamRole, MemberStatus, SwarmContext, AgentSpawnConfig, SpawnMode, IsolationMode
// POS:    Core types for multi-agent swarm infrastructure.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Team definition — describes a named group of collaborating agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamDefinition {
    /// Human-readable team name (also used as lookup key).
    pub name: String,
    /// What this team is for.
    pub description: String,
    /// Optional agent type hint (e.g. "coding", "research").
    pub agent_type: Option<String>,
    /// Unix timestamp (seconds) when the team was created.
    pub created_at: u64,
    /// Current team members.
    pub members: Vec<TeamMember>,
}

/// A team member (agent) within a team.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    /// Unique agent ID (generated at spawn time).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Role within the team.
    pub role: TeamRole,
    /// Current lifecycle status.
    pub status: MemberStatus,
    /// Optional model override for this member.
    pub model: Option<String>,
}

/// Role a team member plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRole {
    /// Orchestrates the team, delegates work.
    Leader,
    /// Executes tasks assigned by the leader.
    Worker,
    /// Coordinates between workers without directly executing tasks.
    Coordinator,
}

/// Lifecycle status of a team member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Idle,
    Working,
    WaitingForApproval,
    Completed,
    Failed,
    Shutdown,
}

/// Swarm context carried by agents participating in a team.
///
/// This is attached to an agent's extensions so it knows about its team
/// membership, leader, and peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmContext {
    /// Team name this agent belongs to.
    pub team_name: String,
    /// Whether this agent is the team leader.
    pub is_leader: bool,
    /// Leader agent ID (workers use this to send messages back).
    pub leader_id: Option<String>,
    /// Snapshot of current team members.
    pub members: Vec<TeamMember>,
    /// Shared task list ID (if using a collaborative task board).
    pub task_list_id: Option<String>,
}

/// Configuration for spawning a new agent into a team.
#[derive(Debug, Clone)]
pub struct AgentSpawnConfig {
    /// Name for the spawned agent.
    pub name: String,
    /// Initial prompt/task for the agent.
    pub prompt: String,
    /// Optional model override.
    pub model: Option<String>,
    /// Working directory for the agent.
    pub workspace: std::path::PathBuf,
    /// How to spawn (in-process, subprocess, tmux).
    pub mode: SpawnMode,
    /// Optional filesystem isolation strategy.
    pub isolation: Option<IsolationMode>,
    /// Whether to run in the background (non-blocking).
    pub run_in_background: bool,
    /// Maximum agent nesting depth.
    pub max_depth: usize,
}

/// How the agent process is spawned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnMode {
    /// Spawn as a tokio task within the current process.
    InProcess,
    /// Spawn as a separate OS process.
    Subprocess,
    /// Spawn in a tmux split pane.
    Tmux,
}

/// Filesystem isolation strategy for spawned agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationMode {
    /// Use `git worktree` for branch-level isolation.
    Worktree,
    /// Use a separate directory (copy or symlink).
    Directory,
}
