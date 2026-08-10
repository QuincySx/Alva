// INPUT:  hub-gpui backend DTOs (TimelineEvent, HandoffPackage, ...)
// OUTPUT: UI-projected event model shared by timeline / approval rendering
// POS:    Mirrors the old React `ProjectedEvent` shape so the GPUI port can
//         render the same cards 1:1. Pure value types, no gpui imports.

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

/// One dispatch target: value is the worker id, label is "To worker on …".
#[derive(Clone, PartialEq)]
pub struct WorkerOption {
    pub worker_id: String,
    pub label: String,
}

impl gpui_component::searchable_list::SearchableListItem for WorkerOption {
    type Value = String;

    fn title(&self) -> gpui::SharedString {
        self.label.clone().into()
    }

    fn value(&self) -> &Self::Value {
        &self.worker_id
    }
}

pub mod actions;
pub mod app;
pub mod approval;
pub mod details;
pub mod fallback;
pub mod pump;
pub mod readiness;
pub mod sidebar;
pub mod team;
pub mod team_actions;
pub mod timeline;
pub mod topbar;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_time_labels() {
        let now = 1_800_000_000_000;
        assert_eq!(format_relative_time(now - 20_000, now), "just now");
        assert_eq!(format_relative_time(now - 300_000, now), "5m ago");
        assert_eq!(format_relative_time(now - 10_800_000, now), "3h ago");
        assert_eq!(format_relative_time(now - 172_800_000, now), "2d ago");
        assert_eq!(format_relative_time(now - 2_592_000_000, now), "2026-12-16");
    }

    #[test]
    fn readiness_badge_labels() {
        assert_eq!(readiness_badge("healthy"), "Healthy");
        assert_eq!(readiness_badge("missing"), "Not installed");
        assert_eq!(readiness_badge("incompatible"), "Incompatible");
        assert_eq!(readiness_badge("unauthenticated"), "Sign-in needed");
        assert_eq!(readiness_badge("unhealthy"), "Unhealthy");
    }

    #[test]
    fn profile_labels() {
        assert_eq!(profile_label("interactive"), "Ask before risky actions");
        assert_eq!(profile_label("workspace_write"), "Workspace writes allowed");
        assert_eq!(profile_label("unrestricted"), "Everything allowed");
    }

    #[test]
    fn claim_descriptions() {
        use alva_hub_gpui::{ClaimMode, ResourceClaim};
        assert_eq!(
            describe_claim(&ResourceClaim::Path {
                path: "src/lib.rs".into(),
                mode: ClaimMode::Write,
            }),
            "write src/lib.rs"
        );
        assert_eq!(
            describe_claim(&ResourceClaim::Facility {
                name: "target-dir".into()
            }),
            "facility target-dir"
        );
        assert_eq!(
            describe_claim(&ResourceClaim::ExclusiveTool { tool: "git".into() }),
            "tool git"
        );
    }

    #[test]
    fn worker_option_value_is_worker_id() {
        let option = WorkerOption {
            worker_id: "w-1".into(),
            label: "To worker on codex".into(),
        };
        use gpui_component::searchable_list::SearchableListItem;
        assert_eq!(option.value(), "w-1");
        assert_eq!(option.title().to_string(), "To worker on codex");
    }
}
