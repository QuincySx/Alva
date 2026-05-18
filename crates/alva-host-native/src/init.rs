// INPUT:  std::sync, alva_kernel_abi
// OUTPUT: model
// POS:    Resolves a "provider/model_id" spec string into a LanguageModel via ProviderRegistry.
use std::sync::Arc;
use alva_kernel_abi::{LanguageModel, ProviderRegistry, ProviderError};

/// Parse a `provider/model_id` string and resolve from registry.
///
/// # Examples
/// ```rust,no_run
/// # use alva_kernel_abi::ProviderRegistry;
/// # let registry = ProviderRegistry::new();
/// let llm = alva_host_native::model("anthropic/claude-sonnet-4-20250514", &registry);
/// ```
pub fn model(
    spec: &str,
    registry: &ProviderRegistry,
) -> Result<Arc<dyn LanguageModel>, ProviderError> {
    let (provider_id, model_id) = spec.split_once('/').ok_or_else(|| {
        ProviderError::InvalidArgument {
            argument: "spec".to_string(),
            message: format!("expected 'provider/model_id' format, got '{}'", spec),
        }
    })?;
    registry.language_model(provider_id, model_id)
}

#[cfg(test)]
mod tests {
    //! Tests for `model()` spec parser.
    //!
    //! The split-once boundary is load-bearing: `model_id` is allowed
    //! to contain `/` (OpenRouter uses `"openai/gpt-4o"`-style ids),
    //! so we MUST cut at the FIRST `/` — switching to `rsplit_once`
    //! silently breaks OpenRouter callers.
    //!
    //! The error path returns ProviderError with the user-supplied
    //! spec in the message so users can copy-paste-diagnose.
    use super::*;
    use alva_kernel_abi::Provider;

    /// Minimal Provider that always errs — registry tests already
    /// pin success path coverage at L119. Here we just need register-
    /// ability so split-then-route reaches the right branch.
    struct ErringProvider {
        id: &'static str,
    }
    impl Provider for ErringProvider {
        fn id(&self) -> &str {
            self.id
        }
        fn language_model(
            &self,
            model_id: &str,
        ) -> Result<Arc<dyn LanguageModel>, ProviderError> {
            // Echo model_id in the error so tests can verify what was
            // forwarded.
            Err(ProviderError::InvalidArgument {
                argument: "model_id".into(),
                message: format!("erring on '{}'", model_id),
            })
        }
    }

    fn registry_with(provider_id: &'static str) -> ProviderRegistry {
        let mut r = ProviderRegistry::new();
        r.register(Arc::new(ErringProvider { id: provider_id }));
        r
    }

    // -- Spec parsing -----------------------------------------------------

    #[test]
    fn missing_slash_returns_invalid_argument_with_spec_in_message() {
        let registry = ProviderRegistry::new();
        match model("just-a-model", &registry) {
            Err(ProviderError::InvalidArgument { argument, message }) => {
                assert_eq!(argument, "spec");
                assert!(
                    message.contains("just-a-model"),
                    "message must include the user's spec for copy-paste diagnosis: {message}"
                );
                assert!(message.contains("expected"), "message must hint at expected format");
            }
            Err(other) => panic!("expected InvalidArgument, got {other:?}"),
            Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn empty_spec_returns_invalid_argument() {
        // "" has no '/' — InvalidArgument fires before anything else.
        let registry = ProviderRegistry::new();
        match model("", &registry) {
            Err(ProviderError::InvalidArgument { .. }) => { /* ok */ }
            Err(other) => panic!("expected InvalidArgument, got {other:?}"),
            Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn unknown_provider_returns_no_such_model_via_registry() {
        // spec format is valid → split succeeds → registry lookup
        // fails → registry's NoSuchModel surfaces.
        let registry = ProviderRegistry::new();
        match model("openai/gpt-4o", &registry) {
            Err(ProviderError::NoSuchModel { model_id, model_type }) => {
                assert_eq!(model_id, "openai:gpt-4o");
                assert_eq!(model_type, "language");
            }
            Err(other) => panic!("expected NoSuchModel, got {other:?}"),
            Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn registered_provider_receives_correct_model_id() {
        // Pin the forwarding: provider receives EXACTLY the part
        // after the first '/' — no rewriting.
        let registry = registry_with("openai");
        match model("openai/gpt-4o", &registry) {
            Err(ProviderError::InvalidArgument { argument: _, message }) => {
                // Our ErringProvider echoes model_id; verify it
                // received "gpt-4o" not "openai/gpt-4o".
                assert!(message.contains("'gpt-4o'"), "model_id forwarded wrongly: {message}");
            }
            Err(other) => panic!("expected echo error, got {other:?}"),
            Ok(_) => panic!("expected Err"),
        }
    }

    // -- Slash-handling edge cases ----------------------------------------

    #[test]
    fn split_once_uses_first_slash_so_model_id_can_contain_slashes() {
        // Pinned regression: OpenRouter uses "openai/gpt-4o"-style
        // ids that contain '/'. The host spec then becomes
        // "openrouter/openai/gpt-4o" — cutting at the FIRST '/'
        // must give provider="openrouter", model_id="openai/gpt-4o".
        // A switch to rsplit_once would route it to provider=
        // "openrouter/openai" (no such provider) silently.
        let registry = registry_with("openrouter");
        match model("openrouter/openai/gpt-4o", &registry) {
            Err(ProviderError::InvalidArgument { argument: _, message }) => {
                assert!(
                    message.contains("'openai/gpt-4o'"),
                    "model_id must preserve all '/' after the first split: {message}"
                );
            }
            Err(other) => panic!("expected echo error from openrouter provider, got {other:?}"),
            Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn leading_slash_creates_empty_provider_id() {
        // split_once('/') on "/foo" returns Some(("", "foo")). We hit
        // the registry path, which will return NoSuchModel for the
        // empty provider id. Pin so callers can rely on the error
        // routing instead of getting InvalidArgument.
        let registry = ProviderRegistry::new();
        match model("/foo", &registry) {
            Err(ProviderError::NoSuchModel { model_id, .. }) => {
                assert_eq!(model_id, ":foo", "empty provider preserved in model_id format");
            }
            Err(other) => panic!("expected NoSuchModel for empty provider, got {other:?}"),
            Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn trailing_slash_creates_empty_model_id() {
        // split_once('/') on "p/" returns Some(("p", "")). Empty model
        // id surfaces as NoSuchModel via the (unregistered) registry.
        let registry = ProviderRegistry::new();
        match model("p/", &registry) {
            Err(ProviderError::NoSuchModel { model_id, .. }) => {
                assert_eq!(model_id, "p:");
            }
            Err(other) => panic!("expected NoSuchModel, got {other:?}"),
            Ok(_) => panic!("expected Err"),
        }
    }
}
