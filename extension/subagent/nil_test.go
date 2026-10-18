// INPUT: context, encoding/json, errors, testing, agent/abi/context, kernel/abi, llm/wire
// OUTPUT: Typed-nil delegation capabilities, communication handles and factory outputs rejected before child execution
// POS: [L4 extension] Keeps invalid executable contributions from failing only after a child performs work.
package subagent

import (
	"context"
	"encoding/json"
	"errors"
	"testing"

	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type absentReferences struct{ agentcontext.ReferenceBackend }
type absentTemplates struct{ TemplateRegistry }
type absentRunner struct{ Runner }
type absentSession struct{ abi.ConversationLog }
type absentHooks struct{ abi.ContextHooks }

type handleComm struct{ handle agentabi.SpawnCommHandle }

func (handleComm) Kind() string                  { return "handle" }
func (handleComm) Description() string           { return "fixture" }
func (handleComm) ConfigSchema() json.RawMessage { return json.RawMessage(`{"type":"object"}`) }
func (c handleComm) Attach(context.Context, agentabi.SpawnCommContext, json.RawMessage) (agentabi.SpawnCommHandle, error) {
	return c.handle, nil
}

func TestInvalidCommunicationHandlesRejectedBeforeFactoryIO(t *testing.T) {
	for _, handle := range []agentabi.SpawnCommHandle{
		{OnComplete: agentabi.ChildCompleteFunc(nil)},
		{OnComplete: completionFunc(nil)},
		{Hooks: []abi.ContextHooks{nil}},
		{Hooks: []abi.ContextHooks{(*absentHooks)(nil)}},
	} {
		p, _, execution := setup(t, func(context.Context, []wire.Message, func(abi.Event) error) error {
			t.Error("invalid handle started child")
			return nil
		}, nil)
		prepared := false
		p.config.Factory = ChildFactoryFunc(func(context.Context, ChildRequest) (Child, error) {
			prepared = true
			return Child{}, errors.New("factory must not be called")
		})
		registry, _ := agentabi.NewCommunicationRegistry(4)
		if err := registry.Register(handleComm{handle}); err != nil {
			t.Fatal(err)
		}
		p.config.Communications = registry
		if _, err := execute(t, p, execution, `{"task":"work","comms":[{"kind":"handle","config":{}}]}`); err == nil || prepared {
			t.Fatal("invalid communication escaped validation", prepared, err)
		}
	}
}

func TestSubagentConstructorRejectsTypedNilCapabilities(t *testing.T) {
	p, _, _ := setup(t, func(context.Context, []wire.Message, func(abi.Event) error) error { return nil }, nil)
	for _, mutate := range []func(*Config){
		func(c *Config) { c.Factory = ChildFactoryFunc(nil) },
		func(c *Config) { c.References = (*absentReferences)(nil) },
		func(c *Config) { c.Models = ModelResolverFunc(nil) },
		func(c *Config) { c.Templates = (*absentTemplates)(nil) },
	} {
		config := p.config
		mutate(&config)
		if _, err := New(config); err == nil {
			t.Fatal("typed-nil capability admitted")
		}
	}
}

func TestFactoryTypedNilOutputsStillCleanUpBeforeExecution(t *testing.T) {
	for _, nilField := range []string{"runner", "session"} {
		t.Run(nilField, func(t *testing.T) {
			p, _, execution := setup(t, func(context.Context, []wire.Message, func(abi.Event) error) error {
				t.Error("invalid child ran")
				return nil
			}, nil)
			original := p.config.Factory
			closed := 0
			p.config.Factory = ChildFactoryFunc(func(ctx context.Context, request ChildRequest) (Child, error) {
				child, err := original.Prepare(ctx, request)
				if nilField == "runner" {
					child.Runner = (*absentRunner)(nil)
				} else {
					child.Session = (*absentSession)(nil)
				}
				child.Close = func(context.Context) error { closed++; return nil }
				return child, err
			})
			if _, err := execute(t, p, execution, `{"task":"work"}`); err == nil || closed != 1 {
				t.Fatal("invalid factory result leaked resources", closed, err)
			}
		})
	}
}

func TestNilDelegationFunctionAdaptersReturnErrors(t *testing.T) {
	if _, err := ChildFactoryFunc(nil).Prepare(t.Context(), ChildRequest{}); err == nil {
		t.Fatal("missing factory accepted")
	}
	if _, err := ModelResolverFunc(nil).Resolve(t.Context(), "model"); err == nil {
		t.Fatal("missing resolver accepted")
	}
}
