// INPUT:  journal StoredSession fields, optional Alva history JSON on disk
// OUTPUT: handoff preamble strings for HubRequest::with_handoff_context
// POS:    Builds one-shot post-switch context for the next engine turn.

use std::path::PathBuf;

use crate::session_store::{prompt_preview, StoredSession};

/// Build a handoff preamble from journal metadata (+ optional Alva transcript).
pub fn build_handoff_context(
    from_engine: &str,
    to_engine: &str,
    journal: Option<&StoredSession>,
    previous_engine_session_id: Option<&str>,
) -> String {
    let mut body = String::from(
        "## Alva Hub engine handoff\n\n\
         You are continuing **product-session** work after a handoff between coding engines.\n\n",
    );
    body.push_str(&format!("- **From engine:** `{from_engine}`\n"));
    body.push_str(&format!("- **To engine:** `{to_engine}`\n"));
    if let Some(sid) = previous_engine_session_id {
        body.push_str(&format!("- **Previous engine session id:** `{sid}`\n"));
    }
    body.push('\n');

    if let Some(j) = journal {
        if let Some(p) = j.last_prompt_preview.as_deref() {
            body.push_str("### Last user prompt (preview)\n\n");
            body.push_str(p);
            body.push_str("\n\n");
        }
        if let Some(r) = j.last_result_preview.as_deref() {
            body.push_str("### Last assistant result (preview)\n\n");
            body.push_str(r);
            body.push_str("\n\n");
        }
    }

    if from_engine == "alva" {
        if let Some(excerpt) = alva_history_excerpt(previous_engine_session_id) {
            body.push_str("### Excerpt from previous Alva transcript\n\n");
            body.push_str(&excerpt);
            body.push_str("\n\n");
        }
    }

    body.push_str(
        "The previous engine's full conversation is **not** automatically available \
         beyond the previews above. Prefer the previews and the new user message; \
         if critical context is missing, say what you need.\n",
    );
    body
}

fn alva_history_dir() -> Option<PathBuf> {
    crate::session_store::SessionStore::default_path()
        .and_then(|p| p.parent().map(|d| d.join("alva-history")))
}

/// Load a short text excerpt from Alva Hub history JSON if present.
fn alva_history_excerpt(engine_session_id: Option<&str>) -> Option<String> {
    let sid = engine_session_id.filter(|s| !s.is_empty())?;
    let dir = alva_history_dir()?;
    let safe: String = sid
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(format!("{safe}.json"));
    let bytes = std::fs::read(&path).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let arr = value.as_array()?;

    let mut lines = Vec::new();
    let mut chars = 0usize;
    const MAX_CHARS: usize = 2500;
    // Walk newest-last messages; take a tail of text-ish content.
    for msg in arr.iter().rev() {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("?");

        // For tag="kind" on AgentMessage, Standard variant serializes as
        // {"kind":"Standard", "id":..., "role":..., "content":[...]} when using
        // adjacently/internally — with #[serde(tag = "kind")] on enum and
        // Standard(Message), serde typically flattens Message fields into the
        // same object for externally tagged... Actually for internally tagged
        // newtype variants, fields of Message are flattened with kind.
        let texts = extract_text_blocks(msg.get("content"));
        if texts.is_empty() {
            continue;
        }
        let chunk = format!("[{role}] {}", texts.join(" "));
        let chunk = prompt_preview(&chunk, 400).unwrap_or(chunk);
        chars = chars.saturating_add(chunk.len());
        lines.push(chunk);
        if chars >= MAX_CHARS || lines.len() >= 8 {
            break;
        }
    }
    lines.reverse();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn extract_text_blocks(content: Option<&serde_json::Value>) -> Vec<String> {
    let Some(content) = content.and_then(|c| c.as_array()) else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|b| {
            // ContentBlock::Text { text } → {"Text":{"text":"..."}} externally
            // or {"type":"text","text":"..."} depending on serde — try both.
            if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                return Some(t.to_string());
            }
            if let Some(t) = b
                .get("Text")
                .and_then(|o| o.get("text"))
                .and_then(|t| t.as_str())
            {
                return Some(t.to_string());
            }
            None
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preamble_includes_engines_and_previews() {
        let j = StoredSession {
            product_session_id: "p".into(),
            engine_id: "alva".into(),
            engine_session_id: "e".into(),
            workspace: "/w".into(),
            claude_mode: None,
            previous_engine_id: None,
            previous_engine_session_id: None,
            updated_at: 1,
            last_prompt_preview: Some("do the thing".into()),
            last_result_preview: Some("done".into()),
        };
        let s = build_handoff_context("alva", "claude", Some(&j), Some("e-prev"));
        assert!(s.contains("`alva`"));
        assert!(s.contains("`claude`"));
        assert!(s.contains("do the thing"));
        assert!(s.contains("done"));
        assert!(s.contains("e-prev"));
    }
}
