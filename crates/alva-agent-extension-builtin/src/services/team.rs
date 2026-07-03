//! `TeamService` trait + default in-memory backend.
//!
//! Tools (`team_create`/`team_delete`/`send_message`) call into a shared
//! `dyn TeamService` resolved from the bus. The default
//! `InMemoryTeamStore` is a process-local roster + per-recipient inbox,
//! sufficient for in-process multi-agent coordination tests.
//!
//! # Examples
//!
//! Register a teammate, send a message, and read the inbox:
//!
//! ```rust,no_run
//! use alva_agent_extension_builtin::services::{
//!     InMemoryTeamStore, TeamMessage, TeamService, Teammate,
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let store = InMemoryTeamStore::new();
//!
//! // 1. Add a teammate. Re-adding the same name returns
//! //    TeamError::AlreadyExists rather than overwriting.
//! store.create(Teammate {
//!     name: "alice".into(),
//!     agent_type: "researcher".into(),
//!     system_prompt: Some("You are a meticulous researcher.".into()),
//! }).await?;
//!
//! // 2. Deliver a message — recipient must already be registered or
//! //    send_message returns TeamError::NotFound.
//! store.send_message(TeamMessage {
//!     from: "coordinator".into(),
//!     to: "alice".into(),
//!     body: "please research vector DBs".into(),
//!     summary: Some("vec-db research".into()),
//!     timestamp: 1_700_000_000,
//! }).await?;
//!
//! // 3. Drain-style inbox — returns every message in FIFO order; does
//! //    NOT pop, callers tracking once-only semantics keep their own
//! //    read offsets.
//! let inbox = store.inbox("alice").await;
//! assert_eq!(inbox[0].body, "please research vector DBs");
//!
//! // 4. Removing a teammate keeps their inbox intact (audit trail);
//! //    callers wanting a hard reset should clear separately.
//! store.delete("alice").await?;
//! # Ok(())
//! # }
//! ```
//!
//! Swap the in-memory backend for a real one (e.g. Redis Streams,
//! durable queue) by implementing [`TeamService`] on your own type and
//! registering an extension with `name() == "team"`.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

/// A member of the team. `agent_type` is a free-form discriminator (e.g.
/// "researcher", "coder") that the spawning runtime interprets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Teammate {
    pub name: String,
    pub agent_type: String,
    pub system_prompt: Option<String>,
}

/// A delivered (queued) message in a teammate's inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamMessage {
    pub from: String,
    pub to: String,
    pub body: String,
    /// Optional one-line summary the sender supplied. Useful when the
    /// inbox UI wants to show a digest without expanding every entry.
    pub summary: Option<String>,
    /// Unix seconds at send time.
    pub timestamp: u64,
}

/// Errors raised by `TeamService` mutations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamError {
    NotFound(String),
    AlreadyExists(String),
}

impl std::fmt::Display for TeamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(n) => write!(f, "teammate not found: {n}"),
            Self::AlreadyExists(n) => write!(f, "teammate already exists: {n}"),
        }
    }
}

impl std::error::Error for TeamError {}

#[async_trait]
pub trait TeamService: Send + Sync + 'static {
    async fn create(&self, mate: Teammate) -> Result<(), TeamError>;
    async fn delete(&self, name: &str) -> Result<(), TeamError>;
    async fn list(&self) -> Vec<Teammate>;
    async fn get(&self, name: &str) -> Option<Teammate>;
    /// Deliver a message. Recipient (`msg.to`) must already exist —
    /// otherwise `TeamError::NotFound`. Sender (`msg.from`) is recorded
    /// verbatim and not validated; agents and external callers both use
    /// this API and there's no canonical "self" reference here.
    async fn send_message(&self, msg: TeamMessage) -> Result<(), TeamError>;
    /// Drain-style read: returns every message queued for `recipient` in
    /// FIFO order. Doesn't pop — call sites that want once-only semantics
    /// should track read offsets themselves.
    async fn inbox(&self, recipient: &str) -> Vec<TeamMessage>;
}

/// Default in-process backend.
pub struct InMemoryTeamStore {
    members: Mutex<HashMap<String, Teammate>>,
    messages: Mutex<HashMap<String, Vec<TeamMessage>>>,
}

impl InMemoryTeamStore {
    pub fn new() -> Self {
        Self {
            members: Mutex::new(HashMap::new()),
            messages: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryTeamStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TeamService for InMemoryTeamStore {
    async fn create(&self, mate: Teammate) -> Result<(), TeamError> {
        let mut members = self.members.lock().unwrap_or_else(|e| e.into_inner());
        if members.contains_key(&mate.name) {
            return Err(TeamError::AlreadyExists(mate.name));
        }
        members.insert(mate.name.clone(), mate);
        Ok(())
    }

    async fn delete(&self, name: &str) -> Result<(), TeamError> {
        let mut members = self.members.lock().unwrap_or_else(|e| e.into_inner());
        members
            .remove(name)
            .ok_or_else(|| TeamError::NotFound(name.to_string()))?;
        // Don't drop the inbox — a user might delete then recreate the
        // teammate to "reset" but keep the audit trail; if that's not
        // desirable, callers can clear separately.
        Ok(())
    }

    async fn list(&self) -> Vec<Teammate> {
        let members = self.members.lock().unwrap_or_else(|e| e.into_inner());
        let mut out: Vec<_> = members.values().cloned().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    async fn get(&self, name: &str) -> Option<Teammate> {
        self.members
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .cloned()
    }

    async fn send_message(&self, msg: TeamMessage) -> Result<(), TeamError> {
        let exists = self
            .members
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&msg.to);
        if !exists {
            return Err(TeamError::NotFound(msg.to.clone()));
        }
        self.messages
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(msg.to.clone())
            .or_default()
            .push(msg);
        Ok(())
    }

    async fn inbox(&self, recipient: &str) -> Vec<TeamMessage> {
        self.messages
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(recipient)
            .cloned()
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn mate(name: &str) -> Teammate {
        Teammate {
            name: name.into(),
            agent_type: "generic".into(),
            system_prompt: None,
        }
    }

    fn msg(from: &str, to: &str, body: &str) -> TeamMessage {
        TeamMessage {
            from: from.into(),
            to: to.into(),
            body: body.into(),
            summary: None,
            timestamp: 1,
        }
    }

    #[tokio::test]
    async fn create_then_list_sorted() {
        let store = InMemoryTeamStore::new();
        store.create(mate("zoe")).await.unwrap();
        store.create(mate("alice")).await.unwrap();
        let list = store.list().await;
        assert_eq!(list[0].name, "alice");
        assert_eq!(list[1].name, "zoe");
    }

    #[tokio::test]
    async fn create_duplicate_rejects() {
        let store = InMemoryTeamStore::new();
        store.create(mate("alice")).await.unwrap();
        let err = store.create(mate("alice")).await.unwrap_err();
        assert_eq!(err, TeamError::AlreadyExists("alice".into()));
    }

    #[tokio::test]
    async fn delete_then_get_returns_none() {
        let store = InMemoryTeamStore::new();
        store.create(mate("alice")).await.unwrap();
        store.delete("alice").await.unwrap();
        assert!(store.get("alice").await.is_none());
    }

    #[tokio::test]
    async fn delete_unknown_errors() {
        let store = InMemoryTeamStore::new();
        let err = store.delete("nope").await.unwrap_err();
        assert_eq!(err, TeamError::NotFound("nope".into()));
    }

    #[tokio::test]
    async fn send_to_unknown_errors() {
        let store = InMemoryTeamStore::new();
        let err = store
            .send_message(msg("me", "nope", "hi"))
            .await
            .unwrap_err();
        assert_eq!(err, TeamError::NotFound("nope".into()));
    }

    #[tokio::test]
    async fn send_then_inbox_fifo() {
        let store = InMemoryTeamStore::new();
        store.create(mate("alice")).await.unwrap();
        store
            .send_message(msg("me", "alice", "first"))
            .await
            .unwrap();
        store
            .send_message(msg("me", "alice", "second"))
            .await
            .unwrap();
        let inbox = store.inbox("alice").await;
        assert_eq!(inbox.len(), 2);
        assert_eq!(inbox[0].body, "first");
        assert_eq!(inbox[1].body, "second");
    }

    #[tokio::test]
    async fn inbox_empty_for_unknown_recipient() {
        let store = InMemoryTeamStore::new();
        assert!(store.inbox("nobody").await.is_empty());
    }

    /// Concurrency guard (mirrors `InMemoryTaskStore::concurrent_…`):
    /// register one recipient, fan out N concurrent senders, then assert
    /// the inbox preserves every message — no lost writes under contention.
    /// FIFO ordering is intentionally NOT asserted here (concurrent senders
    /// can interleave arbitrarily); we just verify count + presence by body.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_send_message_no_lost_writes() {
        use std::sync::Arc;
        let store = Arc::new(InMemoryTeamStore::new());
        store.create(mate("alice")).await.unwrap();

        let n = 50;
        let mut handles = Vec::with_capacity(n);
        for i in 0..n {
            let s = store.clone();
            handles.push(tokio::spawn(async move {
                s.send_message(TeamMessage {
                    from: format!("sender-{i}"),
                    to: "alice".into(),
                    body: format!("msg-{i}"),
                    summary: None,
                    timestamp: i as u64,
                })
                .await
                .expect("send should succeed");
            }));
        }
        for h in handles {
            h.await.expect("send task should join");
        }

        let inbox = store.inbox("alice").await;
        assert_eq!(inbox.len(), n, "expected {n} messages, got {}", inbox.len());
        // Each msg-i must appear exactly once
        let mut bodies: Vec<&str> = inbox.iter().map(|m| m.body.as_str()).collect();
        bodies.sort();
        for i in 0..n {
            let expected = format!("msg-{i}");
            assert!(
                bodies.binary_search(&expected.as_str()).is_ok(),
                "{expected} missing"
            );
        }
    }
}
