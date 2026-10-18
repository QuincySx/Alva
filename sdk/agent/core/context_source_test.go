// INPUT: context, errors, testing, agent/core, kernel/abi, llm/wire
// OUTPUT: Independent source contribution, authority, no-fallback and sealing regressions
// POS: [L2.5 SDK acceptance] Custom working views cannot replace original logs or silently trigger full history reads.
package core_test

import (
	"context"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type sourceFunc func(context.Context, abi.ConversationReader) ([]wire.Message, error)

func (f sourceFunc) Load(ctx context.Context, r abi.ConversationReader) ([]wire.Message, error) {
	return f(ctx, r)
}

type sourcePlugin struct {
	name   string
	source abi.ContextSource
	saved  **core.Registrar
}

func (p sourcePlugin) Name() string { return p.name }
func (p sourcePlugin) Register(_ context.Context, r *core.Registrar) error {
	if p.saved != nil {
		*p.saved = r
	}
	return r.ContextSource(p.source)
}

type unreadableFullHistory struct {
	*core.MemorySession
	reads int
}

func (s *unreadableFullHistory) Messages(context.Context) ([]wire.Message, error) {
	s.reads++
	return nil, errors.New("full history unavailable")
}

func TestContextSourceContributionIsReadOnlyAndNeverFallsBack(t *testing.T) {
	for _, fail := range []bool{false, true} {
		t.Run(map[bool]string{false: "view", true: "failure"}[fail], func(t *testing.T) {
			session := &unreadableFullHistory{MemorySession: core.NewMemorySession("actual")}
			failure := errors.New("source unavailable")
			calls := 0
			source := sourceFunc(func(_ context.Context, r abi.ConversationReader) ([]wire.Message, error) {
				if r.ID() != "actual" {
					t.Fatal("source got wrong actual reader")
				}
				if _, ok := r.(abi.ConversationLog); ok {
					t.Fatal("source regained log write authority")
				}
				if fail {
					return nil, failure
				}
				return []wire.Message{wire.TextMessage(wire.RoleUser, "bounded working view")}, nil
			})
			model := modelFunc(func(_ context.Context, q abi.ModelRequest, e func(wire.StreamChunk) error) error {
				calls++
				if len(q.Messages) != 1 || q.Messages[0].Text() != "bounded working view" {
					t.Fatal("source not selected")
				}
				return e(wire.StreamChunk{Kind: wire.ChunkFinish})
			})
			var saved *core.Registrar
			agent := build(t, core.NewBuilder(model, session).Plugin(sourcePlugin{"source", source, &saved}))
			err := agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "original")}, discard)
			if fail {
				if !errors.Is(err, failure) || calls != 0 {
					t.Fatal("source failure swallowed", err, calls)
				}
			} else if err != nil || calls != 1 {
				t.Fatal(err, calls)
			}
			if session.reads != 0 {
				t.Fatal("source failure triggered full-history fallback")
			}
			if err := saved.ContextSource(source); err == nil {
				t.Fatal("late source contribution accepted")
			}
			original, err := session.MemorySession.Messages(t.Context())
			if err != nil || len(original) == 0 || original[0].Text() != "original" {
				t.Fatal("working view replaced original history", err)
			}
		})
	}
}

func TestContextSourceDuplicateAndTypedNilAreRejected(t *testing.T) {
	var missing sourceFunc
	source := sourceFunc(func(context.Context, abi.ConversationReader) ([]wire.Message, error) { return nil, nil })
	for _, plugins := range [][]core.Plugin{{sourcePlugin{name: "missing", source: missing}}, {sourcePlugin{name: "one", source: source}, sourcePlugin{name: "two", source: source}}} {
		builder := core.NewBuilder(textModel("unused"), core.NewMemorySession("s"))
		for _, p := range plugins {
			builder.Plugin(p)
		}
		if _, err := builder.Build(t.Context()); err == nil {
			t.Fatal("invalid source assembly accepted")
		}
	}
}
