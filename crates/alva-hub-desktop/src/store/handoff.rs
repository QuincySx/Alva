// INPUT:  handoff_packages rows + workspace skill/MCP discovery + session memory
// OUTPUT: HandoffPackage DTOs, handoff build/persist/mark methods on HubStore
// POS:    cross-engine handoff packaging cluster of the Hub store.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use serde::{Deserialize, Serialize};

use super::branch::EngineTarget;
use super::turn::SideEffectEvidence;
use super::{
    bounded_text, now_unix_ms, query_session_memory, STATUS_COMPLETED, STATUS_FAILED,
    STATUS_INTERRUPTED, STATUS_RUNNING,
};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HandoffKind {
    #[default]
    Full,
    Delta,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffCompletedWork {
    pub prompt: String,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffPendingWork {
    pub prompt: String,
    pub reason: String,
}

/// One discoverable workspace skill, name + description only — the body is
/// a file the target engine reads on demand, so the handoff stays small.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSnapshot {
    pub name: String,
    pub description: String,
    /// Workspace-relative path to the SKILL.md, so the target engine can
    /// open it without guessing.
    pub path: String,
}

impl SkillSnapshot {
    fn from_dir(skill_dir: &Path) -> Option<SkillSnapshot> {
        let path = skill_dir.join("SKILL.md");
        let content = std::fs::read_to_string(&path).ok()?;
        let name = skill_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unnamed")
            .to_string();
        // Frontmatter description line if present, else the first non-blank
        // line — enough for the target engine to decide whether to read it.
        let description = content
            .lines()
            .find_map(|line| {
                let line = line.trim();
                (line.starts_with("description:") || line.starts_with("description :")).then(|| {
                    bounded_text(
                        line.split_once(':')
                            .map(|(_, value)| value)
                            .unwrap_or("")
                            .trim(),
                    )
                })
            })
            .or_else(|| {
                content
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .map(bounded_text)
            })
            .unwrap_or_default();
        Some(SkillSnapshot {
            name: name.clone(),
            description,
            path: format!(".alva/skills/{name}/SKILL.md"),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpSnapshot {
    pub id: String,
    pub display_name: String,
    /// Human line like "stdio: npx -y @x/y" or "sse: https://host/path"
    pub transport: String,
}

/// Scan workspace `.mcp.json` / `mcp.json` for MCP servers, accepting both
/// the Alva `{servers: {id: {transport: …}}}` and ecosystem
/// `{mcpServers: {id: {command, args, url}}}` shapes. Bounded; malformed
/// files are skipped — MCP discovery is an enhancement, never a handoff
/// failure.
pub(crate) fn scan_workspace_mcp(workspace: &Path) -> Vec<McpSnapshot> {
    const MAX_MCP: usize = 20;
    let mut out = Vec::new();
    for file in [workspace.join(".mcp.json"), workspace.join("mcp.json")] {
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let Some(servers) = value
            .get("servers")
            .or_else(|| value.get("mcpServers"))
            .and_then(|servers| servers.as_object())
        else {
            continue;
        };
        for (id, entry) in servers {
            if out.len() >= MAX_MCP {
                break;
            }
            let transport = match entry.get("transport") {
                Some(transport) => match transport.get("type").and_then(|kind| kind.as_str()) {
                    Some("stdio") => {
                        let command = transport
                            .get("command")
                            .and_then(|value| value.as_str())
                            .unwrap_or("?");
                        let args: Vec<&str> = transport
                            .get("args")
                            .and_then(|value| value.as_array())
                            .map(|array| array.iter().filter_map(|value| value.as_str()).collect())
                            .unwrap_or_default();
                        if args.is_empty() {
                            format!("stdio: {command}")
                        } else {
                            format!("stdio: {command} {}", args.join(" "))
                        }
                    }
                    Some("sse") => {
                        let url = transport
                            .get("url")
                            .and_then(|value| value.as_str())
                            .unwrap_or("?");
                        format!("sse: {url}")
                    }
                    _ => "unknown transport".to_string(),
                },
                // Ecosystem shape: entry fields are the transport directly.
                None => {
                    if let Some(command) = entry.get("command").and_then(|value| value.as_str()) {
                        format!("stdio: {command}")
                    } else if let Some(url) = entry.get("url").and_then(|value| value.as_str()) {
                        format!("sse: {url}")
                    } else {
                        "unknown transport".to_string()
                    }
                }
            };
            out.push(McpSnapshot {
                id: id.clone(),
                display_name: entry
                    .get("display_name")
                    .and_then(|value| value.as_str())
                    .unwrap_or(id)
                    .to_string(),
                transport,
            });
        }
    }
    out
}

/// Scan `<workspace>/.alva/skills/<name>/SKILL.md` entries (bounded: the
/// handoff lists skills, it does not ship their bodies). Missing directory
/// or unreadable entries are silently skipped — skills are an enhancement,
/// never a failure condition for a handoff.
pub(crate) fn scan_workspace_skills(workspace: &Path) -> Vec<SkillSnapshot> {
    const MAX_SKILLS: usize = 20;
    let skills_root = workspace.join(".alva").join("skills");
    let Ok(entries) = std::fs::read_dir(&skills_root) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            path.is_dir()
                .then(|| SkillSnapshot::from_dir(&path))
                .flatten()
        })
        .take(MAX_SKILLS)
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffPackage {
    pub handoff_id: String,
    pub product_session_id: String,
    pub source_branch_id: String,
    pub target_branch_id: String,
    pub source: EngineTarget,
    pub target: EngineTarget,
    #[serde(default)]
    pub kind: HandoffKind,
    pub reason: String,
    pub user_goal: Option<String>,
    pub latest_instruction: Option<String>,
    pub accepted_decisions: Vec<String>,
    pub completed_work: Vec<HandoffCompletedWork>,
    pub pending_work: Vec<HandoffPendingWork>,
    pub changed_files: Vec<String>,
    pub recent_commands: Vec<String>,
    pub verification_results: Vec<String>,
    pub workspace_summary: String,
    pub known_side_effects: Vec<SideEffectEvidence>,
    pub team_snapshot: Option<String>,
    /// Full filtered turn transcript — populated only when the user has
    /// explicitly enabled transcript sharing for the session.
    #[serde(default)]
    pub transcript: Vec<TranscriptTurn>,
    pub created_at: i64,
    pub used_at: Option<i64>,
    /// User-curated session memory: corrections and preferences every engine
    /// in this session should respect. Cross-engine by construction — it
    /// travels with every handoff the way Cindy shares memory across
    /// harnesses.
    #[serde(default)]
    pub session_memory: Vec<String>,
    /// Workspace skills the source session could follow: name, description
    /// and the SKILL.md location. The target engine can read the files
    /// itself — the handoff only has to point at them (Cindy's "teach a way
    /// of working once, reuse it everywhere").
    #[serde(default)]
    pub available_skills: Vec<SkillSnapshot>,
    /// Workspace MCP servers (metadata only; credentials never leave the
    /// machine, let alone cross engines).
    #[serde(default)]
    pub available_mcp: Vec<McpSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptTurn {
    pub prompt: String,
    pub result: Option<String>,
}

impl HandoffPackage {
    /// One-line summary shared by the live fallback card and the persisted
    /// `engine_fallback` timeline fact.
    pub(crate) fn fallback_summary(&self) -> String {
        format!(
            "full context · {} completed · {} pending",
            self.completed_work.len(),
            self.pending_work.len()
        )
    }

    pub(crate) fn render_for_engine(&self) -> String {
        let encoded = serde_json::to_string_pretty(self)
            .unwrap_or_else(|_| "{\"error\":\"handoff serialization failed\"}".to_string());
        let kind = match self.kind {
            HandoffKind::Full => "full",
            HandoffKind::Delta => "delta",
        };
        format!("<alva-hub-handoff kind=\"{kind}\">\n{encoded}\n</alva-hub-handoff>")
    }
}

impl super::HubStore {
        pub(crate) fn mark_handoff_used(&self, handoff_id: &str) -> Result<(), String> {
            let connection = self.lock()?;
            connection
                .execute(
                    "UPDATE handoff_packages SET used_at = COALESCE(used_at, ?2)
                      WHERE handoff_id = ?1",
                    params![handoff_id, now_unix_ms()],
                )
                .map_err(|error| format!("mark Hub handoff used: {error}"))?;
            Ok(())
        }
}

pub(crate) fn build_handoff_package(
    transaction: &Transaction<'_>,
    product_session_id: &str,
    source_branch_id: &str,
    target_branch_id: &str,
    source: EngineTarget,
    target: EngineTarget,
    reason: &str,
    now: i64,
    kind: HandoffKind,
    watermark_turn_rowid: Option<i64>,
) -> Result<HandoffPackage, String> {
    let (workspace, include_full_transcript) = transaction
        .query_row(
            "SELECT workspace, include_full_transcript
               FROM hub_sessions WHERE product_session_id = ?1",
            [product_session_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
        )
        .map_err(|error| format!("read workspace for Hub handoff: {error}"))?;
    let mut statement = transaction
        .prepare(
            "SELECT rowid, prompt, final_result, error_message, status
               FROM turns WHERE product_session_id = ?1
              ORDER BY created_at ASC, rowid ASC",
        )
        .map_err(|error| format!("prepare turn history for Hub handoff: {error}"))?;
    let rows = statement
        .query_map([product_session_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| format!("query turn history for Hub handoff: {error}"))?;
    let history = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("decode turn history for Hub handoff: {error}"))?;
    drop(statement);
    let user_goal = history.first().map(|row| bounded_text(&row.1));
    let latest_instruction = history.last().map(|row| bounded_text(&row.1));
    let completed_work = history
        .iter()
        .filter(|(rowid, _, _, _, _)| {
            watermark_turn_rowid.is_none_or(|watermark| *rowid > watermark)
        })
        .filter_map(|(_, prompt, result, _, status)| {
            (status == STATUS_COMPLETED).then(|| {
                result.as_ref().map(|result| HandoffCompletedWork {
                    prompt: bounded_text(prompt),
                    result: bounded_text(result),
                })
            })?
        })
        .collect();
    let pending_work = history
        .iter()
        .filter(|(rowid, _, _, _, _)| {
            watermark_turn_rowid.is_none_or(|watermark| *rowid > watermark)
        })
        .filter(|(_, _, _, _, status)| {
            matches!(status.as_str(), STATUS_FAILED | STATUS_INTERRUPTED)
        })
        .map(|(_, prompt, _, error, status)| HandoffPendingWork {
            prompt: bounded_text(prompt),
            reason: bounded_text(error.as_deref().unwrap_or(status)),
        })
        .collect();
    let mut side_effect_statement = transaction
        .prepare(
            "SELECT ea.side_effects_json
               FROM engine_attempts ea
               JOIN turns t ON t.turn_id = ea.turn_id
              WHERE ea.product_session_id = ?1
                AND (?2 IS NULL OR t.rowid > ?2)
              ORDER BY ea.started_at ASC, ea.rowid ASC",
        )
        .map_err(|error| format!("prepare side effects for Hub handoff: {error}"))?;
    let encoded = side_effect_statement
        .query_map(params![product_session_id, watermark_turn_rowid], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| format!("query side effects for Hub handoff: {error}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("decode side effects for Hub handoff: {error}"))?;
    let mut known_side_effects = Vec::new();
    for json in encoded {
        let mut evidence: Vec<SideEffectEvidence> = serde_json::from_str(&json)
            .map_err(|error| format!("decode side effects for Hub handoff: {error}"))?;
        known_side_effects.append(&mut evidence);
    }
    // Full transcript is opt-in per session; when enabled it passes the same
    // filter as every other field (redact_handoff below).
    let transcript = if include_full_transcript {
        history
            .iter()
            .map(|(_, prompt, result, error, _)| TranscriptTurn {
                prompt: prompt.clone(),
                result: result.clone().or_else(|| error.clone()),
            })
            .collect()
    } else {
        Vec::new()
    };
    // The session's memory rides along so the target engine continues with
    // the accumulated corrections, not from zero.
    let session_memory = query_session_memory(transaction, product_session_id)?;
    // Workspace skills point the target engine at the SKILL.md files it can
    // read directly — discovery without shipping bodies.
    let available_skills = scan_workspace_skills(Path::new(&workspace));
    // Workspace MCP servers, metadata only — the target engine reconnects
    // with its own credentials.
    let available_mcp = scan_workspace_mcp(Path::new(&workspace));

    let mut package = HandoffPackage {
        handoff_id: uuid::Uuid::new_v4().to_string(),
        product_session_id: product_session_id.to_string(),
        source_branch_id: source_branch_id.to_string(),
        target_branch_id: target_branch_id.to_string(),
        source,
        target,
        kind,
        reason: reason.to_string(),
        user_goal,
        latest_instruction,
        accepted_decisions: Vec::new(),
        completed_work,
        pending_work,
        changed_files: Vec::new(),
        recent_commands: Vec::new(),
        verification_results: Vec::new(),
        workspace_summary: format!(
            "Workspace (authoritative; inspect it before trusting transcript evidence): {workspace}"
        ),
        known_side_effects,
        // Team context travels with the handoff so a fallback engine knows
        // it is a lead (with workers) or a worker (with a task and queue).
        team_snapshot: build_team_snapshot(transaction, product_session_id)?,
        transcript,
        session_memory,
        available_skills,
        available_mcp,
        created_at: now,
        used_at: None,
    };
    // The one filter every cross-engine handoff passes, BEFORE persistence —
    // stored packages equal what the target adapter receives.
    crate::redact::redact_handoff(&mut package);
    Ok(package)
}

/// One line of collaboration context for whichever role this session plays
/// in a TeamRun, or None for a solo session.
pub(crate) fn build_team_snapshot(
    transaction: &Transaction<'_>,
    product_session_id: &str,
) -> Result<Option<String>, String> {
    // Lead of a run?
    let mut lead_line: Option<String> = None;
    let lead_run: Option<(String, String)> = transaction
        .query_row(
            "SELECT team_run_id, status FROM team_runs WHERE lead_session_id = ?1
              ORDER BY created_at DESC, rowid DESC LIMIT 1",
            [product_session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| format!("query Hub lead team run: {error}"))?;
    if let Some((team_run_id, status)) = lead_run {
        let mut statement = transaction
            .prepare(
                "SELECT eb.engine_id, tw.lifecycle,
                        (SELECT COUNT(*) FROM worker_tasks t
                          WHERE t.worker_id = tw.worker_id AND t.status = 'queued'),
                        (SELECT COUNT(*) FROM worker_tasks t
                          WHERE t.worker_id = tw.worker_id AND t.status = ?2)
                   FROM team_workers tw
                   JOIN hub_sessions hs ON hs.product_session_id = tw.worker_session_id
                   JOIN engine_branches eb ON eb.branch_id = hs.active_branch_id
                  WHERE tw.team_run_id = ?1
                  ORDER BY tw.created_at ASC, tw.rowid ASC",
            )
            .map_err(|error| format!("prepare Hub team snapshot: {error}"))?;
        let workers = statement
            .query_map(params![team_run_id, STATUS_RUNNING], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|error| format!("query Hub team snapshot: {error}"))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("decode Hub team snapshot: {error}"))?;
        drop(statement);
        let described = workers
            .iter()
            .map(|(engine, lifecycle, queued, running)| {
                format!("{engine} ({lifecycle}, {queued} queued, {running} running)")
            })
            .collect::<Vec<_>>()
            .join("; ");
        let liveness = if status == "active" {
            "Workers keep running independently; this handoff replaces only the lead engine."
        } else {
            "This run has ended; its workers are archived."
        };
        lead_line = Some(bounded_text(&format!(
            "Lead of TeamRun {team_run_id} ({status}) with {} worker(s): {described}. {liveness}",
            workers.len()
        )));
    }

    // A worker of a run? (A session can be BOTH: a worker session may lead
    // its own run, and then it needs both halves of the context.)
    let worker: Option<(String, String, String)> = transaction
        .query_row(
            "SELECT tw.team_run_id, tw.worker_id, tw.lifecycle FROM team_workers tw
              WHERE tw.worker_session_id = ?1",
            [product_session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| format!("query Hub worker team run: {error}"))?;
    let Some((team_run_id, worker_id, lifecycle)) = worker else {
        return Ok(lead_line);
    };
    // A running task has no changed_files yet (those are derived when it
    // finishes), so in-flight evidence must come from the live attempts.
    let current: Option<(String, i64)> = transaction
        .query_row(
            "SELECT scope, COALESCE(started_at, created_at) FROM worker_tasks
              WHERE worker_id = ?1 AND status = ?2
              ORDER BY created_at DESC, rowid DESC LIMIT 1",
            params![worker_id, STATUS_RUNNING],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| format!("query Hub worker current task: {error}"))?;
    let touched_files = match &current {
        Some((_, started_at)) => {
            let mut statement = transaction
                .prepare(
                    "SELECT side_effects_json FROM engine_attempts
                      WHERE product_session_id = ?1 AND started_at >= ?2",
                )
                .map_err(|error| format!("prepare Hub worker evidence snapshot: {error}"))?;
            let rows = statement
                .query_map(params![product_session_id, started_at], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|error| format!("query Hub worker evidence snapshot: {error}"))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| format!("decode Hub worker evidence snapshot: {error}"))?;
            drop(statement);
            let mut files: Vec<String> = Vec::new();
            for encoded in rows {
                let entries: Vec<SideEffectEvidence> = serde_json::from_str(&encoded)
                    .map_err(|error| format!("decode Hub worker evidence: {error}"))?;
                files.extend(
                    entries
                        .into_iter()
                        .filter(|evidence| evidence.succeeded == Some(true))
                        .filter_map(|evidence| evidence.path),
                );
            }
            files.sort();
            files.dedup();
            files
        }
        None => Vec::new(),
    };
    let mut queue_statement = transaction
        .prepare(
            "SELECT scope FROM worker_tasks
              WHERE worker_id = ?1 AND status = 'queued'
              ORDER BY created_at ASC, rowid ASC",
        )
        .map_err(|error| format!("prepare Hub worker queue snapshot: {error}"))?;
    let queued = queue_statement
        .query_map([&worker_id], |row| row.get::<_, String>(0))
        .map_err(|error| format!("query Hub worker queue snapshot: {error}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("decode Hub worker queue snapshot: {error}"))?;
    drop(queue_statement);
    let current_line = current
        .as_ref()
        .map(|(scope, _)| format!("current task: {}", bounded_text(scope)))
        .unwrap_or_else(|| "no task in flight".to_string());
    let evidence = if touched_files.is_empty() {
        String::new()
    } else {
        format!(
            " · files touched so far: {}",
            bounded_text(&touched_files.join(", "))
        )
    };
    const QUEUE_PREVIEW: usize = 20;
    let queue_line = if queued.is_empty() {
        "queue empty".to_string()
    } else {
        let preview = queued
            .iter()
            .take(QUEUE_PREVIEW)
            .map(|scope| bounded_text(scope))
            .collect::<Vec<_>>()
            .join(" | ");
        let overflow = queued.len().saturating_sub(QUEUE_PREVIEW);
        let suffix = if overflow > 0 {
            format!(" … and {overflow} more")
        } else {
            String::new()
        };
        format!("{} queued: {preview}{suffix}", queued.len())
    };
    let worker_liveness = if lifecycle == "active" {
        "The lead and sibling workers keep running; this handoff replaces only this worker's engine."
    } else {
        "This worker is not active; its queue is parked."
    };
    let worker_line = format!(
        "Worker ({lifecycle}) of TeamRun {team_run_id} — {current_line}; {queue_line}{evidence}. {worker_liveness}"
    );
    // A session can be BOTH a worker and the lead of its own run; the target
    // engine needs both halves, not whichever we happened to check first.
    Ok(Some(bounded_text(&match lead_line {
        Some(lead) => format!("{lead} {worker_line}"),
        None => worker_line,
    })))
}

pub(crate) fn persist_handoff_package(
    transaction: &Transaction<'_>,
    package: &HandoffPackage,
) -> Result<(), String> {
    let package_json = serde_json::to_string(package)
        .map_err(|error| format!("serialize {:?} Hub handoff: {error}", package.kind))?;
    transaction
        .execute(
            "INSERT INTO handoff_packages
               (handoff_id, product_session_id, source_branch_id, target_branch_id,
                kind, reason, package_json, created_at, used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)",
            params![
                package.handoff_id,
                package.product_session_id,
                package.source_branch_id,
                package.target_branch_id,
                handoff_kind_name(package.kind),
                package.reason,
                package_json,
                package.created_at
            ],
        )
        .map_err(|error| format!("persist Hub handoff: {error}"))?;
    Ok(())
}

pub(crate) fn query_handoffs(
    connection: &Connection,
    product_session_id: &str,
) -> Result<Vec<HandoffPackage>, String> {
    let mut statement = connection
        .prepare(
            "SELECT package_json, used_at FROM handoff_packages
              WHERE product_session_id = ?1
              ORDER BY created_at ASC, rowid ASC",
        )
        .map_err(|error| format!("prepare Hub handoffs: {error}"))?;
    let rows = statement
        .query_map([product_session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
        })
        .map_err(|error| format!("query Hub handoffs: {error}"))?;
    rows.map(|row| {
        let (json, used_at) = row.map_err(|error| format!("decode Hub handoff row: {error}"))?;
        let mut package: HandoffPackage = serde_json::from_str(&json)
            .map_err(|error| format!("decode Hub handoff package: {error}"))?;
        package.used_at = used_at;
        Ok(package)
    })
    .collect()
}

pub(crate) fn query_latest_handoff(
    connection: &Connection,
    target_branch_id: &str,
    kind: Option<HandoffKind>,
) -> Result<Option<HandoffPackage>, String> {
    let sql = if kind.is_some() {
        "SELECT package_json FROM handoff_packages
           WHERE target_branch_id = ?1 AND kind = ?2
           ORDER BY created_at DESC, rowid DESC LIMIT 1"
    } else {
        "SELECT package_json FROM handoff_packages
           WHERE target_branch_id = ?1
           ORDER BY created_at DESC, rowid DESC LIMIT 1"
    };
    let json = match kind {
        Some(kind) => connection.query_row(
            sql,
            params![target_branch_id, handoff_kind_name(kind)],
            |row| row.get::<_, String>(0),
        ),
        None => connection.query_row(sql, [target_branch_id], |row| row.get::<_, String>(0)),
    }
    .optional()
    .map_err(|error| format!("query latest Hub handoff: {error}"))?;
    json.map(|json| {
        serde_json::from_str(&json).map_err(|error| format!("decode latest Hub handoff: {error}"))
    })
    .transpose()
}

pub(crate) fn handoff_kind_name(kind: HandoffKind) -> &'static str {
    match kind {
        HandoffKind::Full => "full",
        HandoffKind::Delta => "delta",
    }
}

pub(crate) fn source_branch_for_handoff(prior: &Option<HandoffPackage>, fallback_branch_id: &str) -> String {
    prior
        .as_ref()
        .map(|package| package.source_branch_id.clone())
        .unwrap_or_else(|| fallback_branch_id.to_string())
}

