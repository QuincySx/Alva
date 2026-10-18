// INPUT: context, JSON/crypto/reflect/time, strictjson, portable references and ABI/wire.
// OUTPUT: NewTool, ParseQuestion, Ask and bounded Receipt reuse one human-question implementation.
// POS: [L4 extension] Ordinary question Tools require only a Handler and reference backend; native audit is an adapter concern.
package interaction

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"reflect"
	"time"

	"github.com/QuincySx/Alva/platform/system/strictjson"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type questionTool struct {
	handler    Handler
	references agentcontext.ReferenceBackend
}

// NewTool installs no native lifecycle. Handler must honor its context; the
// caller owns timeout policy, reference scope, persistence and redaction.
func NewTool(handler Handler, references agentcontext.ReferenceBackend) (abi.Tool, error) {
	if missing(handler) || missing(references) {
		return nil, errors.New("question Handler and reference backend required")
	}
	return &questionTool{handler: handler, references: references}, nil
}
func missing(value any) bool {
	if value == nil {
		return true
	}
	v := reflect.ValueOf(value)
	switch v.Kind() {
	case reflect.Pointer, reflect.Func, reflect.Map, reflect.Slice, reflect.Interface, reflect.Chan:
		return v.IsNil()
	}
	return false
}

type Receipt struct {
	QuestionID string         `json:"question_id"`
	Question   wire.Reference `json:"question"`
	Answer     wire.Reference `json:"answer"`
	Structured bool           `json:"structured"`
	Selected   []int          `json:"selected"`
	Notice     string         `json:"notice"`
}

func (r Receipt) Result(callID string) (wire.ToolResult, error) {
	raw, err := json.Marshal(map[string]any{"question_id": r.QuestionID, "question": r.Question, "answer": r.Answer, "structured": r.Structured, "selected": r.Selected, "notice": r.Notice})
	if err != nil {
		return wire.ToolResult{}, err
	}
	if len(raw) > 16<<10 {
		return wire.ToolResult{}, errors.New("question receipt exceeds 16 KiB")
	}
	return wire.ToolResult{CallID: callID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(raw)}}}, nil
}

// ParseQuestion derives identity solely from the dispatched execution context.
// Callers must also verify the actual Tool instance before delegating here.
func ParseQuestion(e abi.ToolExecutionContext, call wire.ToolCall) (Question, error) {
	if !small(string(call.Arguments), 32<<10) {
		return Question{}, errors.New("question arguments exceed 32 KiB UTF-8")
	}
	var q Question
	if err := strictjson.ValidateObject(call.Arguments, &q); err != nil {
		return Question{}, err
	}
	d := json.NewDecoder(bytes.NewReader(call.Arguments))
	d.DisallowUnknownFields()
	if err := d.Decode(&q); err != nil {
		return Question{}, err
	}
	if d.Decode(new(any)) != io.EOF {
		return Question{}, errors.New("one question object required")
	}
	// Input identity fields are forbidden even though host DTOs have them.
	var fields map[string]json.RawMessage
	json.Unmarshal(call.Arguments, &fields)
	for key, value := range fields {
		if bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return Question{}, errors.New("question fields cannot be null")
		}
		if key != "question" && key != "header" && key != "options" && key != "multi_select" && key != "metadata" {
			return Question{}, errors.New("host question identity is not model input")
		}
	}
	var random [16]byte
	if _, err := rand.Read(random[:]); err != nil {
		return Question{}, err
	}
	q.ID = "question-" + hex.EncodeToString(random[:])
	q.SessionID = e.SessionID
	q.RunID = e.RunID
	q.CallID = e.CallID
	if err := q.Validate(); err != nil {
		return Question{}, err
	}
	return q, nil
}
func (*questionTool) Name() string { return "ask_human" }
func (*questionTool) Description() string {
	return "Ask a real human clarification and wait for a response; optional 2..4 choices/multi-select. Returns full question/answer references under the configured backend policy; never grants permissions or reads ambient stdin."
}
func (*questionTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["question"],"properties":{"question":{"type":"string","maxLength":8192},"header":{"type":"string","maxLength":1024},"options":{"type":"array","minItems":2,"maxItems":4,"items":{"type":"object","additionalProperties":false,"required":["label"],"properties":{"label":{"type":"string"},"value":{"type":"string"}}}},"multi_select":{"type":"boolean"},"metadata":{"type":"object"}}}`)
}
func (*questionTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (*questionTool) IsReadOnly(wire.ToolCall) bool { return true }
func (*questionTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Human clarification only, explicitly not authorization for host side effects"}
}

// Ask synchronously joins Handler, rejects late answers after cancellation and
// retains bounded source evidence. Native adapters may add an intent to retain;
// their outcome/permission/resource ownership stays outside this operation.
func Ask(ctx context.Context, q Question, handler Handler, retain func(context.Context, string, string, any) (wire.Reference, error)) (Receipt, error) {
	receipt := Receipt{QuestionID: q.ID, Notice: "Use bounded context_read/search for the actual human answer; this is not host authorization"}
	if missing(handler) || retain == nil {
		return receipt, errors.New("question Handler and evidence writer required")
	}
	if err := q.Validate(); err != nil {
		return receipt, err
	}
	if err := ctx.Err(); err != nil {
		return receipt, err
	}
	ref, err := retain(ctx, "human-question", q.ID, q)
	if err != nil {
		return receipt, err
	}
	if err := ref.Validate(); err != nil {
		return receipt, err
	}
	receipt.Question = ref
	if err := ctx.Err(); err != nil {
		return receipt, err
	}
	answer, err := handler.Ask(ctx, CloneQuestion(q))
	if ctx.Err() != nil {
		err = errors.Join(err, ctx.Err())
	}
	if err != nil {
		return receipt, err
	}
	resolved, err := q.Resolve(answer)
	if err != nil {
		return receipt, err
	}
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	ref, err = retain(cleanup, "human-answer", q.ID, resolved)
	if err != nil {
		return receipt, err
	}
	if err := ref.Validate(); err != nil {
		return receipt, err
	}
	receipt.Answer, receipt.Structured, receipt.Selected = ref, resolved.Structured, append([]int(nil), resolved.Selected...)
	return receipt, nil
}
func (t *questionTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if e.ActualTool != t || call.ID == "" || call.Name != t.Name() || e.CallID != call.ID {
		return wire.ToolResult{}, errors.New("question requires actual dispatched tool/call identity")
	}
	q, err := ParseQuestion(e, call)
	if err != nil {
		return wire.ToolResult{}, err
	}
	receipt, err := Ask(ctx, q, t.handler, func(ctx context.Context, _, _ string, value any) (wire.Reference, error) {
		raw, err := json.Marshal(value)
		if err != nil {
			return wire.Reference{}, err
		}
		return t.references.StoreText(ctx, string(raw))
	})
	if err != nil {
		return wire.ToolResult{}, err
	}
	return receipt.Result(call.ID)
}
