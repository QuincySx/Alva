// ACP special message types: execution plan steps and heartbeat ping/pong.
use serde::{Deserialize, Serialize};

/// Agent execution plan (step list, displayed to user)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanData {
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub index: u32,
    pub description: String,
    pub status: PlanStepStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

/// Heartbeat (bidirectional)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingPongData {
    pub id: String,
    pub timestamp_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_data_serde() {
        let plan = PlanData {
            steps: vec![
                PlanStep {
                    index: 0,
                    description: "Read file".to_string(),
                    status: PlanStepStatus::Done,
                },
                PlanStep {
                    index: 1,
                    description: "Modify code".to_string(),
                    status: PlanStepStatus::Running,
                },
                PlanStep {
                    index: 2,
                    description: "Run tests".to_string(),
                    status: PlanStepStatus::Pending,
                },
            ],
        };
        let json = serde_json::to_string(&plan).unwrap();
        let deserialized: PlanData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.steps.len(), 3);
        assert_eq!(deserialized.steps[0].status, PlanStepStatus::Done);
        assert_eq!(deserialized.steps[1].status, PlanStepStatus::Running);
    }

    #[test]
    fn test_plan_step_status_variants() {
        for (status, expected_str) in [
            (PlanStepStatus::Pending, "pending"),
            (PlanStepStatus::Running, "running"),
            (PlanStepStatus::Done, "done"),
            (PlanStepStatus::Failed, "failed"),
            (PlanStepStatus::Skipped, "skipped"),
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert!(json.contains(expected_str));
        }
    }
}
