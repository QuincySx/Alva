// INPUT:  serde
// OUTPUT: AgentProfile
// POS:    Agent identity and relationship descriptor for blackboard registration.

use serde::{Deserialize, Serialize};

/// An agent's self-description: who it is, what it does, and how it
/// relates to other agents in the collaboration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    /// Unique identifier (e.g., "planner", "generator-1", "evaluator").
    pub id: String,
    /// Human-readable role description (injected into everyone's context).
    pub role: String,
    /// What this agent can do.
    pub capabilities: Vec<String>,
    /// Agents whose output this agent depends on.
    pub depends_on: Vec<String>,
    /// Agents that depend on this agent's output.
    pub provides_to: Vec<String>,
}

impl AgentProfile {
    pub fn new(id: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: role.into(),
            capabilities: Vec::new(),
            depends_on: Vec::new(),
            provides_to: Vec::new(),
        }
    }

    pub fn with_capability(mut self, cap: impl Into<String>) -> Self {
        self.capabilities.push(cap.into());
        self
    }

    pub fn depends_on<I, S>(mut self, deps: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.depends_on.extend(deps.into_iter().map(Into::into));
        self
    }

    pub fn provides_to<I, S>(mut self, targets: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.provides_to.extend(targets.into_iter().map(Into::into));
        self
    }

    /// Build the "team prompt" section describing this agent's perspective.
    ///
    /// Given the full roster, generates text like:
    /// ```text
    /// ## 团队
    /// 你是 generator，负责代码实现。
    ///
    /// 房间里还有：
    /// - planner（需求分析）— 你依赖他的产出
    /// - evaluator（质量评审）— 他依赖你的产出
    ///
    /// 沟通规则：
    /// - 收到 @generator 的消息要回应
    /// - 完成产出后 @evaluator 请求评审
    /// ```
    pub fn build_team_prompt(&self, all_profiles: &[AgentProfile]) -> String {
        let mut s = String::new();

        s.push_str(&format!(
            "## Team\n\nYou are **{}**, responsible for: {}.\n",
            self.id, self.role,
        ));

        if !self.capabilities.is_empty() {
            s.push_str(&format!(
                "Your capabilities: {}.\n",
                self.capabilities.join(", "),
            ));
        }

        let peers: Vec<&AgentProfile> = all_profiles
            .iter()
            .filter(|p| p.id != self.id)
            .collect();

        if !peers.is_empty() {
            s.push_str("\nTeam members:\n");
            for peer in &peers {
                let relation = self.describe_relation(peer);
                s.push_str(&format!("- **{}**（{}）{}\n", peer.id, peer.role, relation));
            }
        }

        s.push_str("\nCommunication rules:\n");
        s.push_str(&format!(
            "- When someone mentions @{}, you must respond.\n",
            self.id
        ));

        for dep in &self.depends_on {
            s.push_str(&format!(
                "- Watch for artifacts and status updates from @{}.\n",
                dep
            ));
        }

        for target in &self.provides_to {
            s.push_str(&format!(
                "- When you finish work, notify @{} with your output.\n",
                target
            ));
        }

        s.push_str("- Use @name to address specific team members.\n");
        s.push_str("- Post status updates when you start, get blocked, or finish.\n");

        s
    }

    fn describe_relation(&self, other: &AgentProfile) -> String {
        let is_dep = self.depends_on.contains(&other.id);
        let is_target = self.provides_to.contains(&other.id);

        match (is_dep, is_target) {
            (true, true) => " — bidirectional dependency".to_string(),
            (true, false) => " — you depend on their output".to_string(),
            (false, true) => " — they depend on your output".to_string(),
            (false, false) => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_prompt_contains_relations() {
        let profiles = vec![
            AgentProfile::new("planner", "requirements"),
            AgentProfile::new("generator", "coding")
                .depends_on(["planner"])
                .provides_to(["evaluator"]),
            AgentProfile::new("evaluator", "review")
                .depends_on(["generator"]),
        ];

        let prompt = profiles[1].build_team_prompt(&profiles);

        assert!(prompt.contains("You are **generator**"));
        assert!(prompt.contains("planner"));
        assert!(prompt.contains("evaluator"));
        assert!(prompt.contains("you depend on their output"));
        assert!(prompt.contains("they depend on your output"));
        assert!(!prompt.contains("**generator**（")); // shouldn't list self as peer
    }

    #[test]
    fn team_prompt_with_capabilities() {
        let profiles = vec![
            AgentProfile::new("gen", "coding").with_capability("write rust"),
        ];

        let prompt = profiles[0].build_team_prompt(&profiles);
        assert!(prompt.contains("write rust"));
    }
}
