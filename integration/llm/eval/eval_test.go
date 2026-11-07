// INPUT: Real bounded stream assembly, injected model failures and versioned contract graders.
// OUTPUT: Preflight budgets, error/behavior separation, trial isolation, cancellation and private-report acceptance.
// POS: [evaluation testing] No network, tools, credentials or external models are used.
package eval_test

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/llm/eval"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type model func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (m model) Stream(ctx context.Context, r abi.ModelRequest, e func(wire.StreamChunk) error) error {
	return m(ctx, r, e)
}
func target() eval.Target {
	return eval.Target{ID: "fixture", Protocol: "test", Model: "configured-only"}
}
func cases() []eval.Case {
	return []eval.Case{{ID: "choice", Version: "v1", Request: abi.ModelRequest{Messages: []wire.Message{wire.TextMessage(wire.RoleUser, "ask")}, Tools: []wire.ToolDefinition{{Name: "ask", Description: "ask a user", Parameters: []byte(`{"type":"object"}`)}}}, Grade: func(m wire.Message) []string {
		if m.Text() != "accepted" {
			return []string{"unexpected_answer"}
		}
		return nil
	}}}
}

func TestEvaluationDistinguishesBehaviorFromExecutionAndKeepsUsage(t *testing.T) {
	calls := 0
	m := model(func(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		calls++
		if err := emit(wire.StreamChunk{Kind: wire.ChunkUsage, Usage: &wire.Usage{InputTokens: 12, OutputTokens: 3}}); err != nil {
			return err
		}
		if calls == 3 {
			return errors.New("PRIVATE-API-KEY-and-response")
		}
		text := "accepted"
		if calls == 2 {
			text = "wrong"
		}
		if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: text}); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	report, err := eval.Run(t.Context(), m, target(), cases(), eval.Options{Repeats: 3})
	if err != nil || calls != 3 || report.Passed != 1 || report.Failed != 1 || report.Errors != 1 || report.PlannedCalls != 3 || report.AttemptedCalls != 3 {
		t.Fatal(report, err, calls)
	}
	for _, r := range report.Results {
		if r.Usage == nil || r.Usage.InputTokens != 12 || r.RequestSHA256 == "" {
			t.Fatal(r)
		}
	}
	data, _ := json.Marshal(report)
	if strings.Contains(string(data), "PRIVATE") || strings.Contains(string(data), "accepted") || strings.Contains(string(data), "wrong\"") {
		t.Fatal("raw content leaked", string(data))
	}
}

func TestEvaluationPreflightBeforeAnyCallAndPlanIsNotSuccess(t *testing.T) {
	calls := 0
	m := model(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { calls++; return nil })
	for _, options := range []eval.Options{{Repeats: 2, MaxCalls: 1}, {Repeats: -1}, {Timeout: -time.Second}, {Timeout: 3 * time.Minute}, {MaxCalls: 257}} {
		if _, err := eval.Run(t.Context(), m, target(), cases(), options); err == nil {
			t.Fatal("invalid budget admitted", options)
		}
	}
	duplicated := append(cases(), cases()...)
	if _, err := eval.Run(t.Context(), m, target(), duplicated, eval.Options{}); err == nil {
		t.Fatal("duplicate cases admitted")
	}
	plan, err := eval.Plan(target(), cases(), eval.Options{Repeats: 2})
	if err != nil || plan.Passed != 0 || plan.AttemptedCalls != 0 || plan.PlannedCalls != 2 || len(plan.Results) != 2 || plan.Results[0].Status != "planned" || calls != 0 {
		t.Fatal(plan, err, calls)
	}
	var missing model
	if _, err := eval.Run(t.Context(), missing, target(), cases(), eval.Options{}); err == nil {
		t.Fatal("typed nil model accepted")
	}
}

func TestEvaluationFreezesRequestsAndVersionedFingerprints(t *testing.T) {
	c := cases()
	original := string(c[0].Request.Tools[0].Parameters)
	var hashes []string
	m := model(func(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if string(r.Tools[0].Parameters) != original || r.Messages[0].Text() != "ask" {
			t.Fatal("prior trial changed request", r)
		}
		r.Tools[0].Parameters[0] = 'x'
		r.Messages[0].Content[0].Text = "mutated"
		if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "accepted"}); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	r, err := eval.Run(t.Context(), m, target(), c, eval.Options{Repeats: 2})
	if err != nil || r.Passed != 2 || string(c[0].Request.Tools[0].Parameters) != original {
		t.Fatal(r, err)
	}
	for _, result := range r.Results {
		hashes = append(hashes, result.RequestSHA256)
	}
	if hashes[0] != hashes[1] {
		t.Fatal("same recipe fingerprint changed")
	}
	c[0].Request.Tools[0].Name = "another_name"
	plan, err := eval.Plan(target(), c, eval.Options{})
	if err != nil || plan.Results[0].RequestSHA256 == hashes[0] {
		t.Fatal("name variant lost provenance", plan, err)
	}
}

func TestEvaluationCancellationPartialResultsAndNoExtraCalls(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	calls := 0
	m := model(func(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		calls++
		cancel()
		<-ctx.Done()
		return ctx.Err()
	})
	report, err := eval.Run(ctx, m, target(), cases(), eval.Options{Repeats: 3})
	if !errors.Is(err, context.Canceled) || calls != 1 || report.AttemptedCalls != 1 || report.Errors != 1 {
		t.Fatal(report, err, calls)
	}
	m = model(func(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		<-ctx.Done()
		return ctx.Err()
	})
	report, err = eval.Run(t.Context(), m, target(), cases(), eval.Options{Timeout: time.Millisecond})
	if err != nil || report.Errors != 1 || report.Results[0].Findings[0] != "request_canceled_or_expired" {
		t.Fatal(report, err)
	}
}

func TestEvaluationEmptyDeltasAndUsageAreNotFirstOutput(t *testing.T) {
	c := cases()
	c[0].Grade = func(wire.Message) []string { return nil }
	m := model(func(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		for _, chunk := range []wire.StreamChunk{{Kind: wire.ChunkText}, {Kind: wire.ChunkReasoning}, {Kind: wire.ChunkUsage, Usage: &wire.Usage{InputTokens: 10}}, {Kind: wire.ChunkFinish}} {
			if err := emit(chunk); err != nil {
				return err
			}
		}
		return nil
	})
	report, err := eval.Run(t.Context(), m, target(), c, eval.Options{})
	if err != nil || report.Passed != 1 || report.Results[0].FirstOutputMS != nil {
		t.Fatal("empty deltas or usage were counted as content", report, err)
	}
}

func TestEvaluationRejectsTruncationOversizeAndInvalidGraderOutput(t *testing.T) {
	for _, kind := range []string{"missing-finish", "oversize", "ignored-callback", "grader"} {
		t.Run(kind, func(t *testing.T) {
			c := cases()
			if kind == "grader" {
				c[0].Grade = func(wire.Message) []string { return []string{"raw secret with spaces"} }
			}
			m := model(func(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				text := "accepted"
				if kind == "oversize" || kind == "ignored-callback" {
					text = strings.Repeat("x", 257<<10)
				}
				err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: text})
				if err != nil && kind != "ignored-callback" {
					return err
				}
				if kind == "missing-finish" {
					return nil
				}
				_ = emit(wire.StreamChunk{Kind: wire.ChunkFinish})
				return nil
			})
			report, err := eval.Run(t.Context(), m, target(), c, eval.Options{})
			if err != nil || report.Passed != 0 || report.Errors != 1 {
				t.Fatal(report, err)
			}
		})
	}
}
