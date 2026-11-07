// INPUT: Explicit portable generation values from an admitted execution.
// OUTPUT: Validated optional sampling values, independent of provider and product defaults.
// POS: [L4.5 engine contract] Value-only request parameters; adapters declare and enforce support.
package engine

import (
	"errors"
	"math"
)

type GenerationOptions struct {
	Temperature     *float64 `json:"temperature,omitempty"`
	MaxTokens       *uint32  `json:"max_tokens,omitempty"`
	ReasoningEffort string   `json:"reasoning_effort,omitempty"`
}

func (v GenerationOptions) Validate() error {
	if v.Temperature != nil && (math.IsNaN(*v.Temperature) || math.IsInf(*v.Temperature, 0) || *v.Temperature < 0) {
		return errors.New("temperature must be finite and nonnegative")
	}
	if v.MaxTokens != nil && *v.MaxTokens == 0 {
		return errors.New("max output tokens must be positive")
	}
	switch v.ReasoningEffort {
	case "", "none", "minimal", "low", "medium", "high", "xhigh", "max":
	default:
		return errors.New("unknown reasoning effort")
	}
	return nil
}

func (v GenerationOptions) Clone() GenerationOptions {
	if v.Temperature != nil {
		x := *v.Temperature
		v.Temperature = &x
	}
	if v.MaxTokens != nil {
		x := *v.MaxTokens
		v.MaxTokens = &x
	}
	return v
}

func (v GenerationOptions) Empty() bool {
	return v.Temperature == nil && v.MaxTokens == nil && v.ReasoningEffort == ""
}
