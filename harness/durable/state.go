// INPUT: bytes/context, crypto/rand/sha256, encoding/hex/json, errors, time/utf8, kernel/abi and llm/wire.
// OUTPUT: Backend, Snapshot, State, Request, Resolution and suspended/completed outcomes.
// POS: [L5 optional harness] Versioned bounded wait state separate from the Agent loop and ordinary tool execution.
package durable

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type Status string

const (
	Idle        Status = "idle"
	Running     Status = "running"
	Waiting     Status = "waiting"
	Interrupted Status = "interrupted"
)

var ErrConflict = errors.New("durable state revision changed")
var ErrInterrupted = errors.New("execution interrupted; inspect facts and explicitly abandon before new work; no automatic replay")
var ErrExpired = errors.New("durable request expired")

type Request struct {
	ID        string          `json:"id"`
	Kind      string          `json:"kind"`
	RunID     string          `json:"run_id"`
	CallID    string          `json:"call_id"`
	Binding   string          `json:"binding"`
	Payload   json.RawMessage `json:"payload"`
	ExpiresAt time.Time       `json:"expires_at"`
	Result    json.RawMessage `json:"result,omitempty"`
}
type State struct {
	Version  int       `json:"version"`
	Status   Status    `json:"status"`
	Requests []Request `json:"requests,omitempty"`
	Grants   []Request `json:"grants,omitempty"`
}
type Snapshot struct {
	Revision uint64
	State    State
}
type Resolution struct {
	RequestID, Binding string
	Result             json.RawMessage
}
type Outcome struct {
	Suspended bool
	Requests  []Request
}

// Backend owns the exact log and an exclusive execution claim. Commit atomically
// compares Revision and appends state plus all supplied conversation messages.
// Load returns owned data; errors must never be retried automatically. Storage
// implementations may be volatile or durable and must describe which they are.
type Backend interface {
	Log() abi.ConversationLog
	Claim(context.Context) (func(), error)
	Load(context.Context) (Snapshot, error)
	Commit(context.Context, uint64, State, []wire.Message) (Snapshot, error)
}

func fingerprint(kind string, payload json.RawMessage) string {
	// Native journals canonicalize object key order. Preserve numeric lexemes
	// while hashing the same semantic object before and after that storage step.
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.UseNumber()
	var value any
	if err := decoder.Decode(&value); err != nil {
		return ""
	}
	if kind == "question" {
		// Keep actual source identities in stored payloads. Only the matching
		// descriptor omits per-invocation IDs, so the same question in a new
		// invocation can consume its one stored answer in the same session.
		if object, ok := value.(map[string]any); ok {
			delete(object, "ID")
			delete(object, "RunID")
			delete(object, "CallID")
		}
	}
	canonical, err := json.Marshal(value)
	if err != nil {
		return ""
	}
	sum := sha256.Sum256(append([]byte(kind+"\x00"), canonical...))
	return hex.EncodeToString(sum[:])
}
func clone[T any](value T) T {
	data, _ := json.Marshal(value)
	var out T
	_ = json.Unmarshal(data, &out)
	return out
}
func (state State) Validate() error {
	if state.Version != 1 || (state.Status != Idle && state.Status != Running && state.Status != Waiting && state.Status != Interrupted) || len(state.Requests) > 16 || len(state.Grants) > 16 {
		return errors.New("unsupported or oversized durable state")
	}
	if state.Status == Idle && (len(state.Requests) > 0 || len(state.Grants) > 0) || state.Status == Waiting && len(state.Requests) == 0 {
		return errors.New("durable status and requests disagree")
	}
	seen := map[string]bool{}
	for _, group := range [][]Request{state.Requests, state.Grants} {
		for _, r := range group {
			if r.ID == "" || len(r.ID) > 128 || r.RunID == "" || r.CallID == "" || len(r.RunID) > 512 || len(r.CallID) > 512 || !utf8.ValidString(r.ID+r.RunID+r.CallID) || seen[r.ID] || r.ExpiresAt.IsZero() || (r.Kind != "question" && r.Kind != "approval" && r.Kind != "external") || len(r.Payload) > 32<<10 || !wire.ValidJSON(r.Payload) || !utf8.Valid(r.Payload) || r.Binding != fingerprint(r.Kind, r.Payload) || len(r.Result) > 16<<10 || (len(r.Result) > 0 && !wire.ValidJSON(r.Result)) {
				return errors.New("invalid durable request identity/binding/result")
			}
			seen[r.ID] = true
			if err := validateRequestSource(r); err != nil {
				return err
			}
			if len(r.Result) > 0 {
				if err := validateResolution(r, r.Result); err != nil {
					return err
				}
			}
		}
	}
	for _, r := range state.Grants {
		if r.Kind == "external" || len(r.Result) == 0 {
			return errors.New("only resolved approval/question decisions may be consumed")
		}
	}
	data, err := json.Marshal(state)
	if err != nil || len(data) > 256<<10 {
		return errors.New("durable snapshot exceeds 256 KiB")
	}
	return nil
}
func newRequest(kind, runID, callID string, payload json.RawMessage, expires time.Time) (Request, error) {
	var id [16]byte
	if _, err := rand.Read(id[:]); err != nil {
		return Request{}, err
	}
	return Request{ID: "wait-" + hex.EncodeToString(id[:]), Kind: kind, RunID: runID, CallID: callID, Binding: fingerprint(kind, payload), Payload: append(json.RawMessage(nil), payload...), ExpiresAt: expires}, nil
}

type suspension struct{}

func (*suspension) Error() string { return "durable request pending; execution yielded without replay" }
func onlySuspension(err error) bool {
	if err == nil {
		return false
	}
	var session *abi.SessionError
	if errors.As(err, &session) {
		return false
	}
	if _, ok := err.(*suspension); ok {
		return true
	}
	if many, ok := err.(interface{ Unwrap() []error }); ok {
		leaves := many.Unwrap()
		if len(leaves) == 0 {
			return false
		}
		for _, leaf := range leaves {
			if !onlySuspension(leaf) {
				return false
			}
		}
		return true
	}
	if one, ok := err.(interface{ Unwrap() error }); ok {
		return onlySuspension(one.Unwrap())
	}
	return false
}
