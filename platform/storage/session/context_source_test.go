// INPUT: context, encoding/json, errors, fmt, filepath, reflect, strings, testing, agent/core, kernel/abi, llm/wire
// OUTPUT: Bounded native source isolation, restart, protocol and oversized-history integration regressions
// POS: [Native context testing] Proves working views bypass full restoration without silently losing the current turn or original facts.
package session

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type sourceOnlyReader string

func (reader sourceOnlyReader) ID() string { return string(reader) }
func (sourceOnlyReader) Messages(context.Context) ([]wire.Message, error) {
	panic("bounded source restored whole conversation")
}
func addContextMessages(t *testing.T, log *Session, texts ...string) {
	t.Helper()
	batch := make([]abi.SessionRecord, 0, len(texts))
	for _, text := range texts {
		batch = append(batch, conversation(text))
	}
	for i := range batch {
		batch[i] = abi.BuiltinRecord(batch[i])
	}
	if _, err := log.AppendRecords(t.Context(), batch); err != nil {
		t.Fatal(err)
	}
}
func TestNativeContextSourceReopensWithStableBoundedView(t *testing.T) {
	ctx := t.Context()
	dir := filepath.Join(t.TempDir(), "facts")
	repo, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.Create(ctx, "owner", "native-source", "")
	if err != nil {
		t.Fatal(err)
	}
	for start := 0; start < 256; start += 64 {
		var texts []string
		for i := start; i < start+64; i++ {
			texts = append(texts, fmt.Sprintf("message %d", i))
		}
		addContextMessages(t, log, texts...)
	}
	source, err := repo.BoundContextSource(ctx, "owner", log.ID())
	if err != nil {
		t.Fatal(err)
	}
	view, err := source.Load(ctx, sourceOnlyReader(log.ID()))
	if err != nil {
		t.Fatal(err)
	}
	if len(view) != contextTargetMessages+1 || view[len(view)-1].Text() != "message 255" || !strings.Contains(view[0].Text(), "not a summary") {
		t.Fatal("incorrect bounded tail", len(view), err)
	}
	if _, err := repo.BoundContextSource(ctx, "foreign", log.ID()); !errors.Is(err, ErrNotFound) {
		t.Fatal("foreign owner accepted", err)
	}
	if _, err := source.Load(ctx, sourceOnlyReader("foreign")); err == nil {
		t.Fatal("source accepted foreign runtime")
	}
	var missingReader *Session
	if _, err := source.Load(ctx, missingReader); err == nil {
		t.Fatal("source accepted a typed-nil reader")
	}
	all, err := log.Messages(ctx)
	if err != nil || len(all) != 256 {
		t.Fatal("source changed original log", len(all), err)
	}
	// Simulate an existing schema-v2 repository created before the optional
	// working-view access path. Reopen must add it without changing facts.
	if _, err := repo.db.ExecContext(ctx, "DROP INDEX conversation_tail"); err != nil {
		t.Fatal(err)
	}
	if err := repo.Close(); err != nil {
		t.Fatal(err)
	}
	repo, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	var indexed int
	if err := repo.db.QueryRowContext(ctx, "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='conversation_tail'").Scan(&indexed); err != nil || indexed != 1 {
		t.Fatal("older repository did not gain conversation index", indexed, err)
	}
	source, err = repo.BoundContextSource(ctx, "owner", "native-source")
	if err != nil {
		t.Fatal(err)
	}
	reopened, err := source.Load(ctx, sourceOnlyReader("native-source"))
	if err != nil || !reflect.DeepEqual(view, reopened) {
		t.Fatal("restart changed same high-water projection", err)
	}
}

func TestNativeContextSourceIncludesWholeLatestToolTurn(t *testing.T) {
	repo, log := repoSession(t)
	addContextMessages(t, log, "older")
	var blocks []wire.ContentBlock
	for i := 0; i < 80; i++ {
		call := wire.ToolCall{ID: fmt.Sprintf("c%d", i), Name: "fixture", Arguments: json.RawMessage(`{}`)}
		blocks = append(blocks, wire.ContentBlock{Kind: wire.ContentToolCall, ToolCall: &call})
	}
	user := wire.TextMessage(wire.RoleUser, "latest input")
	assistant := wire.Message{Role: wire.RoleAssistant, Content: blocks}
	batch := []abi.SessionRecord{{Kind: abi.RecordConversation, Message: &user}, {Kind: abi.RecordConversation, Message: &assistant}}
	for i := 0; i < 80; i++ {
		result := wire.ToolResult{CallID: fmt.Sprintf("c%d", i), Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "result"}}}
		message := wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &result}}}
		batch = append(batch, abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})
	}
	for i := range batch {
		batch[i] = abi.BuiltinRecord(batch[i])
	}
	if _, err := log.AppendRecords(t.Context(), batch); err != nil {
		t.Fatal(err)
	}
	source, err := repo.BoundContextSource(t.Context(), "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	view, err := source.Load(t.Context(), sourceOnlyReader(log.ID()))
	if err != nil {
		t.Fatal(err)
	}
	if len(view) != 83 || view[1].Text() != "latest input" || len(view[2].Content) != 80 {
		t.Fatal("tool batch lost current input/protocol", len(view))
	}
	if err := validateContextProtocol(view); err != nil {
		t.Fatal(err)
	}
}

type sourceModelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (fn sourceModelFunc) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(ctx, request, emit)
}

type sourceContribution struct{ source abi.ContextSource }

func (sourceContribution) Name() string { return "source-fixture" }
func (plugin sourceContribution) Register(_ context.Context, r *core.Registrar) error {
	return r.ContextSource(plugin.source)
}

func TestNativeContextSourceRunsBeyondCompleteRestoreBudget(t *testing.T) {
	repo, log := repoSession(t)
	// Real original bytes exceed the complete 64 MiB restore budget. None of
	// these old bodies needs to be decoded for a later small bounded turn.
	large := strings.Repeat("x", 16<<20)
	for i := 0; i < 4; i++ {
		addContextMessages(t, log, large)
	}
	large = ""
	addContextMessages(t, log, "latest before the smaller tail")
	earlySource, err := repo.BoundContextSource(t.Context(), "owner", log.ID())
	if err != nil {
		t.Fatal(err)
	}
	earlyView, err := earlySource.Load(t.Context(), sourceOnlyReader(log.ID()))
	if err != nil || len(earlyView) != 2 || earlyView[1].Text() != "latest before the smaller tail" {
		t.Fatal("older oversized turns prevented the latest bounded turn", len(earlyView), err)
	}
	for i := 0; i < 2; i++ {
		texts := make([]string, 64)
		for j := range texts {
			texts[j] = fmt.Sprintf("small %d", i*64+j)
		}
		addContextMessages(t, log, texts...)
	}
	if _, err := log.Messages(t.Context()); err == nil || !strings.Contains(err.Error(), "64 MiB") {
		t.Fatal("fixture did not exceed original complete restore budget", err)
	}
	source, err := repo.BoundContextSource(t.Context(), "owner", log.ID())
	if err != nil {
		t.Fatal(err)
	}
	calls, messageCount := 0, 0
	model := sourceModelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		calls++
		messageCount = len(request.Messages)
		if request.Messages[len(request.Messages)-1].Text() != "newest input" {
			t.Fatal("source lost latest committed input")
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, log).Plugin(sourceContribution{source}).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "newest input")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal("bounded source could not execute", err)
	}
	if calls != 1 || messageCount > contextTargetMessages+1 {
		t.Fatal("model did not receive bounded view", calls, messageCount)
	}
	var stored int
	if err := repo.db.QueryRowContext(t.Context(), "SELECT count(*) FROM records WHERE session_id=? AND kind='conversation'", log.ID()).Scan(&stored); err != nil || stored != 135 {
		t.Fatal("complete facts were changed", stored, err)
	}
}

func TestNativeContextSourceRejectsInvalidSelectedEvidence(t *testing.T) {
	for _, failure := range []string{"bytes", "missing", "body", "orphan", "too-many"} {
		t.Run(failure, func(t *testing.T) {
			repo, log := repoSession(t)
			addContextMessages(t, log, "latest")
			switch failure {
			case "bytes":
				_, _ = repo.db.ExecContext(t.Context(), "UPDATE artifacts SET bytes=? WHERE session_id=?", contextMaxBytes+1, log.ID())
			case "missing":
				_, _ = repo.db.ExecContext(t.Context(), "PRAGMA foreign_keys=OFF")
				_, _ = repo.db.ExecContext(t.Context(), "DELETE FROM artifacts WHERE session_id=?", log.ID())
			case "body":
				_, _ = repo.db.ExecContext(t.Context(), "UPDATE artifact_chunks SET data=?", []byte("corrupt"))
			case "orphan":
				result := wire.ToolResult{CallID: "missing"}
				message := wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &result}}}
				if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})}); err != nil {
					t.Fatal(err)
				}
			case "too-many":
				batch := make([]abi.SessionRecord, contextMaxMessages)
				for i := range batch {
					message := wire.TextMessage(wire.RoleAssistant, "long unfinished turn")
					batch[i] = abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})
				}
				for i := range batch {
					batch[i] = abi.BuiltinRecord(batch[i])
				}
				if _, err := log.AppendRecords(t.Context(), batch); err != nil {
					t.Fatal(err)
				}
			}
			source, err := repo.BoundContextSource(t.Context(), "owner", log.ID())
			if err != nil {
				t.Fatal(err)
			}
			if messages, err := source.Load(t.Context(), sourceOnlyReader(log.ID())); err == nil || messages != nil {
				t.Fatal("invalid evidence yielded partial/success view", messages, err)
			}
		})
	}
}

func TestNativeContextSourceDoesNotReadOmittedBodies(t *testing.T) {
	repo, log := repoSession(t)
	texts := make([]string, 128)
	for i := range texts {
		texts[i] = fmt.Sprintf("history %d", i)
	}
	addContextMessages(t, log, texts...)
	if _, err := repo.db.ExecContext(t.Context(), "UPDATE artifact_chunks SET data=? WHERE artifact_id=(SELECT artifact_id FROM records WHERE session_id=? AND seq=1)", []byte("corrupt omitted body"), log.ID()); err != nil {
		t.Fatal(err)
	}
	if _, err := log.Messages(t.Context()); err == nil {
		t.Fatal("full read did not detect corruption")
	}
	source, err := repo.BoundContextSource(t.Context(), "owner", log.ID())
	if err != nil {
		t.Fatal(err)
	}
	view, err := source.Load(t.Context(), sourceOnlyReader(log.ID()))
	if err != nil || len(view) != contextTargetMessages+1 {
		t.Fatal("source restored omitted bodies", len(view), err)
	}
}
