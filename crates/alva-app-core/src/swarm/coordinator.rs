// INPUT:  super::types::*, super::mailbox::*, std::collections::HashMap, std::sync::Arc, tokio::sync::RwLock
// OUTPUT: SwarmCoordinator
// POS:    Team lifecycle management — create/delete teams, track members, store agent summaries.

use super::mailbox::{MailboxSystem, SwarmMessage, SwarmMessageType};
use super::types::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Swarm coordinator — manages team lifecycle and inter-agent coordination.
///
/// The coordinator is the central registry for all active teams. It owns the
/// shared [`MailboxSystem`] and provides methods to create/delete teams, add/
/// remove members, track member status, and store periodic agent summaries.
pub struct SwarmCoordinator {
    /// Active teams indexed by team name.
    teams: Arc<RwLock<HashMap<String, TeamDefinition>>>,
    /// Shared mailbox system for inter-agent messaging.
    mailbox: Arc<MailboxSystem>,
    /// Per-agent summaries (updated periodically by background tasks).
    agent_summaries: Arc<RwLock<HashMap<String, String>>>,
}

impl SwarmCoordinator {
    pub fn new(mailbox: Arc<MailboxSystem>) -> Self {
        Self {
            teams: Arc::new(RwLock::new(HashMap::new())),
            mailbox,
            agent_summaries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new team and register it.
    pub async fn create_team(
        &self,
        name: String,
        description: String,
        agent_type: Option<String>,
    ) -> TeamDefinition {
        let team = TeamDefinition {
            name: name.clone(),
            description,
            agent_type,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            members: Vec::new(),
        };

        self.teams.write().await.insert(name, team.clone());
        team
    }

    /// Delete a team, shutting down all its members first.
    pub async fn delete_team(&self, name: &str) -> Result<(), String> {
        // Shutdown all members before removing the team
        if let Some(team) = self.teams.read().await.get(name) {
            for member in &team.members {
                self.shutdown_member(&member.id).await;
            }
        }

        self.teams
            .write()
            .await
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| format!("Team not found: {}", name))
    }

    /// Add a member to an existing team.
    pub async fn add_member(&self, team_name: &str, member: TeamMember) -> Result<(), String> {
        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| format!("Team not found: {}", team_name))?;
        team.members.push(member);
        Ok(())
    }

    /// Remove a member from a team by agent ID.
    pub async fn remove_member(&self, team_name: &str, agent_id: &str) -> Result<(), String> {
        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| format!("Team not found: {}", team_name))?;
        let before = team.members.len();
        team.members.retain(|m| m.id != agent_id);
        if team.members.len() == before {
            return Err(format!("Member not found: {}", agent_id));
        }
        Ok(())
    }

    /// Update a member's status across all teams.
    pub async fn update_member_status(&self, agent_id: &str, status: MemberStatus) {
        let mut teams = self.teams.write().await;
        for team in teams.values_mut() {
            for member in &mut team.members {
                if member.id == agent_id {
                    member.status = status;
                    return;
                }
            }
        }
    }

    /// Send a shutdown message to a member via the mailbox.
    async fn shutdown_member(&self, agent_id: &str) {
        let msg = SwarmMessage {
            from: "coordinator".to_string(),
            to: agent_id.to_string(),
            content: "Shutdown requested by team deletion".to_string(),
            summary: None,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            message_type: SwarmMessageType::ShutdownRequest,
        };
        let _ = self.mailbox.send(msg).await;
    }

    /// Update an agent's periodic summary (called by background monitor tasks).
    pub async fn update_summary(&self, agent_id: &str, summary: &str) {
        self.agent_summaries
            .write()
            .await
            .insert(agent_id.to_string(), summary.to_string());
    }

    /// Get the last known summary for an agent.
    pub async fn get_summary(&self, agent_id: &str) -> Option<String> {
        self.agent_summaries.read().await.get(agent_id).cloned()
    }

    /// List all registered teams.
    pub async fn list_teams(&self) -> Vec<TeamDefinition> {
        self.teams.read().await.values().cloned().collect()
    }

    /// Get a specific team by name.
    pub async fn get_team(&self, name: &str) -> Option<TeamDefinition> {
        self.teams.read().await.get(name).cloned()
    }

    /// Get a reference to the underlying mailbox system.
    pub fn mailbox(&self) -> &Arc<MailboxSystem> {
        &self.mailbox
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_coordinator() -> SwarmCoordinator {
        SwarmCoordinator::new(Arc::new(MailboxSystem::new()))
    }

    fn make_member(id: &str, name: &str) -> TeamMember {
        TeamMember {
            id: id.to_string(),
            name: name.to_string(),
            role: TeamRole::Worker,
            status: MemberStatus::Idle,
            model: None,
        }
    }

    #[tokio::test]
    async fn create_and_list_team() {
        let coord = make_coordinator();
        let team = coord
            .create_team("alpha".into(), "Test team".into(), None)
            .await;
        assert_eq!(team.name, "alpha");
        assert_eq!(team.members.len(), 0);

        let teams = coord.list_teams().await;
        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0].name, "alpha");
    }

    #[tokio::test]
    async fn add_and_remove_member() {
        let coord = make_coordinator();
        coord
            .create_team("alpha".into(), "Test".into(), None)
            .await;

        coord
            .add_member("alpha", make_member("a1", "Alice"))
            .await
            .unwrap();
        coord
            .add_member("alpha", make_member("a2", "Bob"))
            .await
            .unwrap();

        let team = coord.get_team("alpha").await.unwrap();
        assert_eq!(team.members.len(), 2);

        coord.remove_member("alpha", "a1").await.unwrap();
        let team = coord.get_team("alpha").await.unwrap();
        assert_eq!(team.members.len(), 1);
        assert_eq!(team.members[0].id, "a2");
    }

    #[tokio::test]
    async fn remove_member_from_unknown_team() {
        let coord = make_coordinator();
        let result = coord.remove_member("nonexistent", "a1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn update_member_status_across_teams() {
        let coord = make_coordinator();
        coord
            .create_team("alpha".into(), "Test".into(), None)
            .await;
        coord
            .add_member("alpha", make_member("a1", "Alice"))
            .await
            .unwrap();

        coord
            .update_member_status("a1", MemberStatus::Working)
            .await;
        let team = coord.get_team("alpha").await.unwrap();
        assert_eq!(team.members[0].status, MemberStatus::Working);
    }

    #[tokio::test]
    async fn delete_team() {
        let coord = make_coordinator();
        coord
            .create_team("alpha".into(), "Test".into(), None)
            .await;
        coord.delete_team("alpha").await.unwrap();
        assert!(coord.get_team("alpha").await.is_none());
    }

    #[tokio::test]
    async fn delete_nonexistent_team() {
        let coord = make_coordinator();
        let result = coord.delete_team("nope").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn agent_summaries() {
        let coord = make_coordinator();
        assert!(coord.get_summary("a1").await.is_none());

        coord.update_summary("a1", "Working on task X").await;
        assert_eq!(
            coord.get_summary("a1").await.unwrap(),
            "Working on task X"
        );

        coord.update_summary("a1", "Completed task X").await;
        assert_eq!(
            coord.get_summary("a1").await.unwrap(),
            "Completed task X"
        );
    }
}
