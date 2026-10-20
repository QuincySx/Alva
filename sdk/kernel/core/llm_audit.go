// INPUT: time, kernel/abi and llm/wire stream metadata.
// OUTPUT: Body-free StreamObservation counters without durable encoding policy.
// POS: [L2 kernel runtime] Measures a logical model stream for an explicitly installed optional observer.
package core

import (
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"time"
)

type StreamObservation struct {
	RunID       string      `json:"run_id"`
	RequestID   string      `json:"request_id"`
	Events      uint64      `json:"event_count"`
	Visible     uint64      `json:"visible_event_count"`
	Text        uint64      `json:"text_delta_count"`
	Reasoning   uint64      `json:"reasoning_delta_count"`
	Tools       uint64      `json:"tool_delta_count"`
	FirstEvent  *int64      `json:"first_event_ms"`
	FirstOutput *int64      `json:"time_to_first_output_ms"`
	FirstToken  *int64      `json:"time_to_first_token_ms"`
	Duration    int64       `json:"stream_duration_ms"`
	Outcome     string      `json:"outcome"`
	FirstKind   string      `json:"first_output_kind,omitempty"`
	Usage       *wire.Usage `json:"-"`
	started     time.Time
}

func newStreamObservation(run, request string) *StreamObservation {
	return &StreamObservation{RunID: run, RequestID: request, started: time.Now()}
}
func (o *StreamObservation) observe(chunk wire.StreamChunk) {
	if o == nil {
		return
	}
	elapsed := max(0, time.Since(o.started).Milliseconds())
	o.Events++
	if o.FirstEvent == nil {
		o.FirstEvent = &elapsed
	}
	kind := ""
	token := false
	switch chunk.Kind {
	case wire.ChunkText:
		if chunk.Text != "" {
			o.Text++
			kind = "text"
			token = true
		}
	case wire.ChunkReasoning:
		if chunk.Text != "" {
			o.Reasoning++
			kind = "reasoning"
			token = true
		}
	case wire.ChunkToolCall:
		o.Tools++
		kind = "tool_call"
	case wire.ChunkContentBlock:
		if b := chunk.ContentBlock; b != nil {
			switch b.Kind {
			case wire.ContentText:
				if b.Text != "" {
					kind = "text"
					token = true
				}
			case wire.ContentReasoning:
				if b.Reasoning != nil && b.Reasoning.Text != "" {
					kind = "reasoning"
					token = true
				}
			case wire.ContentToolCall:
				o.Tools++
				kind = "tool_call"
			case wire.ContentImage, wire.ContentAudio, wire.ContentVideo, wire.ContentRefusal:
				kind = string(b.Kind)
			}
		}
	}
	if kind != "" {
		o.Visible++
		if o.FirstOutput == nil {
			o.FirstOutput = &elapsed
			o.FirstKind = kind
		}
	}
	if token && o.FirstToken == nil {
		o.FirstToken = &elapsed
	}
}
func (o *StreamObservation) snapshot(usage *wire.Usage, err error) StreamObservation {
	o.Duration = max(0, time.Since(o.started).Milliseconds())
	o.Outcome = "ok"
	if err != nil {
		o.Outcome = "error"
		if abi.OnlyCancellation(err) {
			o.Outcome = "cancelled"
		}
	}
	if usage != nil {
		copied := wire.CloneUsage(*usage)
		o.Usage = &copied
	}
	return *o
}
