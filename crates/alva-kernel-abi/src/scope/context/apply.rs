// INPUT:  alva_kernel_abi::scope::context::*, alva_kernel_abi::AgentMessage, alva_kernel_abi::Message
// OUTPUT: pub fn apply_injections, pub async fn apply_compressions
// POS:    Runtime helpers for applying context hook results to agent state.
//         Moved from alva_kernel_abi::context::apply — types crate should not contain runtime logic.

use crate::base::content::ContentBlock;
use crate::base::message::{Message, MessageRole};
use crate::scope::context::{
    CompressAction, ContextHandle, Injection, InjectionContent, MessageSelector,
};
use crate::AgentMessage;
use tracing::debug;

/// Apply a list of Injections to the layered system prompt and message
/// list.
///
/// Each entry in `system_prompt` is a "segment". Convention used by
/// the kernel:
///   - **All entries except the last** are stable / cacheable (Layer
///     L0 / L1 / L3 / Memory contributions live here).
///   - **The last entry** is the dynamic / volatile bucket (Layer L2
///     RuntimeInject — date, git status, fresh tool results).
///
/// Routing:
///   - `Memory` / `Skill` / `SystemPrompt` injections → appended to
///     the **stable** bucket (the second-to-last entry, or a fresh
///     stable entry if the vec was all-dynamic).
///   - `RuntimeContext` injection → appended to the **dynamic**
///     bucket (the last entry, creating it if necessary).
///   - `Message` → unchanged, pushed onto `messages`.
///
/// This preserves the long-stable prefix that prompt-cache providers
/// (Anthropic, OpenAI auto-prefix) need to hit cache reliably.
pub fn apply_injections(
    injections: Vec<Injection>,
    system_prompt: &mut Vec<String>,
    messages: &mut Vec<AgentMessage>,
) {
    /// Get a mutable reference to the dynamic (last) segment, creating
    /// an empty one if the prompt vec is empty or contains only stable
    /// content (we conservatively treat the existing trailing entry as
    /// stable when only one entry exists — caller's responsibility to
    /// already split via `assemble_system_prompt`).
    fn dynamic_seg(buf: &mut Vec<String>) -> &mut String {
        if buf.is_empty() {
            buf.push(String::new());
        }
        buf.last_mut().unwrap()
    }

    /// Mutable ref to the stable bucket — second-to-last entry. If the
    /// prompt has only one entry (all-stable so far), we append in
    /// place; if it has zero, we create one.
    fn stable_seg(buf: &mut Vec<String>) -> &mut String {
        if buf.is_empty() {
            buf.push(String::new());
            return buf.first_mut().unwrap();
        }
        if buf.len() == 1 {
            return buf.first_mut().unwrap();
        }
        // 2+ entries: last is dynamic, second-to-last is stable bucket.
        let idx = buf.len() - 2;
        &mut buf[idx]
    }

    for injection in injections {
        match injection.content {
            InjectionContent::Memory(facts) => {
                if !facts.is_empty() {
                    let text = facts
                        .iter()
                        .map(|f| format!("- {}", f.text))
                        .collect::<Vec<_>>()
                        .join("\n");
                    stable_seg(system_prompt).push_str(&format!(
                        "\n\n<user_memory>\n{}\n</user_memory>",
                        text
                    ));
                }
            }
            InjectionContent::Skill { name, content } => {
                stable_seg(system_prompt).push_str(&format!(
                    "\n\n<skill name=\"{}\">\n{}\n</skill>",
                    name, content
                ));
            }
            InjectionContent::RuntimeContext(data) => {
                dynamic_seg(system_prompt)
                    .push_str(&format!("\n\n<runtime>\n{}\n</runtime>", data));
            }
            InjectionContent::Message(msg) => {
                messages.push(msg);
            }
            InjectionContent::SystemPrompt(section) => {
                // PromptSection doesn't carry a layer (it's named L0
                // by definition), so route to the stable bucket.
                let target = stable_seg(system_prompt);
                target.push_str("\n\n");
                target.push_str(&section.content);
            }
        }
    }
}

/// Apply a list of CompressActions to messages.
///
/// Handles SlidingWindow, ReplaceToolResult, Summarize, RemoveByPriority, Externalize.
/// Summarize calls `handle.summarize()` with a 5-second timeout.
pub async fn apply_compressions(
    actions: Vec<CompressAction>,
    messages: &mut Vec<AgentMessage>,
    handle: &dyn ContextHandle,
    session_id: &str,
) {
    for action in actions {
        match action {
            CompressAction::SlidingWindow { keep_recent } => {
                if messages.len() > keep_recent {
                    let drop_count = messages.len() - keep_recent;
                    messages.drain(..drop_count);
                    debug!(drop_count, keep_recent, "budget: applied sliding window");
                }
            }
            CompressAction::RemoveByPriority { .. } => {
                // Will activate once ContextStore holds real entries.
            }
            CompressAction::ReplaceToolResult {
                message_id,
                summary,
            } => {
                for msg in messages.iter_mut() {
                    if let AgentMessage::Standard(m) = msg {
                        if m.id == message_id {
                            m.content = vec![ContentBlock::Text {
                                text: summary.clone(),
                            }];
                            break;
                        }
                    }
                }
            }
            CompressAction::Summarize { range, hints } => {
                let msg_len = messages.len();
                let from_idx = resolve_selector(&range.from, messages, 0);
                let to_idx = resolve_selector(&range.to, messages, msg_len);

                if from_idx < to_idx && to_idx <= msg_len {
                    // The kernel does not enforce a timeout here — that is the
                    // ContextHandle implementation's responsibility. A real impl
                    // (e.g., LLM-backed) should bound its own latency; a noop or
                    // synchronous impl returns immediately. This keeps kernel-abi
                    // free of any tokio::time dependency.
                    let _range_text =
                        serialize_range(&messages[from_idx..to_idx], from_idx);
                    let summary_text =
                        handle.summarize(session_id, range.clone(), &hints).await;

                    let summary_msg = AgentMessage::Standard(Message {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: MessageRole::User,
                        content: vec![ContentBlock::Text {
                            text: format!(
                                "<conversation_summary>\n{}\n</conversation_summary>",
                                summary_text
                            ),
                        }],
                        tool_call_id: None,
                        usage: None,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    });

                    messages.drain(from_idx..to_idx);
                    messages.insert(from_idx, summary_msg);
                    debug!(
                        from = from_idx,
                        to = to_idx,
                        "budget: summarized {} messages",
                        to_idx - from_idx
                    );
                }
            }
            CompressAction::Externalize { .. } => {
                debug!("budget: externalize action not yet implemented");
            }
        }
    }
}

fn resolve_selector(
    selector: &MessageSelector,
    messages: &[AgentMessage],
    default: usize,
) -> usize {
    let len = messages.len();
    match selector {
        MessageSelector::FromStart => 0,
        MessageSelector::ToEnd => len,
        MessageSelector::ByIndex(i) => (*i).min(len),
        MessageSelector::ById(id) => messages
            .iter()
            .position(|m| match m {
                AgentMessage::Standard(msg) => msg.id == *id,
                _ => false,
            })
            .unwrap_or(default),
    }
}

fn serialize_range(messages: &[AgentMessage], base_idx: usize) -> String {
    messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let role = match m {
                AgentMessage::Standard(msg) => format!("{:?}", msg.role),
                AgentMessage::Extension { .. } => "Extension".to_string(),
                _ => "Other".to_string(),
            };
            let text = match m {
                AgentMessage::Standard(msg) => msg.text_content(),
                AgentMessage::Extension { data, .. } => data.to_string(),
                _ => String::new(),
            };
            let truncated = if text.len() > 2000 {
                // UTF-8 safe truncation — back off to the previous char
                // boundary so a multi-byte char straddling byte 2000
                // (emoji, CJK, etc.) doesn't panic on `&text[..2000]`.
                let mut end = 2000;
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...[truncated]", &text[..end])
            } else {
                text
            };
            format!("[{}] {}: {}", base_idx + i, role, truncated)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    //! Pure-logic tests for `apply.rs`.
    //!
    //! Covers the three sync, no-I/O functions: `apply_injections`,
    //! `resolve_selector`, `serialize_range`. The bucket-split helpers
    //! (`stable_seg` / `dynamic_seg`, which are nested fn's inside
    //! `apply_injections`) are exercised indirectly via the routing
    //! tests at the top.
    //!
    //! `apply_compressions` is covered at the bottom — it uses
    //! `NoopContextHandle` from `noop.rs` as the handle, since only the
    //! Summarize variant actually invokes `handle.summarize()` and that
    //! noop returns a known string we can assert on.
    use super::*;
    use crate::scope::context::{
        CompressAction, ContextLayer, Injection, InjectionContent, MemoryCategory, MemoryFact,
        MessageRange, MessageSelector, NoopContextHandle, PromptSection,
    };
    use crate::base::message::{Message, MessageRole};
    use crate::AgentMessage;

    fn user_msg(text: &str) -> AgentMessage {
        AgentMessage::Standard(Message::user(text))
    }

    fn memory_fact(text: &str) -> MemoryFact {
        MemoryFact {
            id: "id".to_string(),
            text: text.to_string(),
            fingerprint: "fp".to_string(),
            confidence: 1.0,
            category: MemoryCategory::UserPreference,
            source_session: "sess".to_string(),
            created_at: 0,
            last_accessed_at: 0,
            access_count: 0,
        }
    }

    // -- apply_injections: bucket routing ----------------------------------

    #[test]
    fn memory_injection_appends_to_stable_bucket_when_split_exists() {
        // 2-entry vec means [0]=stable, [1]=dynamic per the convention.
        let mut sys = vec!["STABLE".to_string(), "DYNAMIC".to_string()];
        let mut msgs = vec![];
        let inj = Injection {
            content: InjectionContent::Memory(vec![
                memory_fact("user prefers Rust"),
                memory_fact("uses macOS"),
            ]),
            layer: ContextLayer::Memory,
            priority: None,
        };
        apply_injections(vec![inj], &mut sys, &mut msgs);

        assert!(sys[0].contains("<user_memory>"), "stable bucket should get memory: {:?}", sys);
        assert!(sys[0].contains("- user prefers Rust"));
        assert!(sys[0].contains("- uses macOS"));
        assert_eq!(sys[1], "DYNAMIC", "dynamic bucket must be untouched");
        assert!(msgs.is_empty());
    }

    #[test]
    fn empty_memory_facts_emit_nothing() {
        // Guard at line 70: `if !facts.is_empty()` should skip insertion.
        let mut sys = vec!["STABLE".to_string(), "DYNAMIC".to_string()];
        let mut msgs = vec![];
        apply_injections(
            vec![Injection {
                content: InjectionContent::Memory(vec![]),
                layer: ContextLayer::Memory,
                priority: None,
            }],
            &mut sys,
            &mut msgs,
        );
        assert_eq!(sys[0], "STABLE", "no facts → no append");
        assert_eq!(sys[1], "DYNAMIC");
    }

    #[test]
    fn skill_injection_appends_to_stable_bucket() {
        let mut sys = vec!["STABLE".to_string(), "DYNAMIC".to_string()];
        let mut msgs = vec![];
        apply_injections(
            vec![Injection::skill("git".to_string(), "use rebase".to_string())],
            &mut sys,
            &mut msgs,
        );
        assert!(sys[0].contains("<skill name=\"git\">"));
        assert!(sys[0].contains("use rebase"));
        assert_eq!(sys[1], "DYNAMIC");
    }

    #[test]
    fn system_prompt_injection_appends_to_stable_bucket() {
        let mut sys = vec!["STABLE".to_string(), "DYNAMIC".to_string()];
        let mut msgs = vec![];
        let section = PromptSection {
            id: "identity".to_string(),
            content: "You are Claude.".to_string(),
            priority: crate::scope::context::Priority::Critical,
        };
        apply_injections(
            vec![Injection::system_prompt(section)],
            &mut sys,
            &mut msgs,
        );
        assert!(sys[0].ends_with("You are Claude."));
        assert_eq!(sys[1], "DYNAMIC");
    }

    #[test]
    fn runtime_context_injection_appends_to_dynamic_bucket() {
        let mut sys = vec!["STABLE".to_string(), "DYNAMIC".to_string()];
        let mut msgs = vec![];
        apply_injections(
            vec![Injection::runtime_context("date=2026-05-17".to_string())],
            &mut sys,
            &mut msgs,
        );
        assert_eq!(sys[0], "STABLE", "stable bucket must not be touched");
        assert!(sys[1].contains("<runtime>"));
        assert!(sys[1].contains("date=2026-05-17"));
    }

    #[test]
    fn message_injection_pushes_to_messages_not_prompt() {
        let mut sys = vec!["STABLE".to_string(), "DYNAMIC".to_string()];
        let mut msgs = vec![];
        apply_injections(
            vec![Injection::message(user_msg("hello"))],
            &mut sys,
            &mut msgs,
        );
        assert_eq!(sys[0], "STABLE", "prompt vec must be untouched");
        assert_eq!(sys[1], "DYNAMIC");
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn empty_prompt_vec_grows_for_both_buckets() {
        // stable_seg on empty vec pushes ONE entry (becomes the stable
        // bucket itself, len==1 branch). dynamic_seg on empty also
        // pushes one. So Memory + Runtime both into empty vec end up
        // sharing the same (only) entry per the `len()==1` rule.
        let mut sys: Vec<String> = vec![];
        let mut msgs = vec![];
        apply_injections(
            vec![
                Injection {
                    content: InjectionContent::Memory(vec![memory_fact("fact1")]),
                    layer: ContextLayer::Memory,
                    priority: None,
                },
                Injection::runtime_context("ctx1".to_string()),
            ],
            &mut sys,
            &mut msgs,
        );
        // After Memory: sys.len() becomes 1 (stable_seg pushed). After
        // Runtime: dynamic_seg sees len==1 → returns last, which is the
        // same entry. So both blobs land in sys[0].
        assert_eq!(sys.len(), 1, "single shared entry expected on empty start");
        assert!(sys[0].contains("<user_memory>"));
        assert!(sys[0].contains("<runtime>"));
        assert!(msgs.is_empty());
    }

    #[test]
    fn mixed_injections_preserve_bucket_separation_in_split_vec() {
        // The whole point of the split: with a proper 2-entry vec,
        // stable additions stay in [0] and dynamic in [1], protecting
        // prompt-cache hits.
        let mut sys = vec!["BASE_STABLE".to_string(), "BASE_DYN".to_string()];
        let mut msgs = vec![];
        apply_injections(
            vec![
                Injection {
                    content: InjectionContent::Memory(vec![memory_fact("M")]),
                    layer: ContextLayer::Memory,
                    priority: None,
                },
                Injection::skill("s".to_string(), "S".to_string()),
                Injection::runtime_context("R".to_string()),
                Injection::message(user_msg("hi")),
            ],
            &mut sys,
            &mut msgs,
        );
        assert!(sys[0].starts_with("BASE_STABLE"), "stable prefix preserved: {}", sys[0]);
        assert!(sys[0].contains("<user_memory>"));
        assert!(sys[0].contains("<skill name=\"s\">"));
        assert!(!sys[0].contains("<runtime>"), "runtime should NOT be in stable");
        assert!(sys[1].starts_with("BASE_DYN"));
        assert!(sys[1].contains("<runtime>"));
        assert_eq!(msgs.len(), 1);
    }

    // -- resolve_selector --------------------------------------------------

    #[test]
    fn resolve_selector_variants() {
        let msgs = vec![
            user_msg("first"),
            user_msg("second"),
            user_msg("third"),
        ];

        assert_eq!(resolve_selector(&MessageSelector::FromStart, &msgs, 99), 0);
        assert_eq!(resolve_selector(&MessageSelector::ToEnd, &msgs, 99), 3);
        assert_eq!(resolve_selector(&MessageSelector::ByIndex(1), &msgs, 99), 1);
        // Out-of-bounds index clamps to len.
        assert_eq!(resolve_selector(&MessageSelector::ByIndex(99), &msgs, 7), 3);

        // ByID match: pull a real id from msgs[1]
        let id = match &msgs[1] {
            AgentMessage::Standard(m) => m.id.clone(),
            _ => unreachable!(),
        };
        assert_eq!(
            resolve_selector(&MessageSelector::ById(id), &msgs, 99),
            1,
            "id match should return its index"
        );
        // ById miss falls back to default
        assert_eq!(
            resolve_selector(
                &MessageSelector::ById("nonexistent-id".to_string()),
                &msgs,
                42,
            ),
            42,
            "missing id should fall back to default"
        );
    }

    #[test]
    fn resolve_selector_empty_messages() {
        let msgs: Vec<AgentMessage> = vec![];
        assert_eq!(resolve_selector(&MessageSelector::FromStart, &msgs, 0), 0);
        assert_eq!(resolve_selector(&MessageSelector::ToEnd, &msgs, 0), 0);
        // ByIndex on empty clamps to 0
        assert_eq!(resolve_selector(&MessageSelector::ByIndex(5), &msgs, 0), 0);
    }

    // -- serialize_range ---------------------------------------------------

    #[test]
    fn serialize_range_renders_role_and_text_with_base_idx() {
        let msgs = vec![user_msg("hello"), user_msg("world")];
        let s = serialize_range(&msgs, 10);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 2);
        // base_idx of 10 means first entry shows as [10], second as [11]
        assert!(lines[0].starts_with("[10] User: hello"), "got: {}", lines[0]);
        assert!(lines[1].starts_with("[11] User: world"), "got: {}", lines[1]);
    }

    #[test]
    fn serialize_range_truncates_text_over_2000_chars() {
        let big = "a".repeat(2500);
        let msgs = vec![user_msg(&big)];
        let s = serialize_range(&msgs, 0);
        assert!(s.contains("...[truncated]"), "expected truncation marker: {}…", &s[..80]);
        // The kept portion is the first 2000 chars of `text`, prefixed
        // by `[0] User: `. So full line len ≈ 10 prefix + 2000 + 15 suffix.
        assert!(s.len() < 2100, "truncation didn't actually shorten output: len={}", s.len());
    }

    #[test]
    fn serialize_range_truncation_does_not_panic_on_multibyte_char_at_boundary() {
        // Regression guard for a real bug: the truncation used to do
        // `&text[..2000]` which panics if byte 2000 lands inside a
        // multi-byte UTF-8 char (emoji is 4 bytes, CJK is 3 bytes).
        //
        // Construct: 1998 ASCII + 1 4-byte emoji + 100 more ASCII.
        // text.len() = 1998 + 4 + 100 = 2102, triggers the > 2000
        // branch. Byte 2000 falls inside the emoji (bytes 1998..2002).
        // Naive `&text[..2000]` would panic; the fix backs off to
        // byte 1998 (the boundary before the emoji).
        let text = format!("{}{}{}", "a".repeat(1998), "🦀", "b".repeat(100));
        assert_eq!(text.len(), 2102, "test premise: 2102 bytes");
        assert!(!text.is_char_boundary(2000), "test premise: byte 2000 mid-emoji");
        let msgs = vec![user_msg(&text)];
        // Must not panic. Output must end with "...[truncated]".
        let s = serialize_range(&msgs, 0);
        assert!(s.contains("...[truncated]"));
    }

    #[test]
    fn serialize_range_handles_extension_message() {
        use serde_json::json;
        let msg = AgentMessage::Extension {
            type_name: "custom".to_string(),
            data: json!({"k": "v"}),
        };
        let s = serialize_range(&[msg], 5);
        assert!(s.starts_with("[5] Extension:"), "got: {}", s);
        assert!(s.contains("\"k\""));
    }

    // -- apply_compressions: action dispatch -------------------------------

    fn assistant_msg(id: &str, text: &str) -> AgentMessage {
        AgentMessage::Standard(Message {
            id: id.to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text: text.to_string() }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        })
    }

    #[tokio::test]
    async fn sliding_window_drops_oldest_when_over_keep_recent() {
        let mut msgs = vec![
            user_msg("a"),
            user_msg("b"),
            user_msg("c"),
            user_msg("d"),
        ];
        apply_compressions(
            vec![CompressAction::SlidingWindow { keep_recent: 2 }],
            &mut msgs,
            &NoopContextHandle,
            "sess",
        )
        .await;
        assert_eq!(msgs.len(), 2, "expected 2 most-recent kept");
        // text_content asserts ordering: 'c' and 'd' survive
        let texts: Vec<String> = msgs
            .iter()
            .map(|m| match m {
                AgentMessage::Standard(s) => s.text_content(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(texts, vec!["c", "d"]);
    }

    #[tokio::test]
    async fn sliding_window_noop_when_under_keep_recent() {
        let mut msgs = vec![user_msg("a"), user_msg("b")];
        apply_compressions(
            vec![CompressAction::SlidingWindow { keep_recent: 5 }],
            &mut msgs,
            &NoopContextHandle,
            "sess",
        )
        .await;
        assert_eq!(msgs.len(), 2, "len <= keep_recent → no drop");
    }

    #[tokio::test]
    async fn replace_tool_result_rewrites_matching_message_id_only() {
        let mut msgs = vec![
            assistant_msg("id-1", "original-1"),
            assistant_msg("id-2", "original-2"),
            assistant_msg("id-3", "original-3"),
        ];
        apply_compressions(
            vec![CompressAction::ReplaceToolResult {
                message_id: "id-2".to_string(),
                summary: "REPLACED".to_string(),
            }],
            &mut msgs,
            &NoopContextHandle,
            "sess",
        )
        .await;
        // Only id-2 should be replaced; id-1 and id-3 untouched
        match &msgs[0] {
            AgentMessage::Standard(m) => assert_eq!(m.text_content(), "original-1"),
            _ => panic!(),
        }
        match &msgs[1] {
            AgentMessage::Standard(m) => assert_eq!(m.text_content(), "REPLACED"),
            _ => panic!("id-2 should have been replaced"),
        }
        match &msgs[2] {
            AgentMessage::Standard(m) => assert_eq!(m.text_content(), "original-3"),
            _ => panic!(),
        }
        assert_eq!(msgs.len(), 3, "length must not change on replace");
    }

    #[tokio::test]
    async fn replace_tool_result_noop_on_unknown_id() {
        let mut msgs = vec![assistant_msg("id-1", "original")];
        apply_compressions(
            vec![CompressAction::ReplaceToolResult {
                message_id: "nonexistent".to_string(),
                summary: "X".to_string(),
            }],
            &mut msgs,
            &NoopContextHandle,
            "sess",
        )
        .await;
        match &msgs[0] {
            AgentMessage::Standard(m) => assert_eq!(m.text_content(), "original"),
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn summarize_drains_range_and_inserts_summary_message() {
        let mut msgs = vec![
            user_msg("keep-before"),
            assistant_msg("a", "drain-1"),
            assistant_msg("b", "drain-2"),
            user_msg("keep-after"),
        ];
        // Use ByIndex selectors directly — FromStart/ToEnd would drain the
        // whole vec which is correct but defeats the "drain a slice" test.
        let action = CompressAction::Summarize {
            range: MessageRange {
                from: MessageSelector::ByIndex(1),
                to: MessageSelector::ByIndex(3),
            },
            hints: vec!["short".to_string()],
        };
        apply_compressions(vec![action], &mut msgs, &NoopContextHandle, "sess").await;

        // 4 → 3: removed 2 in [1..3), inserted 1 summary at index 1
        assert_eq!(msgs.len(), 3, "expected drain(2)+insert(1) → net -1");

        // Position 0 untouched
        match &msgs[0] {
            AgentMessage::Standard(m) => assert_eq!(m.text_content(), "keep-before"),
            _ => panic!(),
        }
        // Position 1 is the new summary, wrapped in <conversation_summary>
        match &msgs[1] {
            AgentMessage::Standard(m) => {
                assert_eq!(m.role, MessageRole::User, "summary inserted as User role");
                let t = m.text_content();
                assert!(t.starts_with("<conversation_summary>"), "summary tags missing: {}", t);
                assert!(t.contains("[no context system configured]"), "noop output missing: {}", t);
                assert!(t.trim_end().ends_with("</conversation_summary>"), "close tag missing: {}", t);
            }
            _ => panic!("summary should be Standard message"),
        }
        // Position 2 = original `keep-after`
        match &msgs[2] {
            AgentMessage::Standard(m) => assert_eq!(m.text_content(), "keep-after"),
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn summarize_invalid_range_is_noop() {
        // from >= to → skip (guard at line 148: `if from_idx < to_idx && to_idx <= msg_len`)
        let mut msgs = vec![user_msg("a"), user_msg("b")];
        apply_compressions(
            vec![CompressAction::Summarize {
                range: MessageRange {
                    from: MessageSelector::ByIndex(2),
                    to: MessageSelector::ByIndex(1),
                },
                hints: vec![],
            }],
            &mut msgs,
            &NoopContextHandle,
            "sess",
        )
        .await;
        assert_eq!(msgs.len(), 2, "invalid range → no mutation");
    }

    #[tokio::test]
    async fn externalize_and_remove_by_priority_are_noops_currently() {
        // Both are documented as TODO / not-yet-active. Calling them
        // must not panic and must not mutate the message vec — a
        // regression that started mutating prematurely would break
        // downstream expectations.
        let mut msgs = vec![user_msg("a"), user_msg("b")];
        let before = msgs.len();
        apply_compressions(
            vec![
                CompressAction::Externalize {
                    range: MessageRange {
                        from: MessageSelector::FromStart,
                        to: MessageSelector::ToEnd,
                    },
                    path: "/tmp/x".to_string(),
                },
                CompressAction::RemoveByPriority {
                    priority: crate::scope::context::Priority::Low,
                },
            ],
            &mut msgs,
            &NoopContextHandle,
            "sess",
        )
        .await;
        assert_eq!(msgs.len(), before, "no-op actions must not mutate");
    }
}
