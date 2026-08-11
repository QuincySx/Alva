// INPUT:  journal StoredSession fields, optional Alva typed records on disk
// OUTPUT: handoff preamble strings for HubRequest::with_handoff_context
// POS:    Builds one-shot post-switch context for the next engine turn.

use std::path::PathBuf;

use alva_engine_adapter_alva::SessionRepository;

use crate::session_store::StoredSession;

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

pub(crate) fn alva_repository_path() -> Option<PathBuf> {
    crate::session_store::SessionStore::default_path().and_then(|p| {
        p.parent()
            .map(|directory| directory.join("alva-history/alva-agent-records.db"))
    })
}

/// Load a short text excerpt from Alva Hub typed records if present.
fn alva_history_excerpt(engine_session_id: Option<&str>) -> Option<String> {
    let sid = engine_session_id.filter(|s| !s.is_empty())?;
    let repository = SessionRepository::open(alva_repository_path()?).ok()?;
    repository.conversation_excerpt(sid).ok().flatten()
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
