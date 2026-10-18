// INPUT: Real native SQLite/harness, authenticated local HTTP model and exact approval/file fixtures
// OUTPUT: Cold/no-replay resume, reference-only uploads, actual HITL writes and retained cleanup ownership
// POS: [L6 application] Direct application regressions never import Hub/engine or use paid model accounts.
package direct

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func fixture(t *testing.T, config Config) *Service {
	t.Helper()
	if config.DataDirectory == "" {
		config.DataDirectory = filepath.Join(t.TempDir(), "data")
	}
	s, err := Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := s.Close(ctx); err != nil {
			t.Error(err)
		}
	})
	return s
}
func waitRun(t *testing.T, run Run) host.RunSnapshot {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for {
		snapshot, err := run.Controller.Snapshot(run.ID)
		if err != nil {
			t.Fatal(err)
		}
		if snapshot.Status != host.RunRunning {
			return snapshot
		}
		if time.Now().After(deadline) {
			t.Fatal("actual direct harness did not finish")
		}
		time.Sleep(time.Millisecond)
	}
}
func completed(w http.ResponseWriter, text string) {
	w.Header().Set("Content-Type", "text/event-stream")
	raw, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": map[string]string{"content": text}, "finish_reason": "stop"}}})
	fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", raw)
}

func TestColdDirectResumeKeepsActualNativeIdentityWithoutReplay(t *testing.T) {
	config := Config{DataDirectory: filepath.Join(t.TempDir(), "data"), Demo: true}
	s := fixture(t, config)
	page, err := s.List(t.Context(), "")
	if err != nil || len(page.Sessions) != 0 || len(s.bindings) != 0 {
		t.Fatal("cold open restored work", err)
	}
	id, err := s.Create(t.Context(), "durable direct")
	if err != nil {
		t.Fatal(err)
	}
	run, err := s.Start(t.Context(), id, "first real turn")
	if err != nil {
		t.Fatal(err)
	}
	if snapshot := waitRun(t, run); snapshot.Status != host.RunCompleted || !strings.Contains(snapshot.Text, "first real turn") {
		t.Fatal(snapshot.Status)
	}
	if err := s.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	reopened := fixture(t, config)
	page, err = reopened.List(t.Context(), "")
	if err != nil || len(page.Sessions) != 1 || page.Sessions[0].ID != id || page.Sessions[0].Title != "durable direct" || len(reopened.bindings) != 0 {
		t.Fatal("cold list restored/replaced runtime", err)
	}
	before, err := reopened.Conversation(t.Context(), id, 0)
	if err != nil || len(before.Rows) == 0 || len(reopened.bindings) != 0 {
		t.Fatal("cold history built runtime", err)
	}
	run, err = reopened.Start(t.Context(), id, "second real turn")
	if err != nil {
		t.Fatal(err)
	}
	if snapshot := waitRun(t, run); snapshot.Status != host.RunCompleted {
		t.Fatal(snapshot.Status, snapshot.Error)
	}
	log, err := reopened.repository.OpenSession(t.Context(), owner, id)
	if err != nil {
		t.Fatal(err)
	}
	messages, err := log.Messages(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	users := 0
	for _, message := range messages {
		if message.Role == wire.RoleUser {
			users++
		}
	}
	if users != 2 {
		t.Fatal("reopen/history replayed an input", users)
	}
}

func TestDirectUploadsStayReferencesAndForeignFactsRemainUnavailable(t *testing.T) {
	var calls atomic.Int32
	secretBody := "original-upload-body-must-not-enter-provider"
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		body, _ := io.ReadAll(r.Body)
		if bytes.Contains(body, []byte(secretBody)) {
			t.Error("raw upload entered model")
		}
		if r.Header.Get("Authorization") != "Bearer synthetic-fixture-key" {
			t.Error("explicit API key lost")
		}
		completed(w, "reference only")
	}))
	defer server.Close()
	s := fixture(t, Config{ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "fixture", APIKey: "synthetic-fixture-key"}})
	id, _ := s.Create(t.Context(), "refs")
	other, _ := s.Create(t.Context(), "other")
	path := filepath.Join(t.TempDir(), "upload.txt")
	os.WriteFile(path, []byte(secretBody), 0600)
	artifact, err := s.Upload(t.Context(), id, path)
	if err != nil || calls.Load() != 0 || len(s.bindings) != 0 {
		t.Fatal("upload ran model", err)
	}
	if _, err := s.InspectorRead(t.Context(), other, artifact.Reference().URI, 0); err == nil {
		t.Fatal("other session read upload")
	}
	foreign, err := s.repository.Create(t.Context(), "foreign-owner", "foreign-session", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.History(t.Context(), foreign.ID(), "", 0, 0); !errors.Is(err, session.ErrNotFound) {
		t.Fatal("foreign facts exposed", err)
	}
	run, err := s.Start(t.Context(), id, "inspect only a relevant range if needed")
	if err != nil {
		t.Fatal(err)
	}
	if snapshot := waitRun(t, run); snapshot.Status != host.RunCompleted {
		t.Fatal(snapshot.Status, snapshot.Error)
	}
	if calls.Load() != 1 || len(s.DraftAttachments(id)) != 0 {
		t.Fatal("reference admission not consumed once")
	}
	_, original, err := s.repository.ReadArtifactFull(t.Context(), owner, id, artifact.Reference().URI, 1024)
	if err != nil || string(original) != secretBody {
		t.Fatal("original bytes changed", err)
	}
	log, _ := s.repository.OpenSession(t.Context(), owner, id)
	messages, _ := log.Messages(t.Context())
	references := 0
	for _, message := range messages {
		if message.Role == wire.RoleUser {
			for _, block := range message.Content {
				if block.Kind == wire.ContentReference {
					references++
				}
			}
		}
	}
	if references != 1 {
		t.Fatal("actual committed input lost its upload reference")
	}
}

func TestDirectWorkspaceWritesRequireActualOnceApproval(t *testing.T) {
	workspace := t.TempDir()
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if calls.Add(1) > 1 {
			completed(w, "write approved once")
			return
		}
		args, _ := json.Marshal(map[string]string{"path": "approved.txt", "content": "actual approved bytes"})
		raw, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "direct-write", "type": "function", "function": map[string]string{"name": "write_file", "arguments": string(args)}}}}, "finish_reason": "tool_calls"}}})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", raw)
	}))
	defer server.Close()
	s := fixture(t, Config{Workspace: workspace, ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "fixture"}})
	id, _ := s.Create(t.Context(), "project")
	other, _ := s.Create(t.Context(), "other")
	run, err := s.Start(t.Context(), id, "write only after actual approval")
	if err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(3 * time.Second)
	for len(s.PendingApprovals(id)) == 0 {
		if time.Now().After(deadline) {
			snapshot, _ := run.Controller.Snapshot(run.ID)
			t.Fatal("actual file call did not request approval", snapshot.Status, snapshot.Error)
		}
		time.Sleep(time.Millisecond)
	}
	requests := s.PendingApprovals(id)
	if len(requests) != 1 || requests[0].SessionID != id || requests[0].CallID != "direct-write" || !strings.Contains(requests[0].InputSummary, "approved.txt") {
		t.Fatal("approval scope/input lost")
	}
	if _, err := os.Stat(filepath.Join(workspace, "approved.txt")); !os.IsNotExist(err) {
		t.Fatal("write happened before approval")
	}
	if err := s.RespondApproval(t.Context(), other, requests[0].ID, true); err == nil {
		t.Fatal("foreign selected session answered")
	}
	if _, err := s.Start(t.Context(), other, "cannot overlap the active root"); err == nil {
		t.Fatal("two direct roots overlapped")
	}
	if err := s.RespondApproval(t.Context(), id, requests[0].ID, true); err != nil {
		t.Fatal(err)
	}
	if snapshot := waitRun(t, run); snapshot.Status != host.RunCompleted {
		t.Fatal(snapshot.Status, snapshot.Error)
	}
	raw, err := os.ReadFile(filepath.Join(workspace, "approved.txt"))
	if err != nil || string(raw) != "actual approved bytes" {
		t.Fatal("normal file tool not executed", err)
	}
	if err := s.RespondApproval(t.Context(), id, requests[0].ID, true); err == nil {
		t.Fatal("expired approval replayed")
	}
}

type blockedRunner func(context.Context, []wire.Message, func(abi.Event) error) error

func (f blockedRunner) Run(ctx context.Context, input []wire.Message, emit func(abi.Event) error) error {
	return f(ctx, input, emit)
}
func TestDirectCloseRetainsDatabaseOwnerUntilActualExecutionExits(t *testing.T) {
	s := fixture(t, Config{Demo: true})
	id, _ := s.Create(t.Context(), "blocked")
	entered, release := make(chan struct{}), make(chan struct{})
	var once sync.Once
	t.Cleanup(func() { once.Do(func() { close(release) }) })
	controller := host.NewController(blockedRunner(func(ctx context.Context, _ []wire.Message, _ func(abi.Event) error) error {
		close(entered)
		<-release
		return ctx.Err()
	}), 256)
	runID, err := controller.StartPrompt(t.Context(), "blocked fixture")
	if err != nil {
		t.Fatal(err)
	}
	s.bindings[id] = &binding{controller: controller, lastRun: runID}
	<-entered
	ctx, cancel := context.WithTimeout(t.Context(), 20*time.Millisecond)
	defer cancel()
	if err := s.Close(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("incomplete execution close reported success", err)
	}
	if other, err := session.Open(t.Context(), s.config.DataDirectory); err == nil {
		other.Close()
		t.Fatal("database ownership released while execution still active")
	}
	once.Do(func() { close(release) })
	if err := s.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
}

func TestDirectColdHistoryOmitsLargeBodyWholeAndNeverRestoresRuntime(t *testing.T) {
	s := fixture(t, Config{Demo: true})
	id, _ := s.Create(t.Context(), "large fact")
	log, _ := s.repository.OpenSession(t.Context(), owner, id)
	message := wire.TextMessage(wire.RoleAssistant, strings.Repeat("large", 12000))
	if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{{Kind: abi.RecordConversation, Namespace: "alva.kernel", Type: "conversation", SchemaVersion: 1, Message: &message}}); err != nil {
		t.Fatal(err)
	}
	page, err := s.Conversation(t.Context(), id, 0)
	if err != nil || len(page.Rows) != 1 || page.Rows[0].Message != nil || page.Rows[0].Processing == "" || len(s.bindings) != 0 {
		t.Fatal("large body masqueraded as complete UI text", err)
	}
	text, err := s.InspectorRead(t.Context(), id, page.Rows[0].Reference.URI, 0)
	if err != nil || !text.Truncated || len(text.Text) > 2048 {
		t.Fatal("bounded explicit original read invalid", err)
	}
}
