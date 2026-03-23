// INPUT:  crate::{Provider, ProviderError}
// OUTPUT: assert_provider_conformance, assert_provider_id_non_empty, assert_language_model_returns_valid_id, assert_unknown_model_returns_error, assert_unsupported_models_return_error
// POS:    Provider conformance test helpers — any Provider impl can call these to verify trait contract.

//! Provider conformance test helpers.
//!
//! # Usage (in your provider crate's tests)
//!
//! ```rust,ignore
//! use alva_types::provider_test;
//!
//! #[test]
//! fn conformance() {
//!     let provider = MyProvider::new(api_key);
//!     provider_test::assert_provider_conformance(&provider, "my-model-id");
//! }
//! ```

use crate::{Provider, ProviderError};

/// Assert that provider.id() returns a non-empty, space-free string.
pub fn assert_provider_id_non_empty(provider: &dyn Provider) {
    let id = provider.id();
    assert!(!id.is_empty(), "Provider.id() must return a non-empty string");
    assert!(
        !id.contains(' '),
        "Provider.id() should not contain spaces, got: '{}'",
        id
    );
}

/// Assert that requesting a known model returns a LanguageModel with a non-empty model_id.
pub fn assert_language_model_returns_valid_id(provider: &dyn Provider, model_id: &str) {
    match provider.language_model(model_id) {
        Ok(model) => {
            let returned_id = model.model_id();
            assert!(
                !returned_id.is_empty(),
                "LanguageModel.model_id() must be non-empty"
            );
        }
        Err(e) => {
            panic!(
                "Provider '{}' should support model '{}', got error: {}",
                provider.id(),
                model_id,
                e
            );
        }
    }
}

/// Assert that a nonsense model ID returns NoSuchModel error.
pub fn assert_unknown_model_returns_error(provider: &dyn Provider) {
    match provider.language_model("__nonexistent_model_xyz__") {
        Err(ProviderError::NoSuchModel { .. }) => { /* expected */ }
        Err(other) => {
            panic!(
                "Provider '{}' should return NoSuchModel for unknown model, got error: {}",
                provider.id(),
                other
            );
        }
        Ok(_) => {
            panic!(
                "Provider '{}' should return NoSuchModel for unknown model, but it succeeded",
                provider.id()
            );
        }
    }
}

/// Assert that unsupported capability methods return UnsupportedFunctionality or NoSuchModel.
pub fn assert_unsupported_models_return_error(provider: &dyn Provider) {
    let checks = [
        ("embedding", provider.embedding_model("test").err()),
        ("transcription", provider.transcription_model("test").err()),
        ("speech", provider.speech_model("test").err()),
        ("image", provider.image_model("test").err()),
        ("video", provider.video_model("test").err()),
        ("reranking", provider.reranking_model("test").err()),
        ("moderation", provider.moderation_model("test").err()),
    ];

    for (capability, error) in checks {
        if let Some(err) = error {
            assert!(
                matches!(
                    err,
                    ProviderError::UnsupportedFunctionality(_) | ProviderError::NoSuchModel { .. }
                ),
                "Provider '{}' returned unexpected error for {} capability: {}",
                provider.id(),
                capability,
                err
            );
        }
    }
}

/// Run all basic conformance checks.
pub fn assert_provider_conformance(provider: &dyn Provider, known_model_id: &str) {
    assert_provider_id_non_empty(provider);
    assert_language_model_returns_valid_id(provider, known_model_id);
    assert_unknown_model_returns_error(provider);
    assert_unsupported_models_return_error(provider);
}
