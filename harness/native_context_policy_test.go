// INPUT: context, encoding/json, errors, fmt, filepath, reflect, strings, testing, agent/core, kernel/abi, llm/wire
// OUTPUT: Bounded native source isolation, restart, protocol and oversized-history integration regressions
// POS: [L5 harness context testing] Proves working views bypass full restoration without silently losing the current turn or original facts.
package harness

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type sourceOnlyReader string

func (reader sourceOnlyReader) ID() string { return string(reader) }
func (sourceOnlyReader) Messages(context.Context) ([]wire.Message, error) {
	panic("bounded source restored whole conversation")
}
func addContextMessages(t *testing.T, log *session.Session, texts ...string) {
	t.Helper()
	batch := make([]abi.SessionRecord, 0, len(texts))
	for _, text := range texts {
		batch = append(batch, contextConversation(text))
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
	repo, err := session.Open(ctx, dir)
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
	source, err := bindContextSource(ctx, repo, "owner", log.ID())
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
	if _, err := bindContextSource(ctx, repo, "foreign", log.ID()); !errors.Is(err, session.ErrNotFound) {
		t.Fatal("foreign owner accepted", err)
	}
	if _, err := source.Load(ctx, sourceOnlyReader("foreign")); err == nil {
		t.Fatal("source accepted foreign runtime")
	}
	var missingReader *session.Session
	if _, err := source.Load(ctx, missingReader); err == nil {
		t.Fatal("source accepted a typed-nil reader")
	}
	all, err := log.Messages(ctx)
	if err != nil || len(all) != 256 {
		t.Fatal("source changed original log", len(all), err)
	}
	if err := repo.Close(); err != nil {
		t.Fatal(err)
	}
	repo, err = session.Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	source, err = bindContextSource(ctx, repo, "owner", "native-source")
	if err != nil {
		t.Fatal(err)
	}
	reopened, err := source.Load(ctx, sourceOnlyReader("native-source"))
	if err != nil || !reflect.DeepEqual(view, reopened) {
		t.Fatal("restart changed same high-water projection", err)
	}
}

func TestNativeContextSourceIncludesWholeLatestToolTurn(t *testing.T) {
	repo, log := contextRepoSession(t)
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
	source, err := bindContextSource(t.Context(), repo, "owner", "native")
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
	repo, log := contextRepoSession(t)
	// Real original bytes exceed the complete 64 MiB restore budget. None of
	// these old bodies needs to be decoded for a later small bounded turn.
	large := strings.Repeat("x", 16<<20)
	for i := 0; i < 4; i++ {
		addContextMessages(t, log, large)
	}
	large = ""
	addContextMessages(t, log, "latest before the smaller tail")
	earlySource, err := bindContextSource(t.Context(), repo, "owner", log.ID())
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
	source, err := bindContextSource(t.Context(), repo, "owner", log.ID())
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
	meta, err := repo.Page(t.Context(), "owner", log.ID(), 0, 1, 0)
	if err != nil || meta.HighWater == 0 {
		t.Fatal("facts missing", err)
	}

}

func bindContextSource(ctx context.Context, repo *session.Repository, owner, id string) (abi.ContextSource, error) {
	if _, err := repo.OpenSession(ctx, owner, id); err != nil {
		return nil, err
	}
	return &nativeContextSource{repository: repo, owner: owner, id: id}, nil
}
func contextConversation(text string) abi.SessionRecord {
	message := wire.TextMessage(wire.RoleUser, text)
	return abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})
}
func contextRepoSession(t *testing.T) (*session.Repository, *session.Session) {
	t.Helper()
	repo, err := session.Open(t.Context(), filepath.Join(t.TempDir(), "facts"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { repo.Close() })
	log, err := repo.Create(t.Context(), "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	return repo, log
}

func TestNativeContextRefusesToSplitAnOversizedLatestTurn(t *testing.T) {
	repo, log := contextRepoSession(t)
	addContextMessages(t, log, "latest user input")
	var batch []abi.SessionRecord
	for range contextMaxMessages {
		m := wire.TextMessage(wire.RoleAssistant, "same unfinished user turn")
		batch = append(batch, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &m}))
	}
	if _, err := log.AppendRecords(t.Context(), batch); err != nil {
		t.Fatal(err)
	}
	source := &nativeContextSource{repository: repo, owner: "owner", id: log.ID()}
	if messages, err := source.Load(t.Context(), sourceOnlyReader(log.ID())); err == nil || messages != nil {
		t.Fatal("source silently split the current turn", len(messages), err)
	}
}
