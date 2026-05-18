// INPUT:  thiserror, std::io
// OUTPUT: MemoryError
// POS:    Root error enum for the alva-memory crate.
//! Error types for the memory subsystem.

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("embedding error: {0}")]
    Embedding(String),
    #[error("sync error: {0}")]
    Sync(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    //! Tests for MemoryError Display + #[from] io::Error conversion.
    //!
    //! Display strings drive user-visible error messages; #[from] is
    //! the auto-conversion used by `?` on io operations throughout the
    //! memory subsystem — dropping it would break every fs call that
    //! propagates with `?`. Pinning the variant + the message
    //! preservation guards both.
    use super::*;
    use std::io;

    // -- Display strings ---------------------------------------------------

    #[test]
    fn storage_display_includes_payload() {
        let e = MemoryError::Storage("disk full".into());
        assert_eq!(e.to_string(), "storage error: disk full");
    }

    #[test]
    fn embedding_display_includes_payload() {
        let e = MemoryError::Embedding("dimension mismatch".into());
        assert_eq!(e.to_string(), "embedding error: dimension mismatch");
    }

    #[test]
    fn sync_display_includes_payload() {
        let e = MemoryError::Sync("hash mismatch".into());
        assert_eq!(e.to_string(), "sync error: hash mismatch");
    }

    #[test]
    fn io_display_includes_inner_io_message() {
        // Pin: Display interpolates the inner io::Error via {0}, so
        // the message must contain the wrapped message text.
        let inner = io::Error::new(io::ErrorKind::NotFound, "file gone");
        let e = MemoryError::Io(inner);
        let s = e.to_string();
        assert!(s.starts_with("IO error:"), "missing prefix in {s}");
        assert!(s.contains("file gone"), "inner message missing in {s}");
    }

    // -- #[from] auto-conversion -------------------------------------------

    #[test]
    fn from_io_error_produces_io_variant() {
        // Pin: `#[from] std::io::Error` generates `From<io::Error>`
        // for MemoryError, so `?` on io ops auto-converts. A refactor
        // that dropped `#[from]` would fail every fs path silently
        // (compile error, but this test pins the API contract too).
        let inner = io::Error::new(io::ErrorKind::PermissionDenied, "nope");
        let e: MemoryError = inner.into();
        assert!(matches!(e, MemoryError::Io(_)));
    }

    #[test]
    fn from_io_error_preserves_inner_message_through_display() {
        // The user-facing message after auto-conversion still contains
        // the original io::Error text — no loss.
        let inner = io::Error::new(io::ErrorKind::TimedOut, "the request");
        let e: MemoryError = inner.into();
        let s = e.to_string();
        assert!(s.contains("the request"), "io message lost through From: {s}");
    }

    // -- Debug smoke -------------------------------------------------------

    #[test]
    fn all_variants_implement_debug() {
        let variants = vec![
            MemoryError::Storage("a".into()),
            MemoryError::Embedding("b".into()),
            MemoryError::Sync("c".into()),
            MemoryError::Io(io::Error::new(io::ErrorKind::Other, "d")),
        ];
        for v in &variants {
            assert!(!format!("{v:?}").is_empty());
        }
    }
}
