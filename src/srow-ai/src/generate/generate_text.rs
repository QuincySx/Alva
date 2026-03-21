use srow_core::error::ChatError;

use super::types::{CallSettings, GenerateTextResult, Prompt};

pub async fn generate_text(
    _settings: CallSettings,
    _prompt: Prompt,
) -> Result<GenerateTextResult, ChatError> {
    todo!("Implemented in Task 2")
}
