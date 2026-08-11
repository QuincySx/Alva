// INPUT:  async_trait, futures stream, serde, SessionRecord, RuntimeError
// OUTPUT: Optional EngineInspector capability, snapshots/live updates, JSONL bundle encoding
// POS:    Inspection-only engine contract kept separate from execution-focused EngineRuntime

use std::pin::Pin;

use alva_kernel_abi::SessionRecord;
use async_trait::async_trait;
use futures_core::Stream;
use serde::{Deserialize, Serialize};

use crate::RuntimeError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InspectionLevel {
    Full,
    Summary,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InspectionCapabilities {
    pub level: InspectionLevel,
    pub historical: bool,
    pub live: bool,
    pub raw_records: bool,
    pub recursive_children: bool,
    pub export_jsonl_bundle: bool,
}

impl InspectionCapabilities {
    pub const fn full() -> Self {
        Self {
            level: InspectionLevel::Full,
            historical: true,
            live: true,
            raw_records: true,
            recursive_children: true,
            export_jsonl_bundle: true,
        }
    }

    pub const fn unsupported() -> Self {
        Self {
            level: InspectionLevel::Unsupported,
            historical: false,
            live: false,
            raw_records: false,
            recursive_children: false,
            export_jsonl_bundle: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChildSessionLink {
    pub child_session_id: String,
    pub parent_session_id: String,
    pub parent_run_id: String,
    pub spawn_tool_call_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionSnapshot {
    pub engine_id: String,
    pub native_session_id: String,
    pub parent_session_id: Option<String>,
    pub high_water_seq: u64,
    pub records: Vec<SessionRecord>,
    pub children: Vec<ChildSessionLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "update", rename_all = "snake_case")]
pub enum InspectionUpdate {
    Record { record: Box<SessionRecord> },
    ChildLinked { link: ChildSessionLink },
}

pub type InspectionStream = Pin<Box<dyn Stream<Item = InspectionUpdate> + Send>>;

#[async_trait]
pub trait EngineInspector: Send + Sync {
    fn inspection_capabilities(&self) -> InspectionCapabilities;

    async fn snapshot(&self, native_session_id: &str) -> Result<InspectionSnapshot, RuntimeError>;

    async fn subscribe(
        &self,
        native_session_id: &str,
        after_seq: u64,
    ) -> Result<InspectionStream, RuntimeError>;

    async fn delete_session(&self, native_session_id: &str) -> Result<(), RuntimeError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectorBundleManifest {
    pub format: String,
    pub version: u32,
    pub root_session_id: String,
    pub session_count: usize,
}

pub fn encode_jsonl_bundle(
    root_session_id: &str,
    snapshots: &[InspectionSnapshot],
) -> Result<Vec<u8>, RuntimeError> {
    let manifest = InspectorBundleManifest {
        format: "alva-inspector-jsonl-bundle".to_string(),
        version: 1,
        root_session_id: root_session_id.to_string(),
        session_count: snapshots.len(),
    };
    let mut bytes = serde_json::to_vec(&serde_json::json!({
        "kind": "manifest",
        "value": manifest,
    }))
    .map_err(|error| RuntimeError::ProtocolError(error.to_string()))?;
    bytes.push(b'\n');
    for snapshot in snapshots {
        let line = serde_json::to_vec(&serde_json::json!({
            "kind": "session",
            "value": snapshot,
        }))
        .map_err(|error| RuntimeError::ProtocolError(error.to_string()))?;
        bytes.extend(line);
        bytes.push(b'\n');
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_starts_with_versioned_manifest() {
        let bytes = encode_jsonl_bundle("root", &[]).unwrap();
        let first = std::str::from_utf8(&bytes).unwrap().lines().next().unwrap();
        let value: serde_json::Value = serde_json::from_str(first).unwrap();
        assert_eq!(value["kind"], "manifest");
        assert_eq!(value["value"]["version"], 1);
    }
}
