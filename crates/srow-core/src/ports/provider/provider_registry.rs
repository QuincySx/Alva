// INPUT:  super::errors, super model traits
// OUTPUT: Provider (trait) — COMMENTED OUT
// POS:    Provider registry trait — factory for obtaining model instances by ID.
//         Commented out during migration: depends on deleted LanguageModel trait.
//         TODO: Rebuild using agent_base::LanguageModel.

// use super::errors::ProviderError;
// use super::embedding_model::EmbeddingModel;
// use super::image_model::ImageModel;
// use super::speech_model::SpeechModel;
// use super::transcription_model::TranscriptionModel;
// use super::reranking_model::RerankingModel;
// use super::video_model::VideoModel;
//
// pub trait Provider: Send + Sync {
//     fn specification_version(&self) -> &str { "v4" }
//     fn language_model(&self, model_id: &str) -> Result<Box<dyn LanguageModel>, ProviderError>;
//     fn embedding_model(&self, model_id: &str) -> Result<Box<dyn EmbeddingModel>, ProviderError>;
//     fn image_model(&self, _model_id: &str) -> Result<Box<dyn ImageModel>, ProviderError> { ... }
//     fn speech_model(&self, _model_id: &str) -> Result<Box<dyn SpeechModel>, ProviderError> { ... }
//     fn transcription_model(&self, _model_id: &str) -> Result<Box<dyn TranscriptionModel>, ProviderError> { ... }
//     fn reranking_model(&self, _model_id: &str) -> Result<Box<dyn RerankingModel>, ProviderError> { ... }
//     fn video_model(&self, _model_id: &str) -> Result<Box<dyn VideoModel>, ProviderError> { ... }
// }
