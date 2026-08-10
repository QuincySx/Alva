// INPUT:  hub-gpui backend DTOs (TimelineEvent, HandoffPackage, ...)
// OUTPUT: UI-projected event model shared by timeline / approval rendering
// POS:    Mirrors the old React `ProjectedEvent` shape so the GPUI port can
//         render the same cards 1:1. Pure value types, no gpui imports.

use alva_hub_gpui::{HandoffPackage, PermissionRequestInfo, TimelineStatus};

/// A fallback card's reversible controls: where it came from and the
/// handoff it actually delivered (loaded from the store on demand).
#[derive(Debug, Clone)]
pub struct FallbackControls {
    pub source_engine_id: String,
    pub handoff: Option<HandoffPackage>,
}

/// One timeline card. `created_at` is absent for live run events (they are
/// already in arrival order); persisted projections always carry it.
#[derive(Debug, Clone)]
pub struct ProjectedEvent {
    pub status: TimelineStatus,
    pub engine_id: String,
    pub product_session_id: String,
    pub message: Option<String>,
    pub permission_request: Option<PermissionRequestInfo>,
    pub created_at: Option<i64>,
    pub fallback: Option<FallbackControls>,
}

/// "now-relative" label for a millisecond timestamp, matching the old
/// React helper: "just now", "5m ago", "3h ago", "2d ago", else the date.
pub fn format_relative_time(updated_at_ms: i64, now_ms: i64) -> String {
    let delta = (now_ms - updated_at_ms).max(0);
    let min = delta / 60_000;
    if min < 1 {
        return "just now".to_string();
    }
    if min < 60 {
        return format!("{min}m ago");
    }
    let hours = min / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = hours / 24;
    if days < 7 {
        return format!("{days}d ago");
    }
    // Fall back to a calendar date.
    let secs = updated_at_ms / 1000;
    match chrono::DateTime::from_timestamp(secs, 0) {
        Some(dt) => dt.format("%Y-%m-%d").to_string(),
        None => String::new(),
    }
}

/// Human label for a permission profile, matching the React `profileLabel`.
pub fn profile_label(profile: &str) -> String {
    match profile {
        "interactive" => "Ask before risky actions".to_string(),
        "workspace_write" => "Workspace writes allowed".to_string(),
        "unrestricted" => "Everything allowed".to_string(),
        other => other.to_string(),
    }
}

/// Readiness badge label, matching the React `readinessBadge` map.
pub fn readiness_badge(status: &str) -> String {
    match status {
        "healthy" => "Healthy".to_string(),
        "missing" => "Not installed".to_string(),
        "incompatible" => "Incompatible".to_string(),
        "unauthenticated" => "Sign-in needed".to_string(),
        _ => "Unhealthy".to_string(),
    }
}

/// Human description of a readiness reason, matching the React
/// `describeReadinessReason`.
pub fn describe_readiness_reason(reason: &alva_hub_gpui::ReadinessReason) -> String {
    use alva_hub_gpui::ReadinessReason::*;
    match reason {
        BinaryNotFound { searched } => {
            format!(
                "Not installed — searched {}.",
                if searched.is_empty() {
                    "PATH".to_string()
                } else {
                    searched.join(", ")
                }
            )
        }
        VersionUnreadable { detail } => format!("Version check failed: {detail}"),
        VersionIncompatible { found, minimum } => {
            format!("Version {found} is older than the required {minimum}.")
        }
        Unauthenticated { remedy } => remedy.clone(),
        ProviderKeyMissing { remedy } => remedy.clone(),
        StreamFailing { last_failure } => format!("Streaming is failing: {last_failure}"),
    }
}

/// Resource claim description, matching the React `describeClaim`.
#[allow(dead_code)]
pub fn describe_claim(claim: &alva_hub_gpui::ResourceClaim) -> String {
    use alva_hub_gpui::ResourceClaim::*;
    match claim {
        Path { mode, path } => format!("{} {path}", mode.as_str()),
        Facility { name } => format!("facility {name}"),
        ExclusiveTool { tool } => format!("tool {tool}"),
    }
}

pub mod actions;
pub mod app;
pub mod fallback;
pub mod readiness;
pub mod sidebar;
pub mod team;
pub mod timeline;
pub mod topbar;
