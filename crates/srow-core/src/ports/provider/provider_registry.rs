pub use alva_types::{Provider, ProviderRegistry};

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::*;
    use async_trait::async_trait;
    use std::pin::Pin;
    use std::sync::Arc;

    struct MockModel {
        id: String,
    }

    #[async_trait]
    impl LanguageModel for MockModel {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Result<Message, AgentError> {
            Ok(Message::system("mock"))
        }

        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Pin<Box<dyn futures::Stream<Item = StreamEvent> + Send>> {
            Box::pin(tokio_stream::empty())
        }

        fn model_id(&self) -> &str {
            &self.id
        }
    }

    struct MockProvider;

    impl Provider for MockProvider {
        fn id(&self) -> &str {
            "mock"
        }

        fn language_model(
            &self,
            model_id: &str,
        ) -> Result<Arc<dyn LanguageModel>, ProviderError> {
            Ok(Arc::new(MockModel {
                id: model_id.to_string(),
            }))
        }
    }

    #[test]
    fn register_and_lookup() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider));

        assert!(registry.get("mock").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn language_model_shorthand() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider));

        let model = registry.language_model("mock", "gpt-4").unwrap();
        assert_eq!(model.model_id(), "gpt-4");
    }

    #[test]
    fn missing_provider_returns_error() {
        let registry = ProviderRegistry::new();
        let result = registry.language_model("nonexistent", "model");
        assert!(result.is_err());
    }
}
