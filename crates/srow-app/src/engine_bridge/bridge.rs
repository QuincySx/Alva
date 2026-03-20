//! EngineBridge — adapter between UI layer and engine layer.
//!
//! In Sub-1 this uses a simple mock to simulate streaming replies.
//! In Sub-2, replace the mock call with `AgentEngine::run()`.

use gpui::{AppContext, Context, Entity};
use srow_engine::EngineEvent;

use crate::models::{AgentModel, ChatModel};
use crate::types::AgentStatusKind;

/// Bridges engine events into GPUI Model updates.
///
/// # Replace point for Sub-2
/// Swap the mock implementation inside `send_message` for real `AgentEngine::run`.
pub struct EngineBridge;

impl EngineBridge {
    /// Send a user message and start the mock engine loop.
    /// Engine events are collected in a background thread, then applied to models.
    pub fn send_message<V: 'static>(
        session_id: String,
        prompt: String,
        chat_model: Entity<ChatModel>,
        agent_model: Entity<AgentModel>,
        cx: &mut Context<V>,
    ) {
        // Mark agent as running
        agent_model.update(cx, |model, cx| {
            model.set_status(&session_id, AgentStatusKind::Running, cx);
        });

        let sid = session_id.clone();

        cx.spawn(async move |_this, cx| {
            // Run mock engine entirely in background, collecting all events
            let engine_sid = sid.clone();
            let engine_prompt = prompt.clone();

            let events: Vec<EngineEvent> = cx
                .background_spawn(async move {
                    let (event_tx, event_rx) = std::sync::mpsc::channel::<EngineEvent>();

                    // Run mock engine (blocking, in this background thread)
                    let handle = std::thread::spawn(move || {
                        run_mock_engine(&engine_sid, &engine_prompt, event_tx);
                    });

                    // Collect all events
                    let mut events = Vec::new();
                    while let Ok(event) = event_rx.recv() {
                        let is_terminal = matches!(
                            &event,
                            EngineEvent::Completed { .. } | EngineEvent::Error { .. }
                        );
                        events.push(event);
                        if is_terminal {
                            break;
                        }
                    }

                    let _ = handle.join();
                    events
                })
                .await;

            // Apply all events to models on the main thread
            let chat = chat_model.clone();
            let agent = agent_model.clone();
            let sid2 = sid.clone();

            cx.update(|cx| {
                // Build the full reply text from TextDelta events
                let mut reply_text = String::new();
                let mut completed = false;
                let mut errored = false;

                for event in &events {
                    match event {
                        EngineEvent::TextDelta { text, .. } => {
                            reply_text.push_str(text);
                        }
                        EngineEvent::Completed { .. } => {
                            completed = true;
                        }
                        EngineEvent::Error { .. } => {
                            errored = true;
                        }
                        _ => {}
                    }
                }

                // Append the full streaming buffer and immediately finalize
                if !reply_text.is_empty() {
                    chat.update(cx, |model, cx| {
                        model.append_text_delta(&sid2, &reply_text, cx);
                        model.finalize_stream(&sid2, cx);
                    });
                }

                if completed {
                    agent.update(cx, |model, cx| {
                        model.set_status(&sid2, AgentStatusKind::Idle, cx);
                    });
                } else if errored {
                    agent.update(cx, |model, cx| {
                        model.set_status(&sid2, AgentStatusKind::Error, cx);
                    });
                }
            })
            .ok();
        })
        .detach();
    }
}

/// Mock engine: simulates streaming reply character by character.
/// This runs in a standard thread (uses blocking sleep).
///
/// # Replace point for Sub-2
/// Replace with `AgentEngine::run()` call.
fn run_mock_engine(
    session_id: &str,
    prompt: &str,
    event_tx: std::sync::mpsc::Sender<EngineEvent>,
) {
    let reply = format!("Mock reply: I received your message -- \"{}\"", prompt);
    let sid = session_id.to_string();

    for ch in reply.chars() {
        std::thread::sleep(std::time::Duration::from_millis(25));
        let _ = event_tx.send(EngineEvent::TextDelta {
            session_id: sid.clone(),
            text: ch.to_string(),
        });
    }
    let _ = event_tx.send(EngineEvent::Completed {
        session_id: sid,
    });
}
