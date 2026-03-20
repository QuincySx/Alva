//! ACP integration tests using echo_agent.py

use std::path::PathBuf;

use srow_engine::adapters::acp::{
    protocol::{
        bootstrap::{BootstrapPayload, ModelConfig, SandboxLevel},
        message::{AcpInboundMessage, AcpOutboundMessage},
    },
    process::{
        discovery::{AgentCliCommand, ExternalAgentKind},
        handle::AcpProcessHandle,
    },
};
use tokio::sync::mpsc;

/// Helper: path to the echo_agent.py fixture
fn echo_agent_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("tests").join("fixtures").join("echo_agent.py")
}

/// Helper: build a test bootstrap payload
fn test_bootstrap() -> BootstrapPayload {
    BootstrapPayload {
        workspace: "/tmp/srow-test".to_string(),
        authorized_roots: vec!["/tmp/srow-test".to_string()],
        sandbox_level: SandboxLevel::None,
        model_config: ModelConfig {
            provider: "test".to_string(),
            model: "echo".to_string(),
            api_key: "sk-test".to_string(),
            base_url: None,
            max_tokens: None,
        },
        attachment_paths: vec![],
        srow_version: "0.1.0-test".to_string(),
    }
}

/// Helper: build AgentCliCommand pointing to python3 echo_agent.py
fn echo_agent_cmd() -> AgentCliCommand {
    AgentCliCommand {
        kind: ExternalAgentKind::Generic {
            command: "echo_agent".to_string(),
        },
        executable: PathBuf::from("python3"),
        args: vec![echo_agent_path().to_string_lossy().to_string()],
    }
}

/// Test: spawn echo agent, send bootstrap + prompt, verify task_start -> session_update -> task_complete
#[tokio::test]
async fn test_acp_echo_agent_full_lifecycle() {
    let (inbound_tx, mut inbound_rx) = mpsc::channel::<AcpInboundMessage>(64);

    let handle = AcpProcessHandle::spawn(&echo_agent_cmd(), test_bootstrap(), inbound_tx)
        .await
        .expect("failed to spawn echo agent");

    assert!(handle.pid > 0);

    // Send prompt
    handle
        .send(AcpOutboundMessage::Prompt {
            content: "Hello, echo agent!".to_string(),
            resume: None,
        })
        .await
        .expect("failed to send prompt");

    // Collect messages until task_complete
    let mut received_task_start = false;
    let mut received_session_update = false;
    let mut received_task_complete = false;
    let mut echo_text = String::new();

    let timeout = tokio::time::Duration::from_secs(10);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        match tokio::time::timeout_at(deadline, inbound_rx.recv()).await {
            Ok(Some(msg)) => match msg {
                AcpInboundMessage::TaskStart { data } => {
                    assert_eq!(data.task_id, "echo-task-1");
                    received_task_start = true;
                }
                AcpInboundMessage::SessionUpdate { content, .. } => {
                    for block in content {
                        if let srow_engine::adapters::acp::protocol::content::ContentBlock::Text {
                            text,
                            ..
                        } = block
                        {
                            echo_text.push_str(&text);
                        }
                    }
                    received_session_update = true;
                }
                AcpInboundMessage::TaskComplete { data } => {
                    assert_eq!(
                        data.finish_reason,
                        srow_engine::adapters::acp::protocol::lifecycle::TaskFinishReason::Complete
                    );
                    received_task_complete = true;
                    break;
                }
                other => {
                    // Log unexpected messages but don't fail
                    eprintln!("unexpected message: {:?}", other);
                }
            },
            Ok(None) => {
                panic!("inbound channel closed unexpectedly");
            }
            Err(_) => {
                panic!("timed out waiting for echo agent response");
            }
        }
    }

    assert!(received_task_start, "did not receive task_start");
    assert!(received_session_update, "did not receive session_update");
    assert!(received_task_complete, "did not receive task_complete");
    assert_eq!(echo_text, "Hello, echo agent!");

    // Graceful shutdown
    handle.shutdown().await;
}

/// Test: spawn echo agent and send shutdown without prompt
#[tokio::test]
async fn test_acp_echo_agent_immediate_shutdown() {
    let (inbound_tx, _inbound_rx) = mpsc::channel::<AcpInboundMessage>(64);

    let handle = AcpProcessHandle::spawn(&echo_agent_cmd(), test_bootstrap(), inbound_tx)
        .await
        .expect("failed to spawn echo agent");

    // Immediately shutdown
    handle.shutdown().await;

    // Wait for process to exit (poll with retries, process cleanup is async)
    let mut final_state = handle.state().await;
    for _ in 0..20 {
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        final_state = handle.state().await;
        if !matches!(
            final_state,
            srow_engine::adapters::acp::process::handle::ProcessState::Running
        ) {
            break;
        }
    }

    assert!(
        matches!(
            final_state,
            srow_engine::adapters::acp::process::handle::ProcessState::Exited
                | srow_engine::adapters::acp::process::handle::ProcessState::Crashed { .. }
        ),
        "expected Exited or Crashed, got {:?}",
        final_state
    );
}

/// Test: AcpSession state machine transitions via echo agent
#[tokio::test]
async fn test_acp_session_state_transitions() {

    // We need to spawn via a Generic agent that points to our echo_agent.py
    // But AcpProcessManager uses AgentDiscovery which does PATH lookup...
    // Instead, test at the lower level using AcpProcessHandle directly + AcpSession

    let (inbound_tx, mut inbound_rx) = mpsc::channel::<AcpInboundMessage>(64);
    let handle = AcpProcessHandle::spawn(&echo_agent_cmd(), test_bootstrap(), inbound_tx)
        .await
        .expect("failed to spawn echo agent");

    // We'll create a minimal process manager wrapper to test session
    // For now, just test that we can receive and route messages correctly

    // Send prompt directly
    handle
        .send(AcpOutboundMessage::Prompt {
            content: "State machine test".to_string(),
            resume: None,
        })
        .await
        .expect("failed to send prompt");

    // Collect all messages
    let mut messages = vec![];
    let timeout = tokio::time::Duration::from_secs(10);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        match tokio::time::timeout_at(deadline, inbound_rx.recv()).await {
            Ok(Some(msg)) => {
                let is_complete = matches!(msg, AcpInboundMessage::TaskComplete { .. });
                messages.push(msg);
                if is_complete {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => panic!("timed out"),
        }
    }

    assert!(messages.len() >= 3, "expected at least 3 messages (task_start, session_update, task_complete), got {}", messages.len());

    // Verify message order
    assert!(matches!(messages[0], AcpInboundMessage::TaskStart { .. }));
    assert!(matches!(messages[1], AcpInboundMessage::SessionUpdate { .. }));
    assert!(matches!(messages[2], AcpInboundMessage::TaskComplete { .. }));

    handle.shutdown().await;
}
