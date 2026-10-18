// INPUT: Explicit native protocol Kind and model transport configuration.
// OUTPUT: Kind, canonical Build and protocol-specific constructors.
// POS: [integration / model transport] One native model factory selects independent codec adapters without protocol downgrade.
package provider

import "github.com/QuincySx/Alva/sdk/kernel/abi"

type Kind string

const (
	OpenAIChatKind      Kind = "openai_chat"
	OpenAIResponsesKind Kind = "openai_responses"
	AnthropicKind       Kind = "anthropic"
	GeminiKind          Kind = "gemini"
)

func Build(kind Kind, config Config) (abi.LanguageModel, error) { return newModel(kind, config) }
func NewOpenAIResponses(config Config) (abi.LanguageModel, error) {
	return newModel(OpenAIResponsesKind, config)
}
func NewAnthropic(config Config) (abi.LanguageModel, error) { return newModel(AnthropicKind, config) }
func NewGemini(config Config) (abi.LanguageModel, error)    { return newModel(GeminiKind, config) }
