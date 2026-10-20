// INPUT: context, encoding/json, filepath, strings, testing, storage/session, kernel/abi, llm/wire.
// OUTPUT: Durable native recipe, actual identity, reference reuse and bounded history regressions.
// POS: [L5 harness] Real SQLite harness integration with local fixtures only.
package harness

import (
	"context"
	"encoding/json"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestNativeRecipeReopensSameAuthorityWithoutReplay(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "native-private")
	for turn := 0; turn < 2; turn++ {
		repo, err := session.Open(ctx, dir)
		if err != nil {
			t.Fatal(err)
		}
		var log storage.Session
		if turn == 0 {
			log, err = repo.Create(ctx, "product-owner", "actual-native", "")
		} else {
			log, err = repo.OpenSession(ctx, "product-owner", "actual-native")
		}
		if err != nil {
			t.Fatal(err)
		}
		plugins, err := SessionPlugins(ctx, repo, "product-owner", log.ID())
		if err != nil {
			t.Fatal(err)
		}
		builder := NewDemoBuilder(log).Workspace(t.TempDir())
		for _, plugin := range plugins {
			builder.Plugin(plugin)
		}
		agent, err := builder.Build(ctx)
		if err != nil {
			t.Fatal(err)
		}
		var output strings.Builder
		if err = agent.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, "turn")}, func(event abi.Event) error {
			if event.Record != nil && event.Record.SessionID != "actual-native" {
				t.Fatal("native source identity rewritten")
			}
			if event.Kind == abi.EventTextDelta {
				output.WriteString(event.Text)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
		if !strings.Contains(output.String(), "Tool result: turn") {
			t.Fatalf("missing real tool roundtrip: %s", output.String())
		}
		messages, err := log.Messages(ctx)
		if err != nil || len(messages) != (turn+1)*4 {
			t.Fatalf("durable turns lost/duplicated: %d %v", len(messages), err)
		}
		if err = repo.Close(); err != nil {
			t.Fatal("native run claim leaked", err)
		}
	}
}

func TestNativeReferencesAndHistorySurviveReopenAndRemainScoped(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "native-private")
	repo, err := session.Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.Create(ctx, "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	backend, err := repo.BoundReferences(ctx, "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	reference, err := backend.StoreText(ctx, strings.Repeat("原始日志🙂\n", 1000))
	if err != nil {
		t.Fatal(err)
	}
	message := wire.TextMessage(wire.RoleUser, "user requested attached log inspection")
	message.Content = append(message.Content, wire.ContentBlock{Kind: wire.ContentReference, Reference: &reference})
	if _, err = log.AppendRecords(ctx, []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})}); err != nil {
		t.Fatal(err)
	}
	repo.Close()
	repo, err = session.Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	backend, err = repo.BoundReferences(ctx, "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	page, err := backend.ReadText(ctx, reference.URI, 0, 2048)
	if err != nil || !strings.Contains(page.Text, "原始日志") {
		t.Fatalf("reopened artifact lost: %+v %v", page, err)
	}
	tool := nativeHistoryTool{repository: repo, owner: "owner", id: "native"}
	arguments := json.RawMessage(`{"action":"list","limit":20}`)
	if _, err = tool.Execute(ctx, abi.ToolExecutionContext{SessionID: "foreign"}, wire.ToolCall{ID: "foreign", Arguments: arguments}); err == nil {
		t.Fatal("history granted foreign execution")
	}
	for _, input := range []json.RawMessage{[]byte(`{"action":"list","action":"list"}`), []byte(`{"Action":"list"}`)} {
		if _, err := tool.Execute(ctx, abi.ToolExecutionContext{SessionID: "native", CallID: "bad", ActualTool: &tool}, wire.ToolCall{ID: "bad", Name: tool.Name(), Arguments: input}); err == nil {
			t.Error("ambiguous native history arguments admitted")
		}
	}
	if _, err := tool.Execute(ctx, abi.ToolExecutionContext{SessionID: "native"}, wire.ToolCall{ID: "bad", Name: tool.Name(), Arguments: arguments}); err == nil {
		t.Error("unresolved native history Tool admitted")
	}
	result, err := tool.Execute(ctx, abi.ToolExecutionContext{SessionID: "native", CallID: "list", ActualTool: &tool}, wire.ToolCall{ID: "list", Name: tool.Name(), Arguments: arguments})
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Content[0].Text) > 4096 || strings.Contains(result.Content[0].Text, "原始日志") {
		t.Fatal("history metadata leaked raw artifact body")
	}
}
