use std::io;

use serde_json::json;

use crate::error::RuntimeError;
use crate::event::{PermissionDecision, RuntimeCapabilities, RuntimeEvent, RuntimeUsage};
use crate::request::{RuntimeOptions, RuntimeRequest};

// ---------------------------------------------------------------------------
// RuntimeRequest
// ---------------------------------------------------------------------------

#[test]
fn request_new_sets_prompt() {
    let req = RuntimeRequest::new("hello");
    assert_eq!(req.prompt, "hello");
    assert!(req.resume_session.is_none());
    assert!(req.system_prompt.is_none());
    assert!(req.working_directory.is_none());
}

#[test]
fn request_with_cwd() {
    let req = RuntimeRequest::new("test").with_cwd("/tmp");
    assert_eq!(
        req.working_directory.as_ref().unwrap().to_str().unwrap(),
        "/tmp"
    );
}

#[test]
fn request_with_streaming() {
    let req = RuntimeRequest::new("test").with_streaming();
    assert!(req.options.streaming);
}

#[test]
fn request_builder_chain() {
    let req = RuntimeRequest::new("prompt")
        .with_cwd("/home")
        .with_streaming();
    assert_eq!(req.prompt, "prompt");
    assert!(req.options.streaming);
    assert!(req.working_directory.is_some());
}

#[test]
fn request_field_access() {
    let mut req = RuntimeRequest::new("p");
    req.resume_session = Some("sess-1".into());
    req.system_prompt = Some("You are helpful".into());
    assert_eq!(req.resume_session.as_deref(), Some("sess-1"));
    assert_eq!(req.system_prompt.as_deref(), Some("You are helpful"));
}

// ---------------------------------------------------------------------------
// RuntimeOptions
// ---------------------------------------------------------------------------

#[test]
fn options_default() {
    let opts = RuntimeOptions::default();
    assert!(!opts.streaming);
    assert!(opts.max_turns.is_none());
    assert!(opts.extra.is_empty());
}

#[test]
fn options_extra_fields() {
    let mut opts = RuntimeOptions::default();
    opts.extra
        .insert("temperature".into(), json!(0.7));
    opts.extra
        .insert("model".into(), json!("claude-3"));
    assert_eq!(opts.extra.len(), 2);
    assert_eq!(opts.extra["temperature"], json!(0.7));
    assert_eq!(opts.extra["model"], json!("claude-3"));
}

#[test]
fn options_max_turns() {
    let mut opts = RuntimeOptions::default();
    opts.max_turns = Some(10);
    assert_eq!(opts.max_turns, Some(10));
}

// ---------------------------------------------------------------------------
// RuntimeEvent variant construction
// ---------------------------------------------------------------------------

#[test]
fn event_session_started() {
    let event = RuntimeEvent::SessionStarted {
        session_id: "s1".into(),
        model: Some("gpt-4".into()),
        tools: vec!["bash".into(), "read".into()],
    };
    if let RuntimeEvent::SessionStarted {
        session_id,
        model,
        tools,
    } = &event
    {
        assert_eq!(session_id, "s1");
        assert_eq!(model.as_deref(), Some("gpt-4"));
        assert_eq!(tools.len(), 2);
    } else {
        panic!("Expected SessionStarted");
    }
}

#[test]
fn event_completed_with_usage() {
    let usage = RuntimeUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_cost_usd: Some(0.01),
        duration_ms: 1500,
        num_turns: 3,
    };
    let event = RuntimeEvent::Completed {
        session_id: "s2".into(),
        result: Some("done".into()),
        usage: Some(usage),
    };
    if let RuntimeEvent::Completed {
        session_id,
        result,
        usage,
    } = &event
    {
        assert_eq!(session_id, "s2");
        assert_eq!(result.as_deref(), Some("done"));
        let u = usage.as_ref().unwrap();
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
        assert_eq!(u.num_turns, 3);
    } else {
        panic!("Expected Completed");
    }
}

#[test]
fn event_completed_without_result() {
    let event = RuntimeEvent::Completed {
        session_id: "s3".into(),
        result: None,
        usage: None,
    };
    if let RuntimeEvent::Completed { result, usage, .. } = &event {
        assert!(result.is_none());
        assert!(usage.is_none());
    } else {
        panic!("Expected Completed");
    }
}

#[test]
fn event_error_recoverable() {
    let event = RuntimeEvent::Error {
        message: "rate limit".into(),
        recoverable: true,
    };
    if let RuntimeEvent::Error {
        message,
        recoverable,
    } = &event
    {
        assert_eq!(message, "rate limit");
        assert!(*recoverable);
    } else {
        panic!("Expected Error");
    }
}

#[test]
fn event_error_fatal() {
    let event = RuntimeEvent::Error {
        message: "crash".into(),
        recoverable: false,
    };
    if let RuntimeEvent::Error { recoverable, .. } = &event {
        assert!(!*recoverable);
    } else {
        panic!("Expected Error");
    }
}

#[test]
fn event_tool_start() {
    let event = RuntimeEvent::ToolStart {
        id: "t1".into(),
        name: "bash".into(),
        input: json!({"command": "ls"}),
    };
    if let RuntimeEvent::ToolStart { id, name, input } = &event {
        assert_eq!(id, "t1");
        assert_eq!(name, "bash");
        assert_eq!(input["command"], "ls");
    } else {
        panic!("Expected ToolStart");
    }
}

#[test]
fn event_tool_end() {
    let output = alva_types::ToolOutput::text("file.txt");
    let event = RuntimeEvent::ToolEnd {
        id: "t1".into(),
        name: "bash".into(),
        result: output,
        duration_ms: Some(42),
    };
    if let RuntimeEvent::ToolEnd {
        id, duration_ms, ..
    } = &event
    {
        assert_eq!(id, "t1");
        assert_eq!(*duration_ms, Some(42));
    } else {
        panic!("Expected ToolEnd");
    }
}

#[test]
fn event_permission_request() {
    let event = RuntimeEvent::PermissionRequest {
        request_id: "r1".into(),
        tool_name: "bash".into(),
        tool_input: json!({"command": "rm -rf /"}),
        description: Some("dangerous command".into()),
    };
    if let RuntimeEvent::PermissionRequest {
        request_id,
        tool_name,
        description,
        ..
    } = &event
    {
        assert_eq!(request_id, "r1");
        assert_eq!(tool_name, "bash");
        assert_eq!(description.as_deref(), Some("dangerous command"));
    } else {
        panic!("Expected PermissionRequest");
    }
}

// ---------------------------------------------------------------------------
// RuntimeCapabilities
// ---------------------------------------------------------------------------

#[test]
fn capabilities_construction() {
    let caps = RuntimeCapabilities {
        streaming: true,
        tool_control: false,
        permission_callback: true,
        resume: false,
        cancel: true,
    };
    assert!(caps.streaming);
    assert!(!caps.tool_control);
    assert!(caps.permission_callback);
    assert!(!caps.resume);
    assert!(caps.cancel);
}

#[test]
fn capabilities_all_false() {
    let caps = RuntimeCapabilities {
        streaming: false,
        tool_control: false,
        permission_callback: false,
        resume: false,
        cancel: false,
    };
    assert!(!caps.streaming);
    assert!(!caps.tool_control);
    assert!(!caps.permission_callback);
    assert!(!caps.resume);
    assert!(!caps.cancel);
}

// ---------------------------------------------------------------------------
// RuntimeUsage
// ---------------------------------------------------------------------------

#[test]
fn usage_default() {
    let u = RuntimeUsage::default();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert!(u.total_cost_usd.is_none());
    assert_eq!(u.duration_ms, 0);
    assert_eq!(u.num_turns, 0);
}

#[test]
fn usage_construction() {
    let u = RuntimeUsage {
        input_tokens: 500,
        output_tokens: 200,
        total_cost_usd: Some(0.05),
        duration_ms: 3000,
        num_turns: 5,
    };
    assert_eq!(u.input_tokens, 500);
    assert_eq!(u.output_tokens, 200);
    assert_eq!(u.total_cost_usd, Some(0.05));
    assert_eq!(u.duration_ms, 3000);
    assert_eq!(u.num_turns, 5);
}

// ---------------------------------------------------------------------------
// PermissionDecision
// ---------------------------------------------------------------------------

#[test]
fn permission_decision_allow_no_update() {
    let d = PermissionDecision::Allow {
        updated_input: None,
    };
    if let PermissionDecision::Allow { updated_input } = &d {
        assert!(updated_input.is_none());
    } else {
        panic!("Expected Allow");
    }
}

#[test]
fn permission_decision_allow_with_update() {
    let d = PermissionDecision::Allow {
        updated_input: Some(json!({"command": "ls"})),
    };
    if let PermissionDecision::Allow { updated_input } = &d {
        assert_eq!(updated_input.as_ref().unwrap()["command"], "ls");
    } else {
        panic!("Expected Allow");
    }
}

#[test]
fn permission_decision_deny() {
    let d = PermissionDecision::Deny {
        message: "not allowed".into(),
    };
    if let PermissionDecision::Deny { message } = &d {
        assert_eq!(message, "not allowed");
    } else {
        panic!("Expected Deny");
    }
}

// ---------------------------------------------------------------------------
// RuntimeError display
// ---------------------------------------------------------------------------

#[test]
fn error_not_ready_display() {
    let e = RuntimeError::NotReady("engine starting".into());
    assert_eq!(e.to_string(), "Engine not ready: engine starting");
}

#[test]
fn error_unsupported_display() {
    let e = RuntimeError::Unsupported("streaming".into());
    assert_eq!(e.to_string(), "Unsupported: streaming");
}

#[test]
fn error_session_not_found_display() {
    let e = RuntimeError::SessionNotFound("abc-123".into());
    assert_eq!(e.to_string(), "Session not found: abc-123");
}

#[test]
fn error_permission_not_found_display() {
    let e = RuntimeError::PermissionNotFound("req-1".into());
    assert_eq!(e.to_string(), "Permission request not found: req-1");
}

#[test]
fn error_process_error_display() {
    let e = RuntimeError::ProcessError("spawn failed".into());
    assert_eq!(e.to_string(), "Process error: spawn failed");
}

#[test]
fn error_protocol_error_display() {
    let e = RuntimeError::ProtocolError("invalid json".into());
    assert_eq!(e.to_string(), "Protocol error: invalid json");
}

#[test]
fn error_cancelled_display() {
    let e = RuntimeError::Cancelled;
    assert_eq!(e.to_string(), "Cancelled");
}

#[test]
fn error_other_display() {
    let e = RuntimeError::Other("something went wrong".into());
    assert_eq!(e.to_string(), "something went wrong");
}

#[test]
fn error_from_io() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    let e: RuntimeError = io_err.into();
    assert!(matches!(e, RuntimeError::ProcessError(_)));
    assert!(e.to_string().contains("file not found"));
}

#[test]
fn error_from_serde_json() {
    // Trigger a serde_json error by parsing invalid JSON.
    let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
    let e: RuntimeError = json_err.into();
    assert!(matches!(e, RuntimeError::ProtocolError(_)));
}
