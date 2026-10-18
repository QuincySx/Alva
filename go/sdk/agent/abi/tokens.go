// INPUT: encoding/json, llm/wire
// OUTPUT: TokenEstimator, SimpleTokenEstimator
// POS: [L2.3 agent contracts] Pluggable text/message estimates covering all explicit wire blocks, never opening references or media URLs.
package abi

import (
	"encoding/json"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type TokenEstimator interface {
	EstimateText(string) int
	EstimateMessages([]wire.Message) (int, error)
}

// SimpleTokenEstimator is a heuristic, NOT a provider tokenizer or image/audio
// token formula. Serialized non-text blocks count as a conservative size proxy;
// actual selected media cost needs a provider-aware estimator implementation.
type SimpleTokenEstimator struct{}

func (SimpleTokenEstimator) EstimateText(text string) int {
	total, ascii := 0, 0
	for _, r := range text {
		if r < 128 {
			ascii++
		} else {
			total += (ascii + 3) / 4
			ascii = 0
			total++
		}
	}
	return total + (ascii+3)/4
}
func (estimator SimpleTokenEstimator) CountTokens(text string) int {
	return estimator.EstimateText(text)
}
func (estimator SimpleTokenEstimator) EstimateMessages(messages []wire.Message) (int, error) {
	total := 0
	for _, message := range messages {
		if err := message.Validate(); err != nil {
			return 0, err
		}
		total += 4
		for _, block := range message.Content {
			if block.Kind == wire.ContentText {
				total += estimator.EstimateText(block.Text)
				continue
			}
			encoded, err := json.Marshal(block)
			if err != nil {
				return 0, err
			}
			total += estimator.EstimateText(string(encoded))
		}
	}
	return total, nil
}
