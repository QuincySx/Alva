// INPUT: Injected LanguageModel, versioned single-response cases and explicit bounded trial options.
// OUTPUT: Plan/Run and privacy-preserving per-case reports separating behavior failures from execution errors.
// POS: [integration / evaluation] Measures model contracts without executing advertised tools or changing the Agent runtime.
package eval

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// Grade returns bounded machine-readable finding codes; nil means passed.
// It examines one completed model response. Tools are declarations only.
type Case struct {
	ID, Version string
	Request     abi.ModelRequest
	Grade       func(wire.Message) []string
}

// Target is operator-provided route metadata, not proof of server identity.
type Target struct {
	ID       string `json:"id"`
	Protocol string `json:"protocol"`
	Model    string `json:"configured_model"`
}
type Options struct {
	Repeats, MaxCalls int
	Timeout           time.Duration
}
type Result struct {
	CaseID        string      `json:"case_id"`
	CaseVersion   string      `json:"case_version"`
	RequestSHA256 string      `json:"request_sha256"`
	Trial         int         `json:"trial"`
	Status        string      `json:"status"`
	Findings      []string    `json:"findings,omitempty"`
	DurationMS    int64       `json:"duration_ms"`
	FirstOutputMS *int64      `json:"first_output_ms,omitempty"`
	TextBytes     int         `json:"text_bytes"`
	ToolCalls     int         `json:"tool_calls"`
	Usage         *wire.Usage `json:"usage,omitempty"`
}
type Report struct {
	Version        int      `json:"version"`
	Scope          string   `json:"scope"`
	Target         Target   `json:"target"`
	Repeats        int      `json:"repeats"`
	TimeoutMS      int64    `json:"timeout_ms"`
	PlannedCalls   int      `json:"planned_calls"`
	AttemptedCalls int      `json:"attempted_calls"`
	Passed         int      `json:"passed"`
	Failed         int      `json:"failed"`
	Errors         int      `json:"errors"`
	Results        []Result `json:"results"`
}

type frozenCase struct {
	Case
	hash string
}

func token(value string, max int, path bool) bool {
	if len(value) == 0 || len(value) > max {
		return false
	}
	for _, r := range value {
		if !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || strings.ContainsRune("_.-", r) || path && r == '/') {
			return false
		}
	}
	return true
}
func prepare(target Target, cases []Case, options Options) (Report, []frozenCase, Options, error) {
	if options.Repeats == 0 {
		options.Repeats = 1
	}
	if options.MaxCalls == 0 {
		options.MaxCalls = 32
	}
	if options.Timeout == 0 {
		options.Timeout = 30 * time.Second
	}
	if !token(target.ID, 64, false) || !token(target.Protocol, 64, false) || len(target.Model) == 0 || len(target.Model) > 256 || !utf8.ValidString(target.Model) || strings.ContainsRune(target.Model, 0) {
		return Report{}, nil, options, errors.New("bounded explicit evaluation target required")
	}
	if len(cases) == 0 || len(cases) > 64 || options.Repeats < 1 || options.Repeats > 20 || options.MaxCalls < 1 || options.MaxCalls > 256 || options.Timeout <= 0 || options.Timeout > 2*time.Minute || len(cases) > options.MaxCalls/options.Repeats {
		return Report{}, nil, options, errors.New("evaluation exceeds case, repetition, call or timeout budget")
	}
	seen := map[string]bool{}
	frozen := make([]frozenCase, 0, len(cases))
	for _, c := range cases {
		if !token(c.ID, 128, true) || !token(c.Version, 64, false) || seen[c.ID] || c.Grade == nil {
			return Report{}, nil, options, errors.New("unique versioned cases with graders required")
		}
		seen[c.ID] = true
		if err := c.Request.Config.Validate(); err != nil {
			return Report{}, nil, options, errors.New("invalid evaluation model configuration")
		}
		if len(c.Request.Messages) == 0 || len(c.Request.Messages) > 64 || len(c.Request.Tools) > 32 {
			return Report{}, nil, options, errors.New("bounded evaluation request required")
		}
		for _, m := range c.Request.Messages {
			if m.Validate() != nil {
				return Report{}, nil, options, errors.New("invalid evaluation message")
			}
		}
		tools := map[string]bool{}
		for _, t := range c.Request.Tools {
			if !token(t.Name, 64, false) || tools[t.Name] || len(t.Description) > 8192 || !wire.ValidJSON(t.Parameters) {
				return Report{}, nil, options, errors.New("invalid evaluation tool declaration")
			}
			var object map[string]any
			if json.Unmarshal(t.Parameters, &object) != nil || object == nil {
				return Report{}, nil, options, errors.New("tool schema must be an object")
			}
			tools[t.Name] = true
		}
		data, err := json.Marshal(c.Request)
		if err != nil || len(data) > 128<<10 {
			return Report{}, nil, options, errors.New("evaluation request exceeds 128 KiB")
		}
		var request abi.ModelRequest
		if json.Unmarshal(data, &request) != nil {
			return Report{}, nil, options, errors.New("evaluation request cannot be frozen")
		}
		c.Request = request
		sum := sha256.Sum256(data)
		frozen = append(frozen, frozenCase{c, hex.EncodeToString(sum[:])})
	}
	return Report{Version: 1, Scope: "single_response", Target: target, Repeats: options.Repeats, TimeoutMS: options.Timeout.Milliseconds(), PlannedCalls: len(cases) * options.Repeats, Results: []Result{}}, frozen, options, nil
}

// Plan validates and freezes all requests without consulting a model or network.
func Plan(target Target, cases []Case, options Options) (Report, error) {
	report, frozen, options, err := prepare(target, cases, options)
	if err != nil {
		return report, err
	}
	for _, c := range frozen {
		for trial := 1; trial <= options.Repeats; trial++ {
			report.Results = append(report.Results, Result{CaseID: c.ID, CaseVersion: c.Version, RequestSHA256: c.hash, Trial: trial, Status: "planned"})
		}
	}
	return report, nil
}

// Run is sequential, performs no retries and returns partial results on caller
// cancellation. Per-call deadlines require a context-cooperative model, as the
// existing LanguageModel contract does; no detached worker pretends to time out.
func Run(ctx context.Context, model abi.LanguageModel, target Target, cases []Case, options Options) (Report, error) {
	report, frozen, options, err := prepare(target, cases, options)
	if err != nil {
		return report, err
	}
	if model == nil {
		return report, errors.New("model required")
	}
	value := reflect.ValueOf(model)
	switch value.Kind() {
	case reflect.Pointer, reflect.Func, reflect.Map, reflect.Slice, reflect.Chan, reflect.Interface:
		if value.IsNil() {
			return report, errors.New("non-nil model required")
		}
	}
	for _, c := range frozen {
		for trial := 1; trial <= options.Repeats; trial++ {
			if err := ctx.Err(); err != nil {
				return report, err
			}
			result := runCase(ctx, model, c, trial, options.Timeout)
			report.Results = append(report.Results, result)
			report.AttemptedCalls++
			switch result.Status {
			case "passed":
				report.Passed++
			case "failed":
				report.Failed++
			default:
				report.Errors++
			}
			if err := ctx.Err(); err != nil {
				return report, err
			}
		}
	}
	return report, nil
}
func runCase(parent context.Context, model abi.LanguageModel, c frozenCase, trial int, timeout time.Duration) Result {
	result := Result{CaseID: c.ID, CaseVersion: c.Version, RequestSHA256: c.hash, Trial: trial, Status: "error"}
	start := time.Now()
	ctx, cancel := context.WithTimeout(parent, timeout)
	defer cancel()
	accumulator := wire.NewStreamAccumulator(256 << 10)
	chunks := 0
	var callbackErr error
	request := c.Request
	request.Messages = wire.CloneMessages(request.Messages)
	request.Config = wire.CloneModelConfig(request.Config)
	request.Tools = append([]wire.ToolDefinition(nil), request.Tools...)
	for i := range request.Tools {
		request.Tools[i].Parameters = append(json.RawMessage(nil), request.Tools[i].Parameters...)
	}
	err := model.Stream(ctx, request, func(chunk wire.StreamChunk) error {
		if callbackErr != nil {
			return callbackErr
		}
		if ctx.Err() != nil {
			callbackErr = ctx.Err()
			return callbackErr
		}
		chunks++
		if chunks > 4096 {
			callbackErr = errors.New("stream chunk budget exceeded")
			return callbackErr
		}
		hasOutput := (chunk.Kind == wire.ChunkText || chunk.Kind == wire.ChunkReasoning) && chunk.Text != "" || chunk.Kind == wire.ChunkToolCall && chunk.ToolCall != nil || chunk.Kind == wire.ChunkContentBlock && chunk.ContentBlock != nil
		if result.FirstOutputMS == nil && hasOutput {
			ms := time.Since(start).Milliseconds()
			result.FirstOutputMS = &ms
		}
		callbackErr = accumulator.Push(chunk)
		return callbackErr
	})
	result.DurationMS = time.Since(start).Milliseconds()
	message, finishErr := accumulator.Finish()
	result.TextBytes = len(message.Text())
	for _, b := range message.Content {
		if b.Kind == wire.ContentToolCall {
			result.ToolCalls++
		}
	}
	if message.Usage != nil {
		copy := wire.CloneUsage(*message.Usage)
		result.Usage = &copy
	}
	if ctx.Err() != nil {
		result.Findings = []string{"request_canceled_or_expired"}
		return result
	}
	if err != nil || callbackErr != nil || finishErr != nil {
		result.Findings = []string{"model_or_stream_error"}
		return result
	}
	result.Findings = c.Grade(message)
	if len(result.Findings) > 32 {
		result.Findings = []string{"invalid_grader_findings"}
		return result
	}
	for _, finding := range result.Findings {
		if !token(finding, 64, false) {
			result.Findings = []string{"invalid_grader_findings"}
			return result
		}
	}
	result.Findings = append([]string(nil), result.Findings...)
	result.Status = "passed"
	if len(result.Findings) > 0 {
		result.Status = "failed"
	}
	return result
}
