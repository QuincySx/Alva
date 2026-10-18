// INPUT: context, encoding/json, errors, strings, testing, skill protocol, SDK context/core and kernel/wire
// OUTPUT: Independent Skill tool framing, capability, cancellation and projection regressions
// POS: [L4 extension] Exercises replaceable repositories without relying on the default harness to enforce the tool's own boundaries.
package skillextension

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/protocol/skill"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type repositoryProbe struct {
	skill.Repository
	lists, loads int
	load         func(context.Context) (skill.Document, error)
}

func (p *repositoryProbe) List(context.Context) ([]skill.Skill, error) {
	p.lists++
	return nil, nil
}
func (p *repositoryProbe) Load(ctx context.Context, _ string) (skill.Document, error) {
	p.loads++
	if p.load != nil {
		return p.load(ctx)
	}
	return skill.Document{Meta: skill.Meta{Name: "example"}, Body: "actual instructions"}, nil
}
func (p *repositoryProbe) Resource(_ context.Context, _ string, path string) (skill.Resource, error) {
	p.loads++
	return skill.Resource{Path: path, Bytes: []byte("actual resource")}, nil
}

type referenceProbe struct {
	agentcontext.ReferenceBackend
	stores int
}

func (p *referenceProbe) StoreText(context.Context, string) (wire.Reference, error) {
	p.stores++
	return wire.Reference{}, nil
}

func skillInvocation(p *Plugin, raw string) (abi.ToolExecutionContext, wire.ToolCall) {
	return abi.ToolExecutionContext{SessionID: "native", ActualTool: p.tool, CallID: "call"}, wire.ToolCall{ID: "call", Name: "skill", Arguments: json.RawMessage(raw)}
}

func TestSkillRejectsInvalidFrameBeforeRepositoryRead(t *testing.T) {
	for _, raw := range []string{`{"name":"example"} {"name":"other"}`, `{"name":"example","operation":null}`, `{"name":"example","path":null}`, `{"name":"example"}` + strings.Repeat(" ", 4096), `{"name":"example","operation":"resource","path":"../foreign"}`, `{"name":"example","operation":"resource","path":"/foreign"}`, `{"name":"example","operation":"resource"}`} {
		repo := &repositoryProbe{}
		refs, _ := agentcontext.NewMemoryReferences("native", 8<<20)
		p := New(repo, "native", refs)
		e, call := skillInvocation(p, raw)
		result, err := p.tool.Execute(t.Context(), e, call)
		if repo.loads != 0 || (err == nil && !result.IsError) {
			t.Error("invalid frame reached repository", repo.loads, result, err)
		}
	}
}

func TestSkillChecksActualDispatchBeforeReading(t *testing.T) {
	for _, mismatch := range []string{"tool", "call", "name"} {
		t.Run(mismatch, func(t *testing.T) {
			repo := &repositoryProbe{}
			refs, _ := agentcontext.NewMemoryReferences("native", 8<<20)
			p := New(repo, "native", refs)
			e, call := skillInvocation(p, `{"name":"example"}`)
			switch mismatch {
			case "tool":
				e.ActualTool = New(repo, "native", refs).tool
			case "call":
				e.CallID = "another-call"
			case "name":
				call.Name = "another-tool"
			}
			if _, err := p.tool.Execute(t.Context(), e, call); err == nil || repo.loads != 0 {
				t.Fatal("mismatched dispatch read repository", repo.loads, err)
			}
		})
	}
}

func TestSkillRejectsMissingImplementationsBeforeCatalog(t *testing.T) {
	for _, missing := range []string{"repository", "references"} {
		t.Run(missing, func(t *testing.T) {
			defer func() {
				if value := recover(); value != nil {
					t.Errorf("missing implementation panicked: %v", value)
				}
			}()
			repo := &repositoryProbe{}
			refs, _ := agentcontext.NewMemoryReferences("native", 8<<20)
			var source skill.Repository = repo
			var backend agentcontext.ReferenceBackend = refs
			if missing == "repository" {
				source = (*repositoryProbe)(nil)
			} else {
				backend = (*referenceProbe)(nil)
			}
			_, err := core.NewBuilder(noModel{}, core.NewMemorySession("native")).Plugin(New(source, "native", backend)).Build(t.Context())
			if err == nil || repo.lists != 0 {
				t.Fatal("missing capability was admitted or read catalog", repo.lists, err)
			}
		})
	}
}

func TestSkillRejectsCancelledReadAndInvalidProjection(t *testing.T) {
	for _, scenario := range []string{"cancel", "invalid_utf8", "foreign_document", "oversize", "invalid_reference"} {
		t.Run(scenario, func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			repo := &repositoryProbe{load: func(context.Context) (skill.Document, error) {
				document := skill.Document{Meta: skill.Meta{Name: "example"}, Body: "actual instructions"}
				switch scenario {
				case "cancel":
					cancel()
				case "invalid_utf8":
					document.Body = string([]byte{0xff})
				case "foreign_document":
					document.Meta.Name = "foreign"
				case "oversize":
					document.Body = strings.Repeat("x", skill.MaxDocumentBytes+1)
				case "invalid_reference":
					document.Body = strings.Repeat("x", 5000)
				}
				return document, nil
			}}
			refs, _ := agentcontext.NewMemoryReferences("native", 8<<20)
			var backend agentcontext.ReferenceBackend = refs
			if scenario == "invalid_reference" {
				backend = &referenceProbe{}
			}
			p := New(repo, "native", backend)
			e, call := skillInvocation(p, `{"name":"example"}`)
			if result, err := p.tool.Execute(ctx, e, call); err == nil || len(result.Content) != 0 || scenario == "cancel" && !errors.Is(err, context.Canceled) {
				t.Fatal("invalid/late projection delivered", result, err)
			}
		})
	}
}
