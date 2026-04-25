// INPUT:  serde, thiserror, alva_kernel_abi::bus_cap
// OUTPUT: RosterEntry, RosterEntryKind, MultiagentRoster, RosterError, MultiagentRosterCap
// POS:    **Harness-level** declarative multiagent roster. Mirrors Anthropic Managed Agents
//         `multiagent.agents[]`. The types live here in `alva-app-core` (not in
//         `alva-agent-core`) because the consumer is an Extension — `SubAgentExtension` /
//         `AgentSpawnTool` — not the agent loop or builder kernel. Validation (size,
//         distinctness, single self-ref) is exposed via `MultiagentRoster::validate`;
//         whichever extension wires the roster into the bus (via `MultiagentRosterCap`)
//         calls validate first. Declaring a roster remains **opt-in**: ad-hoc spawn keeps
//         working when no cap is published.

use alva_kernel_abi::bus_cap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Hard ceilings on roster size. Anthropic Managed Agents API caps at 20;
/// alva matches for parity. The minimum is 1 — an empty roster is just
/// "no roster", which the App should express by not calling
/// `AgentBuilder::multiagent_roster` at all.
pub const ROSTER_MIN_ENTRIES: usize = 1;
pub const ROSTER_MAX_ENTRIES: usize = 20;

/// What a roster slot points at. Mirrors Anthropic's three variants of
/// `MultiagentRosterEntryParams`: a plain id (resolves to "latest version"),
/// a versioned `(id, version)` reference, or the `self` sentinel meaning
/// "this agent may invoke itself".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RosterEntryKind {
    /// Reference another agent by id. `version = None` means "latest";
    /// `Some(n)` pins to that version.
    Agent { id: String, version: Option<u32> },
    /// Sentinel meaning "this agent" — allows recursive self-invocation.
    /// A roster may contain **at most one** `SelfRef`. Resolved server-side
    /// (or App-side in alva) to the agent that owns the roster.
    SelfRef,
}

/// One entry in a coordinator's roster. The kind says *who*; the
/// description is a free-form hint to the coordinator LLM about *when*
/// to spawn this agent. The description is also a natural place to put
/// per-roster guidance that's distinct from the spawnee's own system
/// prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterEntry {
    pub kind: RosterEntryKind,
    /// Optional hint for the coordinator. Rendered into the parent's
    /// system prompt by App-layer code that wants the LLM to see this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl RosterEntry {
    /// Reference another agent by id, latest version.
    pub fn agent(id: impl Into<String>) -> Self {
        Self {
            kind: RosterEntryKind::Agent {
                id: id.into(),
                version: None,
            },
            description: None,
        }
    }

    /// Reference another agent at a pinned version.
    pub fn versioned(id: impl Into<String>, version: u32) -> Self {
        Self {
            kind: RosterEntryKind::Agent {
                id: id.into(),
                version: Some(version),
            },
            description: None,
        }
    }

    /// `self` sentinel — the owning agent may invoke itself.
    pub fn self_ref() -> Self {
        Self {
            kind: RosterEntryKind::SelfRef,
            description: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Convenience: the agent id this entry resolves to, if any. Returns
    /// `None` for `SelfRef` (caller must substitute its own id).
    pub fn agent_id(&self) -> Option<&str> {
        match &self.kind {
            RosterEntryKind::Agent { id, .. } => Some(id.as_str()),
            RosterEntryKind::SelfRef => None,
        }
    }

    /// Pinned version, if any. `None` for `SelfRef` or unpinned agents.
    pub fn version(&self) -> Option<u32> {
        match &self.kind {
            RosterEntryKind::Agent { version, .. } => *version,
            RosterEntryKind::SelfRef => None,
        }
    }

    pub fn is_self(&self) -> bool {
        matches!(self.kind, RosterEntryKind::SelfRef)
    }
}

/// A coordinator's full declared roster. Constructed with the builder
/// pattern then handed to `AgentBuilder::multiagent_roster`. Validation
/// runs at `AgentBuilder::build` time — invalid rosters fail the build
/// with `AgentError::Other` carrying the underlying `RosterError`.
///
/// Invariants enforced by `validate`:
/// - `ROSTER_MIN_ENTRIES..=ROSTER_MAX_ENTRIES` entries
/// - Distinct agent ids (after resolving SelfRef to "self"; SelfRef
///   doesn't collide with real agent ids)
/// - At most one `SelfRef`
///
/// Entries that pin different versions of the **same agent id** are
/// considered duplicates — pick one. Anthropic enforces the same.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MultiagentRoster {
    pub entries: Vec<RosterEntry>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RosterError {
    #[error("multiagent roster size {got} is out of bounds (min {min}, max {max})")]
    BadSize { got: usize, min: usize, max: usize },

    #[error("duplicate agent id in multiagent roster: '{0}'")]
    DuplicateId(String),

    #[error("multiagent roster may contain at most one self-reference (got {count})")]
    MultipleSelf { count: usize },
}

impl MultiagentRoster {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an entry. Convenience for chained builder style.
    pub fn with_entry(mut self, entry: RosterEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Build from a vec of entries.
    pub fn from_entries(entries: Vec<RosterEntry>) -> Self {
        Self { entries }
    }

    /// Run the parity-required validation. Called automatically by
    /// `AgentBuilder::build`; expose it so App-side code can pre-validate
    /// before passing to the builder.
    pub fn validate(&self) -> Result<(), RosterError> {
        let n = self.entries.len();
        if !(ROSTER_MIN_ENTRIES..=ROSTER_MAX_ENTRIES).contains(&n) {
            return Err(RosterError::BadSize {
                got: n,
                min: ROSTER_MIN_ENTRIES,
                max: ROSTER_MAX_ENTRIES,
            });
        }

        let mut self_count = 0;
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for entry in &self.entries {
            match &entry.kind {
                RosterEntryKind::SelfRef => {
                    self_count += 1;
                }
                RosterEntryKind::Agent { id, .. } => {
                    if !seen.insert(id.as_str()) {
                        return Err(RosterError::DuplicateId(id.clone()));
                    }
                }
            }
        }
        if self_count > 1 {
            return Err(RosterError::MultipleSelf { count: self_count });
        }
        Ok(())
    }

    /// True if `agent_id` matches any non-SelfRef entry in the roster.
    /// Useful for App-side strict-mode enforcement.
    pub fn contains_agent(&self, agent_id: &str) -> bool {
        self.entries
            .iter()
            .any(|e| e.agent_id() == Some(agent_id))
    }

    /// True if the roster contains a SelfRef sentinel.
    pub fn allows_self(&self) -> bool {
        self.entries.iter().any(|e| e.is_self())
    }

    /// Iterate non-self entries' agent ids — the concrete set of distinct
    /// agents this coordinator may spawn (excluding self-invocation).
    pub fn agent_ids(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().filter_map(|e| e.agent_id())
    }
}

/// Bus capability: read-only view of the agent's declared multiagent
/// roster, published by `AgentBuilder::build` when one was set.
///
/// **Provider**: `alva_agent_core::AgentBuilder::build` — publishes after
/// validation. Absent on the bus when no roster was declared (= ad-hoc
/// spawn semantics, same as today).
///
/// **Consumers**: `AgentSpawnTool` (alva-app-core) and any orchestrator /
/// UI surface that wants to enforce, render, or describe the declared set.
/// When `strict` is true, consumers SHOULD reject spawn targets not in
/// the roster; when false, the roster is advisory metadata.
///
/// **Why bus**: the roster is shared state owned by the agent builder but
/// consumed by middleware / tools spread across app crates. Bus-based
/// discovery avoids threading it through every call site, and matches
/// how `ApprovalNotifier` etc. work.
#[bus_cap]
#[derive(Debug, Clone)]
pub struct MultiagentRosterCap {
    pub roster: MultiagentRoster,
    /// When true, consumers MUST reject spawn targets not in the roster.
    /// When false, the roster is advisory — App-side code may still log,
    /// surface in UI, or render in system prompts.
    pub strict: bool,
}

impl MultiagentRosterCap {
    pub fn new(roster: MultiagentRoster) -> Self {
        Self {
            roster,
            strict: false,
        }
    }

    pub fn strict(mut self) -> Self {
        self.strict = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_entry_builds_with_latest_version() {
        let e = RosterEntry::agent("worker");
        assert_eq!(e.agent_id(), Some("worker"));
        assert_eq!(e.version(), None);
        assert!(!e.is_self());
    }

    #[test]
    fn versioned_entry_pins_version() {
        let e = RosterEntry::versioned("worker", 3);
        assert_eq!(e.agent_id(), Some("worker"));
        assert_eq!(e.version(), Some(3));
    }

    #[test]
    fn self_ref_entry_is_classified() {
        let e = RosterEntry::self_ref();
        assert!(e.is_self());
        assert_eq!(e.agent_id(), None);
    }

    #[test]
    fn description_attaches() {
        let e = RosterEntry::agent("worker").with_description("does the work");
        assert_eq!(e.description.as_deref(), Some("does the work"));
    }

    #[test]
    fn empty_roster_fails_validation() {
        let r = MultiagentRoster::new();
        let err = r.validate().unwrap_err();
        assert_eq!(
            err,
            RosterError::BadSize {
                got: 0,
                min: ROSTER_MIN_ENTRIES,
                max: ROSTER_MAX_ENTRIES,
            }
        );
    }

    #[test]
    fn over_size_roster_fails() {
        let mut r = MultiagentRoster::new();
        for i in 0..(ROSTER_MAX_ENTRIES + 1) {
            r.entries.push(RosterEntry::agent(format!("worker-{i}")));
        }
        let err = r.validate().unwrap_err();
        assert!(matches!(err, RosterError::BadSize { .. }));
    }

    #[test]
    fn single_entry_is_valid() {
        let r = MultiagentRoster::new().with_entry(RosterEntry::agent("worker"));
        r.validate().unwrap();
    }

    #[test]
    fn duplicate_agent_id_fails() {
        let r = MultiagentRoster::new()
            .with_entry(RosterEntry::agent("worker"))
            .with_entry(RosterEntry::agent("worker"));
        let err = r.validate().unwrap_err();
        assert_eq!(err, RosterError::DuplicateId("worker".into()));
    }

    #[test]
    fn same_id_different_version_still_dupe() {
        // Anthropic: distinct agents after resolving — same id with different
        // version is still a duplicate.
        let r = MultiagentRoster::new()
            .with_entry(RosterEntry::versioned("worker", 1))
            .with_entry(RosterEntry::versioned("worker", 2));
        let err = r.validate().unwrap_err();
        assert!(matches!(err, RosterError::DuplicateId(id) if id == "worker"));
    }

    #[test]
    fn one_self_ref_is_fine() {
        let r = MultiagentRoster::new()
            .with_entry(RosterEntry::agent("worker"))
            .with_entry(RosterEntry::self_ref());
        r.validate().unwrap();
        assert!(r.allows_self());
    }

    #[test]
    fn multiple_self_refs_fail() {
        let r = MultiagentRoster::new()
            .with_entry(RosterEntry::self_ref())
            .with_entry(RosterEntry::self_ref());
        let err = r.validate().unwrap_err();
        assert_eq!(err, RosterError::MultipleSelf { count: 2 });
    }

    #[test]
    fn contains_agent_lookup() {
        let r = MultiagentRoster::new()
            .with_entry(RosterEntry::agent("planner"))
            .with_entry(RosterEntry::agent("executor"))
            .with_entry(RosterEntry::self_ref());
        assert!(r.contains_agent("planner"));
        assert!(r.contains_agent("executor"));
        assert!(!r.contains_agent("unknown"));
        // SelfRef doesn't satisfy contains_agent — App code resolves self
        // separately by comparing against the owning agent's id.
        assert!(!r.contains_agent("self"));
        assert!(r.allows_self());
    }

    #[test]
    fn agent_ids_iter_skips_self_ref() {
        let r = MultiagentRoster::new()
            .with_entry(RosterEntry::agent("a"))
            .with_entry(RosterEntry::self_ref())
            .with_entry(RosterEntry::agent("b"));
        let ids: Vec<_> = r.agent_ids().collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn cap_strict_builder() {
        let cap = MultiagentRosterCap::new(
            MultiagentRoster::new().with_entry(RosterEntry::agent("worker")),
        );
        assert!(!cap.strict);
        let strict = cap.strict();
        assert!(strict.strict);
    }

    #[test]
    fn round_trips_through_json() {
        let r = MultiagentRoster::new()
            .with_entry(RosterEntry::agent("planner").with_description("plans things"))
            .with_entry(RosterEntry::versioned("executor", 7))
            .with_entry(RosterEntry::self_ref());
        let json = serde_json::to_string(&r).unwrap();
        let back: MultiagentRoster = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
