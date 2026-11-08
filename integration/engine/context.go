// INPUT: errors, unicode/utf8
// OUTPUT: ContextUpdated kind, ContextReport (measured ContextUsage with optional estimated ContextParts, ContextCompaction outcome) and its validation
// POS: [L4.5 engine contract] An engine's own account of its model context; engines that cannot measure it never report one.
package engine

import (
	"errors"
	"unicode/utf8"
)

// ContextUpdated carries a ContextReport: the measured size of the engine's
// latest model request, a context compaction it performed, or both.
const ContextUpdated Kind = "context_updated"

type ContextReport struct {
	Usage      *ContextUsage      `json:"usage,omitempty"`
	Compaction *ContextCompaction `json:"compaction,omitempty"`
}

// ContextUsage is the provider-reported size of the latest model request.
// InputTokens is inclusive of cached input, as in Usage; OutputTokens joins
// the context of the next request. WindowTokens is the model's declared
// window, 0 when undeclared; nothing here is a local estimate except Parts.
type ContextUsage struct {
	InputTokens       uint64  `json:"input_tokens"`
	OutputTokens      uint64  `json:"output_tokens"`
	CachedInputTokens *uint64 `json:"cached_input_tokens,omitempty"`
	WindowTokens      uint64  `json:"window_tokens,omitempty"`
	// Parts splits the measured total by source using local estimates of
	// each part's share; it sums to UsedTokens and is nil when unknown.
	Parts *ContextParts `json:"parts,omitempty"`
}

// UsedTokens is the context the latest request occupied plus its answer.
func (u ContextUsage) UsedTokens() uint64 { return u.InputTokens + u.OutputTokens }

type ContextParts struct {
	System  uint64 `json:"system"`
	Tools   uint64 `json:"tools"`
	Summary uint64 `json:"summary"`
	Recent  uint64 `json:"recent"`
}

type ContextCompactionStatus string

const (
	ContextCompacted        ContextCompactionStatus = "compacted"
	ContextCompactionFailed ContextCompactionStatus = "failed"
	// ContextCompactionSkipped: a requested compaction found nothing to summarize.
	ContextCompactionSkipped ContextCompactionStatus = "skipped"
	// ContextSummaryCorrupt: the stored summary was unusable; the engine fell
	// back to recent conversation and kept the record for diagnosis.
	ContextSummaryCorrupt ContextCompactionStatus = "summary_corrupt"
)

// ContextCompaction is one compaction outcome. Reference names the summary
// record written, or the corrupt one found; Usage is the summary request's
// own token usage, already included in the attempt's completion Usage.
type ContextCompaction struct {
	Status          ContextCompactionStatus `json:"status"`
	Trigger         string                  `json:"trigger"`
	FromSequence    uint64                  `json:"from_sequence,omitempty"`
	ThroughSequence uint64                  `json:"through_sequence,omitempty"`
	Messages        uint64                  `json:"messages,omitempty"`
	Reference       *Reference              `json:"reference,omitempty"`
	Usage           *Usage                  `json:"usage,omitempty"`
	Error           string                  `json:"error,omitempty"`
}

// Validate keeps the report bounded and internally consistent.
func (r ContextReport) Validate() error {
	if r.Usage == nil && r.Compaction == nil {
		return errors.New("context report requires a measurement or a compaction")
	}
	if u := r.Usage; u != nil {
		if u.InputTokens == 0 || u.CachedInputTokens != nil && *u.CachedInputTokens > u.InputTokens {
			return errors.New("context measurement requires reported input tokens")
		}
		if p := u.Parts; p != nil && p.System+p.Tools+p.Summary+p.Recent != u.UsedTokens() {
			return errors.New("context parts must sum to the measured total")
		}
	}
	if c := r.Compaction; c != nil {
		switch c.Status {
		case ContextCompacted:
			if c.FromSequence == 0 || c.FromSequence > c.ThroughSequence || c.Messages == 0 || c.Reference == nil {
				return errors.New("completed compaction requires its covered range and summary record")
			}
		case ContextCompactionFailed, ContextCompactionSkipped, ContextSummaryCorrupt:
		default:
			return errors.New("unknown context compaction status")
		}
		if len(c.Trigger) > 32 || len(c.Error) > 1<<10 || !utf8.ValidString(c.Trigger+c.Error) {
			return errors.New("context compaction detail exceeds its bound")
		}
		if c.Usage != nil {
			if err := c.Usage.Validate(); err != nil {
				return err
			}
		}
	}
	return nil
}
