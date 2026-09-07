// INPUT:  alva_engine_runtime::EngineDescriptor + four adapter descriptor()s
// OUTPUT: engine_descriptor(engine_id) — the ONE canonical engine catalog
// POS:    product-level registry of engine self-descriptions (seam ticket 15
//         batch 3). Readiness and permission gate keep querying this instead
//         of duplicating per-engine literals; an engine that is NOT declared
//         here is genuinely "not supported by Hub" (it must register its
//         descriptor to be vetted).

use alva_engine_runtime::EngineDescriptor;

/// The declared catalog of engines Hub can build. Each entry is the adapter's
/// own [`EngineDescriptor`] — permission plans, config surface, install
/// metadata — so no decision point outside this module needs an engine-name
/// literal to answer "is it supported / what does it need / what can it do".
pub fn engine_descriptor(engine_id: &str) -> Option<EngineDescriptor> {
    match engine_id {
        "codex" => Some(alva_engine_adapter_codex::CodexAdapter::descriptor()),
        "pi" => Some(alva_engine_adapter_pi::PiAdapter::descriptor()),
        "claude" => Some(alva_engine_adapter_claude::ClaudePrintAdapter::descriptor()),
        _ => None,
    }
}

/// Whether an engine is declared in the catalog and its descriptor satisfies
/// a permission profile (used by the permission gate instead of a name match).
pub fn engine_can_satisfy(engine_id: &str, profile: &str) -> bool {
    engine_descriptor(engine_id)
        .is_some_and(|descriptor| descriptor.can_satisfy(profile))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_exposes_declared_surfaces_without_product_literals() {
        let codex = engine_descriptor("codex").expect("codex is declared");
        assert!(!codex.config_surface.supports_max_turns);
        assert_eq!(
            codex.install_metadata.required_binary.as_deref(),
            Some("codex")
        );
        assert!(codex.can_satisfy("workspace_write"));

        let pi = engine_descriptor("pi").expect("pi is declared");
        assert!(pi.can_satisfy("workspace_read"));
        assert!(!pi.can_satisfy("interactive"));
        assert_eq!(pi.install_metadata.required_binary.as_deref(), Some("pi"));

        let undeclared = engine_descriptor("gemini");
        assert!(undeclared.is_none(), "an undeclared engine is not supported");
        assert!(!engine_can_satisfy("gemini", "workspace_write"));
    }
}