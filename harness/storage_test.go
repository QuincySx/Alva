// INPUT: Actual SDK model/tool/model loops, native facts and directory/volatile blob adapters.
// OUTPUT: Unchanged reference retrieval composition over replaceable artifact bytes and truthful retention prompts.
// POS: [L5 storage acceptance] Exercises the installed harness path rather than only byte adapter unit behavior.
package harness

import (
	"context"
	"encoding/json"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/platform/storage/blob"
	"github.com/QuincySx/Alva/platform/storage/blob/directory"
	blobmemory "github.com/QuincySx/Alva/platform/storage/blob/memory"
	"github.com/QuincySx/Alva/platform/storage/session"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestNativeHarnessUsesReplaceableArtifactBytes(t *testing.T) {
	for _, name := range []string{"memory", "directory"} {
		t.Run(name, func(t *testing.T) {
			var objects blob.Store
			retention := blob.Volatile
			if name == "memory" {
				store, err := blobmemory.New(blobmemory.Limits{Bytes: 1 << 20, Objects: 32})
				if err != nil {
					t.Fatal(err)
				}
				objects = store
			} else {
				if runtime.GOOS == "windows" {
					t.Skip("directory adapter requires directory Sync support")
				}
				store, err := directory.Open(t.TempDir())
				if err != nil {
					t.Fatal(err)
				}
				defer store.Close()
				objects, retention = store, blob.Persistent
			}
			repo, err := session.OpenWithBlobs(t.Context(), filepath.Join(t.TempDir(), "facts"), session.BlobConfig{ID: "objects", Retention: retention, Store: objects})
			if err != nil {
				t.Fatal(err)
			}
			defer repo.Close()
			log, err := repo.Create(t.Context(), "owner", "actual", "")
			if err != nil {
				t.Fatal(err)
			}
			refs, err := repo.BoundReferences(t.Context(), "owner", log.ID())
			if err != nil {
				t.Fatal(err)
			}
			text := "selected storage evidence:" + strings.Repeat(" bounded original", 800)
			ref, err := refs.StoreText(t.Context(), text)
			if err != nil {
				t.Fatal(err)
			}
			calls := 0
			model := modelFunc(func(_ context.Context, req abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				calls++
				encoded, _ := json.Marshal(req)
				if strings.Contains(string(encoded), text) {
					t.Fatal("complete artifact auto-delivered")
				}
				if calls == 1 {
					if !strings.Contains(string(encoded), "Artifact byte retention is "+string(retention)) {
						t.Fatal("artifact lifetime not stated")
					}
					args, _ := json.Marshal(map[string]any{"uri": ref.URI, "limit": 64})
					if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "read", Name: "context_read", Arguments: args}}); err != nil {
						return err
					}
				} else {
					last := req.Messages[len(req.Messages)-1]
					if last.Role != wire.RoleTool || len(last.Content) != 1 || last.Content[0].ToolResult == nil {
						t.Fatal("missing actual retrieval result")
					}
					result := last.Content[0].ToolResult
					var page agentcontext.TextPage
					if result.IsError || len(result.Content) != 1 || json.Unmarshal([]byte(result.Content[0].Text), &page) != nil || page.Text != text[:64] || page.TotalBytes != int64(len(text)) || !page.Truncated {
						t.Fatal("bounded original retrieval failed", result)
					}
					if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "selected bytes inspected"}); err != nil {
						return err
					}
				}
				return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
			})
			plugins, err := SessionPlugins(t.Context(), repo, "owner", log.ID())
			if err != nil {
				t.Fatal(err)
			}
			builder := NewBuilder(model, log).Workspace(t.TempDir())
			for _, p := range plugins {
				builder.Plugin(p)
			}
			agent, err := builder.Build(t.Context())
			if err != nil {
				t.Fatal(err)
			}
			if err = agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "inspect the supplied reference")}, func(abi.Event) error { return nil }); err != nil {
				t.Fatal(err)
			}
			if calls != 2 {
				t.Fatal("not a real model/tool/model turn", calls)
			}
			messages, err := log.Messages(t.Context())
			if err != nil || len(messages) != 4 || messages[3].Text() != "selected bytes inspected" {
				t.Fatal("execution facts differ by byte backend", len(messages), err)
			}
		})
	}
}
