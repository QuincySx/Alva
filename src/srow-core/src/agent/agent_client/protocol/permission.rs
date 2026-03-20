use serde::{Deserialize, Serialize};

/// Permission request from external Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    /// Human-readable description ("I will execute rm -rf /tmp/xxx")
    pub description: String,
    /// Risk level
    pub risk_level: RiskLevel,
    /// Tool name
    pub tool_name: String,
    /// Tool input summary (displayed to user)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_input_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// Permission response from Srow back to external Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionData {
    pub option: PermissionOption,
    /// Optional: rejection reason (filled when option = reject_*)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Permission options (1:1 replication of Wukong's four options)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionOption {
    /// Allow this time
    AllowOnce,
    /// Allow permanently (stored in session_approval_memory)
    AllowAlways,
    /// Reject this time
    RejectOnce,
    /// Reject permanently (stored in session_approval_memory)
    RejectAlways,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_option_serde() {
        for (option, expected_str) in [
            (PermissionOption::AllowOnce, "allow_once"),
            (PermissionOption::AllowAlways, "allow_always"),
            (PermissionOption::RejectOnce, "reject_once"),
            (PermissionOption::RejectAlways, "reject_always"),
        ] {
            let json = serde_json::to_string(&option).unwrap();
            assert!(json.contains(expected_str), "expected {expected_str} in {json}");
            let deserialized: PermissionOption = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, option);
        }
    }

    #[test]
    fn test_risk_level_serde() {
        for (level, expected_str) in [
            (RiskLevel::Low, "low"),
            (RiskLevel::Medium, "medium"),
            (RiskLevel::High, "high"),
            (RiskLevel::Critical, "critical"),
        ] {
            let json = serde_json::to_string(&level).unwrap();
            assert!(json.contains(expected_str));
            let deserialized: RiskLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, level);
        }
    }

    #[test]
    fn test_permission_data_roundtrip() {
        let data = PermissionData {
            option: PermissionOption::AllowOnce,
            reason: None,
        };
        let json = serde_json::to_string(&data).unwrap();
        assert!(json.contains("allow_once"));
        assert!(!json.contains("reason")); // skip_serializing_if = None

        let data_with_reason = PermissionData {
            option: PermissionOption::RejectOnce,
            reason: Some("too dangerous".to_string()),
        };
        let json = serde_json::to_string(&data_with_reason).unwrap();
        assert!(json.contains("too dangerous"));
    }
}
