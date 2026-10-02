// INPUT: HubStore saved session/settings, native MCP stdio transport, Tokio bounded concurrency
// OUTPUT: probe_managed_mcp(), McpDiagnostic
// POS: Explicit user-authorized MCP connection test without model calls or persistent runtime mutation

use crate::HubStore;
use alva_protocol_mcp::{transport::McpTransport, transport_stdio::StdioTransport};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, Instant},
};

#[derive(Debug, Serialize)]
pub struct McpDiagnostic {
    pub server_id: String,
    pub revision: i64,
    pub ok: bool,
    pub tools: Vec<String>,
    pub latency_ms: u64,
    pub message: String,
}

pub async fn probe_managed_mcp(
    store: &HubStore,
    session: &str,
    id: &str,
    revision: i64,
    confirmed: bool,
) -> Result<McpDiagnostic, String> {
    if !confirmed {
        return Err(
            "Confirm that testing will execute this MCP program with your host access".into(),
        );
    }
    static SLOTS: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    let _permit = SLOTS
        .get_or_init(|| tokio::sync::Semaphore::new(2))
        .try_acquire()
        .map_err(|_| "MCP diagnostics busy; try again")?;
    let settings = store.tool_settings()?;
    if settings.revision != revision {
        return Err("Tool settings changed; refresh and review again".into());
    }
    let server = settings
        .mcp_servers
        .iter()
        .find(|s| s.id == id)
        .ok_or("MCP server not found")?;
    let workspace = store
        .get_session(session)?
        .ok_or("Saved session required for MCP testing")?
        .session
        .workspace;
    let mut transport =
        StdioTransport::new(server.runtime_config(), Some(PathBuf::from(workspace)));
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(server.connect_timeout_secs as u64),
        async {
            transport.connect().await?;
            transport.list_tools().await
        },
    )
    .await;
    let _ = transport.disconnect().await;
    let (ok, tools, message) = match result {
        Ok(Ok(tools)) => (
            true,
            tools.into_iter().map(|t| t.tool_name).collect(),
            "Connected and discovered tools; test process has been closed".into(),
        ),
        Ok(Err(error)) => (false, vec![], error.to_string()),
        Err(_) => (false, vec![], "MCP connection test timed out".into()),
    };
    Ok(McpDiagnostic {
        server_id: id.into(),
        revision,
        ok,
        tools,
        latency_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        message,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{EngineTarget, ManagedMcpServer, PermissionProfile, ToolSettingsAction};
    #[tokio::test]
    async fn saved_revision_authorizes_one_shot_discovery_without_enabling_or_changing_history() {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(data.path().join("hub.db")).unwrap();
        let turn = store
            .prepare_new_turn(
                "session",
                workspace.path(),
                "fixture",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::Interactive,
            )
            .unwrap();
        store
            .finish_turn(&turn, None, Some("done"), None, None)
            .unwrap();
        let script = r#"import sys,json,os
for line in sys.stdin:
 r=json.loads(line)
 if 'id' not in r: continue
 result={'protocolVersion':r['params']['protocolVersion'],'capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'}} if r['method']=='initialize' else {'tools':[{'name':'cwd','description':os.getcwd(),'inputSchema':{'type':'object'}}]}
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':result}),flush=True)
"#;
        let saved = store
            .tool_settings_action(ToolSettingsAction::CreateMcp {
                revision: 0,
                server: ManagedMcpServer {
                    id: "fixture".into(),
                    display_name: "Fixture".into(),
                    command: "/usr/bin/python3".into(),
                    args: vec!["-u".into(), "-c".into(), script.into()],
                    connect_timeout_secs: 5,
                    enabled: false,
                },
            })
            .unwrap();
        assert!(
            probe_managed_mcp(&store, "session", "fixture", saved.revision, false)
                .await
                .is_err()
        );
        assert!(
            probe_managed_mcp(&store, "missing", "fixture", saved.revision, true)
                .await
                .is_err()
        );
        assert!(probe_managed_mcp(&store, "session", "fixture", 0, true)
            .await
            .is_err());
        let result = probe_managed_mcp(&store, "session", "fixture", saved.revision, true)
            .await
            .unwrap();
        assert!(result.ok, "{}", result.message);
        assert_eq!(result.tools, vec!["cwd"]);
        assert!(!store.tool_settings().unwrap().mcp_servers[0].enabled);
        assert_eq!(
            store.get_session("session").unwrap().unwrap().turns.len(),
            1
        );
    }
}
