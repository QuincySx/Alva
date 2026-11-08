// INPUT: Synthetic committed history, real product stores with local engines, the Alva approval/question brokers and native engine pending handles
// OUTPUT: Interaction block projection/settlement/expiry/bounds, page-edge merge, usage blocks, yielded durable waits, pending binding and unified once-only response regressions
// POS: [L7 Hub product] Interaction and usage tests without MyGo, external processes, accounts or models
package product

import (
	"context"
	"errors"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	productstore "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func boolean(value bool) *bool { return &value }
func tokens(n uint64) *uint64  { return &n }
func at(seq uint64, event engine.Event) hub.HistoryEntry {
	return hub.HistoryEntry{Sequence: seq, TurnID: "turn", AttemptID: "attempt", Event: event}
}

func TestInteractionBlocksSettleAsTheirResolutionSays(t *testing.T) {
	command := &engine.ToolDisplay{Kind: engine.ToolCommand, Command: "go test ./..."}
	permission := engine.Event{Kind: engine.PermissionRequest, ID: "p", Name: "exec", Text: "Run the tests", Effect: engine.EffectExecute, Display: command}
	question := engine.Event{Kind: engine.UserInputRequested, ID: "q", Question: &engine.UserInputRequest{Questions: []engine.UserInputQuestion{{ID: "pick", Header: "Pick", Question: "A or B?", Options: []engine.UserInputOption{{Label: "A"}, {Label: "B"}}}, {ID: "note", Question: "Anything else?"}}}}
	for _, test := range []struct {
		name       string
		events     []engine.Event
		id         string
		state      InteractionState
		choices    []string
		text, note string
	}{
		{"waiting", []engine.Event{permission}, "attempt/permission/p", InteractionWaiting, nil, "", ""},
		{"allowed", []engine.Event{permission, {Kind: engine.InteractionResolved, ID: "p", Name: "permission", Resolution: &engine.InteractionResolution{Allowed: boolean(true)}}}, "attempt/permission/p", InteractionAllowed, nil, "", ""},
		{"denied", []engine.Event{permission, {Kind: engine.InteractionResolved, ID: "p", Name: "permission", Resolution: &engine.InteractionResolution{Allowed: boolean(false)}}}, "attempt/permission/p", InteractionDenied, nil, "", ""},
		{"legacy-resolved", []engine.Event{permission, {Kind: engine.InteractionResolved, ID: "p", Name: "permission"}}, "attempt/permission/p", InteractionResolved, nil, "", ""},
		{"expired", []engine.Event{permission, {Kind: engine.Completed, Outcome: engine.Failed}}, "attempt/permission/p", InteractionExpired, nil, "", ""},
		{"cancelled", []engine.Event{permission, {Kind: engine.Completed, Outcome: engine.Cancelled}}, "attempt/permission/p", InteractionCancelled, nil, "", ""},
		{"single-choice", []engine.Event{question, {Kind: engine.InteractionResolved, ID: "q", Name: "user_input", Resolution: &engine.InteractionResolution{Answers: map[string][]string{"pick": {"B"}}}}}, "attempt/question/q", InteractionAnswered, []string{"B"}, "", ""},
		{"multiple-choice", []engine.Event{question, {Kind: engine.InteractionResolved, ID: "q", Name: "user_input", Resolution: &engine.InteractionResolution{Answers: map[string][]string{"pick": {"A", "B"}}}}}, "attempt/question/q", InteractionAnswered, []string{"A", "B"}, "", ""},
		{"free-text", []engine.Event{question, {Kind: engine.InteractionResolved, ID: "q", Name: "user_input", Resolution: &engine.InteractionResolution{Answers: map[string][]string{"pick": {"neither"}, "note": {"[REDACTED]"}}}}}, "attempt/question/q", InteractionAnswered, nil, "neither", "[REDACTED]"},
		{"skipped", []engine.Event{question, {Kind: engine.InteractionResolved, ID: "q", Name: "user_input", Resolution: &engine.InteractionResolution{}}}, "attempt/question/q", InteractionSkipped, nil, "", ""},
		{"answered-without-body", []engine.Event{question, {Kind: engine.InteractionResolved, ID: "q", Name: "user_input", Resolution: &engine.InteractionResolution{Answers: map[string][]string{"note": {}}}}}, "attempt/question/q", InteractionAnswered, nil, "", ""},
	} {
		t.Run(test.name, func(t *testing.T) {
			var entries []hub.HistoryEntry
			for i, event := range test.events {
				entries = append(entries, at(uint64(i+1), event))
			}
			w := NewTranscriptWindow(0)
			w.Reset(chunkOf(entries...))
			b := blockByID(t, w, test.id)
			v := b.Interaction
			if b.Kind != BlockKindInteraction || v == nil || v.State != test.state {
				t.Fatalf("block %+v view %+v", b, v)
			}
			if (test.state == InteractionWaiting) != (b.Status == BlockStatusWaiting) {
				t.Fatal("block status does not follow the request", b.Status)
			}
			if v.Kind == InteractionKindPermission && (v.Tool == nil || v.Tool.Command != "go test ./..." || v.Detail != "Run the tests" || v.Effect != engine.EffectExecute) {
				t.Fatalf("permission summary lost: %+v", v)
			}
			if v.Kind == InteractionKindQuestion && (len(v.Questions) != 2 || v.Questions[0].Text != "A or B?" || len(v.Questions[0].Options) != 2 || v.Questions[0].Other || !v.Questions[1].Other) {
				t.Fatalf("question summary lost: %+v", v.Questions)
			}
			if test.choices != nil || test.text != "" {
				if len(v.Answers) == 0 || strings.Join(v.Answers[0].Choices, ",") != strings.Join(test.choices, ",") || v.Answers[0].Text != test.text {
					t.Fatalf("answer summary %+v", v.Answers)
				}
			}
			if test.name == "answered-without-body" && (len(v.Answers) != 1 || v.Answers[0].QuestionID != "note" || v.Answers[0].Text != "" || v.Answers[0].Choices != nil) {
				t.Fatalf("an answer kept without its body gained one: %+v", v.Answers)
			}
			if test.note != "" && (len(v.Answers) != 2 || v.Answers[1].Text != test.note) {
				t.Fatalf("stored redacted answer was not shown as stored: %+v", v.Answers)
			}
		})
	}
}

func TestInteractionBoundsClipAndMark(t *testing.T) {
	var options []engine.UserInputOption
	for i := 0; i < 16; i++ {
		options = append(options, engine.UserInputOption{Label: strings.Repeat(string(rune('a'+i)), 512)})
	}
	question := engine.Event{Kind: engine.UserInputRequested, ID: "q", Question: &engine.UserInputRequest{Questions: []engine.UserInputQuestion{{ID: "x", Header: strings.Repeat("h", 256), Question: strings.Repeat("长", 1400), Options: options, IsOther: true}}}}
	resolved := engine.Event{Kind: engine.InteractionResolved, ID: "q", Name: "user_input", Resolution: &engine.InteractionResolution{Answers: map[string][]string{"x": {strings.Repeat("y", 4096)}}}}
	permission := engine.Event{Kind: engine.PermissionRequest, ID: "p", Name: strings.Repeat("n", 200), Input: []byte(`{"command":"` + strings.Repeat("c", 4000) + `"}`)}
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(at(1, question), at(2, resolved), at(3, permission)))
	q := blockByID(t, w, "attempt/question/q").Interaction
	if !q.Truncated || len(q.Questions[0].Header) != interactionHeaderBytes || len(q.Questions[0].Text) > interactionQuestionBytes || len(q.Questions[0].Options[0]) != interactionOptionBytes || len(q.Answers[0].Text) != interactionAnswerBytes {
		t.Fatalf("question texts not bounded: %d %d %d %d", len(q.Questions[0].Header), len(q.Questions[0].Text), len(q.Questions[0].Options[0]), len(q.Answers[0].Text))
	}
	if !strings.HasSuffix(q.Questions[0].Text, "长") {
		t.Fatal("question text clipped inside a rune")
	}
	p := blockByID(t, w, "attempt/permission/p").Interaction
	if !p.Truncated || len(p.Name) != interactionNameBytes || len(p.Detail) != interactionDetailBytes {
		t.Fatalf("permission texts not bounded: %d %d", len(p.Name), len(p.Detail))
	}
	if blockBytes(&Block{Interaction: q}) < len(q.Questions[0].Text)+len(q.Answers[0].Text) {
		t.Fatal("interaction text is not counted toward the window bound")
	}
}

func TestInteractionSettlementMergesAcrossThePageEdge(t *testing.T) {
	question := at(2, engine.Event{Kind: engine.UserInputRequested, ID: "q", Question: &engine.UserInputRequest{Questions: []engine.UserInputQuestion{{ID: "pick", Question: "A or B?", Options: []engine.UserInputOption{{Label: "A"}, {Label: "B"}}}}}})
	permission := at(3, engine.Event{Kind: engine.PermissionRequest, ID: "p", Name: "exec"})
	older := []hub.HistoryEntry{{Sequence: 1, TurnID: "turn", Input: "go"}, question, permission}
	newer := []hub.HistoryEntry{
		at(4, engine.Event{Kind: engine.InteractionResolved, ID: "q", Name: "user_input", Resolution: &engine.InteractionResolution{Answers: map[string][]string{"pick": {"B"}}}}),
		at(5, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}),
	}
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(newer...))
	w.ApplyOlder(chunkOf(older...))
	uniqueIDs(t, w)
	q := blockByID(t, w, "attempt/question/q").Interaction
	if q.State != InteractionAnswered || len(q.Questions) != 1 || len(q.Answers) != 1 || strings.Join(q.Answers[0].Choices, ",") != "B" {
		t.Fatalf("merged question %+v", q)
	}
	if p := blockByID(t, w, "attempt/permission/p").Interaction; p.State != InteractionExpired {
		t.Fatalf("older request of a completed attempt still waits: %+v", p)
	}
}

func TestUsageBlockKeepsReportedAndUnreportedCache(t *testing.T) {
	for _, test := range []struct {
		name  string
		usage *engine.Usage
		shown bool
	}{
		{"with-cache", &engine.Usage{InputTokens: 1000, OutputTokens: 50, CachedInputTokens: tokens(800)}, true},
		{"reported-zero", &engine.Usage{InputTokens: 1000, OutputTokens: 50, CachedInputTokens: tokens(0)}, true},
		{"without-cache", &engine.Usage{InputTokens: 1000, OutputTokens: 50}, true},
		{"empty", &engine.Usage{}, false},
		{"legacy-none", nil, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			w := NewTranscriptWindow(0)
			w.Reset(chunkOf(at(1, engine.Event{Kind: engine.Message, ID: "m", Text: "done"}), at(2, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded, Usage: test.usage})))
			blocks := w.Blocks()
			last := blocks[len(blocks)-1]
			if !test.shown {
				if last.Usage != nil || len(blocks) != 1 {
					t.Fatal("usage block without usage", blocks)
				}
				return
			}
			if last.ID != "attempt/usage" || last.Kind != BlockKindStatus || last.Name != "usage" || last.Usage == nil || last.Usage.InputTokens != 1000 || last.Usage.OutputTokens != 50 {
				t.Fatalf("usage block %+v", last)
			}
			if (last.Usage.CachedInputTokens == nil) != (test.usage.CachedInputTokens == nil) || last.Usage.CachedInputTokens != nil && *last.Usage.CachedInputTokens != *test.usage.CachedInputTokens {
				t.Fatal("cache presence changed", last.Usage.CachedInputTokens)
			}
			if last.Usage == test.usage || last.Usage.CachedInputTokens != nil && last.Usage.CachedInputTokens == test.usage.CachedInputTokens {
				t.Fatal("usage block shares the event's value")
			}
		})
	}
}

// yieldedHistory is a durable ask_human wait as the Hub records it: the
// kernel's suspension becomes an error result, then the waiting completion
// names the call.
func yieldedHistory(seq *uint64, turn, attempt string, wait bool) []hub.HistoryEntry {
	next := func(event engine.Event) hub.HistoryEntry {
		*seq++
		return hub.HistoryEntry{Sequence: *seq, TurnID: turn, AttemptID: attempt, Event: event}
	}
	completed := engine.Event{Kind: engine.Completed, Outcome: engine.Waiting, Wait: &engine.WaitCheckpoint{Revision: 3, Requests: 1, ExpiresAt: 1}}
	if wait {
		completed.Wait.Calls = []string{"ask"}
	}
	return []hub.HistoryEntry{
		next(engine.Event{Kind: engine.ToolStart, ID: "ask", Name: "question tool", Display: &engine.ToolDisplay{Kind: engine.ToolOther}}),
		next(engine.Event{Kind: engine.ToolEnd, ID: "ask", Name: "question tool", IsError: true, Text: "durable request pending; execution yielded without replay"}),
		next(completed),
	}
}

func TestYieldedToolWaitsUntilItsTurnContinues(t *testing.T) {
	seq := uint64(0)
	waiting := yieldedHistory(&seq, "turn", "first", true)
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(waiting...))
	if b := blockByID(t, w, "first/ask"); b.Status != BlockStatusWaiting {
		t.Fatal("a call waiting for its answer shows", b.Status)
	}
	seq++
	resumed := hub.HistoryEntry{Sequence: seq, TurnID: "turn", AttemptID: "second", Timeline: &hub.TimelineEvent{Kind: hub.TimelineWaitResumed, Text: "continued"}}
	w.ApplyNewer(chunkOf(resumed))
	if b := blockByID(t, w, "first/ask"); b.Status != BlockStatusCompleted {
		t.Fatal("an answered call shows", b.Status)
	}
	// Across a page edge: the waiting attempt is older than the continuation.
	paged := NewTranscriptWindow(0)
	paged.Reset(chunkOf(resumed))
	paged.ApplyOlder(chunkOf(waiting...))
	if b := blockByID(t, paged, "first/ask"); b.Status != BlockStatusCompleted {
		t.Fatal("a continued call paged in shows", b.Status)
	}
	abandoned := NewTranscriptWindow(0)
	abandoned.Reset(chunkOf(append(append([]hub.HistoryEntry(nil), waiting...), hub.HistoryEntry{Sequence: seq + 1, TurnID: "turn", Timeline: &hub.TimelineEvent{Kind: hub.TimelineWaitAbandoned}})...))
	if b := blockByID(t, abandoned, "first/ask"); b.Status != BlockStatusCancelled {
		t.Fatal("an abandoned wait shows", b.Status)
	}
	// An older receipt without calls keeps its committed failure.
	seq = 0
	legacy := NewTranscriptWindow(0)
	legacy.Reset(chunkOf(yieldedHistory(&seq, "turn", "first", false)...))
	if b := blockByID(t, legacy, "first/ask"); b.Status != BlockStatusFailed {
		t.Fatal("legacy suspension result changed", b.Status)
	}
}

func TestBindInteractionsPlacesEachPathWhereItHappened(t *testing.T) {
	tool := Block{ID: "attempt/call-1", Kind: BlockKindTool, AttemptID: "attempt", TurnID: "turn", SessionID: "s", NativeSessionID: "native", Status: BlockStatusRunning, Tool: &ToolView{ToolDisplay: engine.ToolDisplay{Kind: engine.ToolCommand, Command: "make"}}}
	reply := Block{ID: "attempt/assistant-0", Kind: BlockKindReply, AttemptID: "attempt", Status: BlockStatusRunning}
	external := Block{ID: "attempt/permission/req", Kind: BlockKindInteraction, AttemptID: "attempt", Status: BlockStatusWaiting, Interaction: &InteractionView{Kind: InteractionKindPermission, State: InteractionWaiting}}
	settled := Block{ID: "attempt/question/old", Kind: BlockKindInteraction, AttemptID: "attempt", Status: BlockStatusCompleted, Interaction: &InteractionView{Kind: InteractionKindQuestion, State: InteractionAnswered}}
	blocks := []Block{tool, external, settled, reply}
	if same, actions := BindInteractions(blocks, nil); &same[0] != &blocks[0] || len(actions) != 0 {
		t.Fatal("no pending request copied the blocks")
	}
	approval := approvalInteraction(security.ApprovalRequest{ID: "broker-1", SessionID: "native", CallID: "call-1", ToolName: "shell", InputSummary: "make"})
	child := questionInteraction(interaction.Question{ID: "question-1", SessionID: "child-native", RunID: "run", CallID: "call-9", Prompt: "Which?"})
	native := Interaction{View: InteractionView{Kind: InteractionKindPermission, State: InteractionWaiting}, BlockID: "attempt/permission/req", path: pathEngine, request: "req"}
	stale := Interaction{View: InteractionView{Kind: InteractionKindQuestion, State: InteractionWaiting}, BlockID: "attempt/question/old", path: pathEngine, request: "old"}
	bound, actions := BindInteractions(blocks, []Interaction{approval, child, native, stale})
	var ids []string
	for _, b := range bound {
		ids = append(ids, b.ID)
	}
	want := []string{"attempt/call-1", pendingBlockID(approval), "attempt/permission/req", "attempt/question/old", "attempt/assistant-0", pendingBlockID(child)}
	if strings.Join(ids, " ") != strings.Join(want, " ") {
		t.Fatalf("placement %v, want %v", ids, want)
	}
	if bound[0].Status != BlockStatusWaiting || blocks[0].Status != BlockStatusRunning {
		t.Fatal("asking call does not show waiting, or the shared block changed")
	}
	if card := bound[1]; card.Kind != BlockKindInteraction || card.AttemptID != "attempt" || card.Interaction.Tool == nil || card.Interaction.Tool.Command != "make" || card.Interaction.Name != "shell" {
		t.Fatalf("anchored card %+v %+v", card, card.Interaction)
	}
	if !actions[pendingBlockID(approval)].Same(approval) || !actions[pendingBlockID(child)].Same(child) || !actions["attempt/permission/req"].Same(native) {
		t.Fatal("actionable requests not bound to their blocks", actions)
	}
	if _, bound := actions["attempt/question/old"]; bound {
		t.Fatal("a stale pending read reopened a settled request")
	}
}

// brokerEngine stands in for the Alva adapter: its permission callback is
// the product's native approval route and it waits on the shared brokers.
type brokerEngine struct {
	service *HubService
	native  string
	ask     func(context.Context) error
	done    chan error
}

func (e *brokerEngine) Execute(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: e.native}); err != nil {
		return err
	}
	err := e.ask(ctx)
	e.done <- err
	return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
}
func (*brokerEngine) Cancel(context.Context, string) error { return nil }
func (e *brokerEngine) RespondPermission(ctx context.Context, root, request string, decision engine.PermissionDecision) error {
	return e.service.RespondNativeApproval(ctx, "alva", root, request, decision.Allow)
}
func (*brokerEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Cancel: true, PermissionCallback: true}
}

func brokerFixture(t *testing.T, ask func(*HubService, string) func(context.Context) error) (*HubService, string, *brokerEngine, []Interaction) {
	t.Helper()
	// The broker engine takes the Alva route itself, so this service has no
	// other engine registered under it.
	directory := filepath.Join(t.TempDir(), "private")
	store, err := productstore.Open(t.Context(), filepath.Join(directory, "product"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	repo, err := session.Open(t.Context(), filepath.Join(directory, "native"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = repo.Close() })
	runtime := hub.New(store)
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = runtime.Close(ctx)
	})
	s, err := NewHubService(store, runtime, repo, hub.Target{EngineID: "alva", ModelID: "fixture", ProviderID: "local", AccountSource: "test", BillingRoute: "no-billing"}, directory, true)
	if err != nil {
		t.Fatal(err)
	}
	s.Approvals = host.NewApprovalBroker(8)
	t.Cleanup(func() { _ = s.Approvals.Close() })
	questions, err := host.NewQuestionBroker(8)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(questions.Close)
	s.Questions = questions
	owner, err := s.Create(t.Context(), "broker interactions", "")
	if err != nil {
		t.Fatal(err)
	}
	e := &brokerEngine{service: s, native: "native-broker-owner", done: make(chan error, 1)}
	e.ask = ask(s, e.native)
	if err = s.Runtime.Register(s.DefaultTarget.EngineID, e); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Repository.Create(t.Context(), owner.ID, e.native, ""); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Submit(t.Context(), owner.ID, "broker fixture", nil); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		items, err := s.PendingInteractions(t.Context(), owner.ID)
		if err != nil {
			t.Fatal(err)
		}
		if len(items) == 1 {
			return s, owner.ID, e, items
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("broker request never became a pending interaction")
	return nil, "", nil, nil
}

func waitInactive(t *testing.T, s *HubService) {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for s.Runtime.HasActive() && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
}

func TestRespondInteractionAlvaApprovalIsOnceOnlyAndLateClicksFail(t *testing.T) {
	for _, allow := range []bool{true, false} {
		decided := make(chan security.ApprovalDecision, 1)
		s, id, e, items := brokerFixture(t, func(s *HubService, native string) func(context.Context) error {
			return func(ctx context.Context) error {
				decision, err := s.Approvals.Approve(ctx, security.ApprovalRequest{SessionID: native, RunID: "run", CallID: "call", ToolName: "shell", InputSummary: "make test", Effect: abi.Effect{Kind: abi.EffectExecute}})
				decided <- decision
				return err
			}
		})
		item := items[0]
		if item.View.Kind != InteractionKindPermission || item.CallID != "call" || item.View.Detail != "make test" || item.View.Effect != engine.EffectExecute {
			t.Fatalf("Alva approval view %+v", item)
		}
		if err := s.RespondInteraction(t.Context(), "other-session", item, InteractionResponse{Allow: true}); err == nil {
			t.Fatal("a request answered for another session")
		}
		if err := s.RespondInteraction(t.Context(), id, item, InteractionResponse{Allow: allow}); err != nil {
			t.Fatal(err)
		}
		if decision := <-decided; decision.Allowed != allow || decision.Scope != security.Once {
			t.Fatal("decision changed", decision)
		}
		if err := s.RespondInteraction(t.Context(), id, item, InteractionResponse{Allow: !allow}); !errors.Is(err, host.ErrApprovalExpired) {
			t.Fatal("a second click on the same request did not fail explicitly", err)
		}
		if err := s.RespondApproval(t.Context(), id, item.request, !allow); !errors.Is(err, host.ErrApprovalExpired) {
			t.Fatal("the panel's entry decided a request the card already decided", err)
		}
		<-e.done
		waitInactive(t, s)
		if err := s.RespondInteraction(t.Context(), id, item, InteractionResponse{Allow: true}); !errors.Is(err, host.ErrApprovalExpired) {
			t.Fatal("a late click after the run ended did not fail explicitly", err)
		}
	}
}

func TestRespondInteractionAlvaQuestionChoicesAndText(t *testing.T) {
	for _, test := range []struct {
		name     string
		multi    bool
		answer   InteractionAnswer
		selected []int
		text     string
		invalid  bool
	}{
		{"single", false, InteractionAnswer{Choices: []string{"B"}}, []int{2}, "", false},
		{"by-index", false, InteractionAnswer{Options: []int{2}}, []int{3}, "", false},
		{"index-not-offered", false, InteractionAnswer{Options: []int{3}}, nil, "", true},
		{"multiple", true, InteractionAnswer{Choices: []string{"A", "C"}}, []int{1, 3}, "", false},
		{"free-text", false, InteractionAnswer{Text: "my own words"}, nil, "my own words", false},
		{"not-offered", false, InteractionAnswer{Choices: []string{"Z"}}, nil, "", true},
	} {
		t.Run(test.name, func(t *testing.T) {
			answered := make(chan interaction.Answer, 1)
			s, id, e, items := brokerFixture(t, func(s *HubService, native string) func(context.Context) error {
				return func(ctx context.Context) error {
					answer, err := s.Questions.Ask(ctx, interaction.Question{ID: "q-1", SessionID: native, RunID: "run", CallID: "ask-call", Prompt: "Pick", Options: []interaction.Choice{{Label: "A"}, {Label: "B"}, {Label: "C"}}, MultiSelect: test.multi})
					answered <- answer
					return err
				}
			})
			item := items[0]
			if item.View.Kind != InteractionKindQuestion || len(item.View.Questions) != 1 || item.View.Questions[0].Multiple != test.multi || !item.View.Questions[0].Other || item.CanSkip || item.CallID != "ask-call" {
				t.Fatalf("Alva question view %+v", item)
			}
			err := s.RespondInteraction(t.Context(), id, item, InteractionResponse{Answers: map[string]InteractionAnswer{"q-1": test.answer}})
			if test.invalid {
				if err == nil {
					t.Fatal("an answer that was not offered was accepted")
				}
				if pending, _ := s.PendingInteractions(t.Context(), id); len(pending) != 1 {
					t.Fatal("a rejected answer consumed the question")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			answer := <-answered
			if !slices.Equal(answer.Selected, test.selected) || (answer.Text == nil) != (test.text == "") || answer.Text != nil && *answer.Text != test.text {
				t.Fatal("broker received", answer.Selected, answer.Text)
			}
			if err := s.RespondInteraction(t.Context(), id, item, InteractionResponse{Answers: map[string]InteractionAnswer{"q-1": test.answer}}); !errors.Is(err, host.ErrQuestionExpired) {
				t.Fatal("a second answer did not fail explicitly", err)
			}
			<-e.done
		})
	}
}

func TestRespondInteractionNativeEnginePathAndBlockBinding(t *testing.T) {
	s, id, e, _ := pendingInteractionFixture(t)
	items, err := s.PendingInteractions(t.Context(), id)
	if err != nil || len(items) != 2 {
		t.Fatal(items, err)
	}
	chunk, err := (&TranscriptService{Store: s.Store}).Tail(t.Context(), id)
	if err != nil {
		t.Fatal(err)
	}
	w := NewTranscriptWindow(0)
	w.Reset(chunk)
	bound, actions := BindInteractions(w.Blocks(), items)
	if len(bound) != len(w.Blocks()) || len(actions) != 2 {
		t.Fatal("native requests floated instead of taking their own history blocks", len(bound), len(w.Blocks()), actions)
	}
	var permission, question Interaction
	for _, item := range items {
		if item.View.Kind == InteractionKindPermission {
			permission = item
		} else {
			question = item
		}
	}
	if !question.CanSkip || permission.BlockID == question.BlockID {
		t.Fatal("native question identity or skip", permission.BlockID, question.BlockID)
	}
	if err := s.RespondInteraction(t.Context(), id, permission, InteractionResponse{Allow: true}); err != nil {
		t.Fatal(err)
	}
	if err := s.RespondInteraction(t.Context(), id, permission, InteractionResponse{Allow: true}); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("a second permission click did not fail explicitly", err)
	}
	if err := s.RespondInteraction(t.Context(), id, question, InteractionResponse{Answers: map[string]InteractionAnswer{"choice": {Choices: []string{"Blue"}}}}); err != nil {
		t.Fatal(err)
	}
	waitInactive(t, s)
	if e.permissions.Load() != 1 || e.questions.Load() != 1 {
		t.Fatal("responses crossed paths", e.permissions.Load(), e.questions.Load())
	}
	if err := s.RespondInteraction(t.Context(), id, question, InteractionResponse{}); err == nil {
		t.Fatal("a late answer after the run ended was accepted")
	}
}

func TestYieldedCallsSettleOnTheirOwnWait(t *testing.T) {
	seq := uint64(0)
	first := yieldedHistory(&seq, "turn", "first", true)
	seq++
	resumed := hub.HistoryEntry{Sequence: seq, TurnID: "turn", AttemptID: "second", Timeline: &hub.TimelineEvent{Kind: hub.TimelineWaitResumed}}
	next := func(attempt string, event engine.Event) hub.HistoryEntry {
		seq++
		return hub.HistoryEntry{Sequence: seq, TurnID: "turn", AttemptID: attempt, Event: event}
	}
	// The continuation waits again in the same Turn.
	second := []hub.HistoryEntry{
		next("second", engine.Event{Kind: engine.ToolStart, ID: "ask2", Name: "question tool"}),
		next("second", engine.Event{Kind: engine.ToolEnd, ID: "ask2", Name: "question tool", IsError: true, Text: "durable request pending"}),
		next("second", engine.Event{Kind: engine.Completed, Outcome: engine.Waiting, Wait: &engine.WaitCheckpoint{Revision: 5, Requests: 1, ExpiresAt: 1, Calls: []string{"ask2"}}}),
	}
	history := append(append(append([]hub.HistoryEntry(nil), first...), resumed), second...)
	statuses := func(w *TranscriptWindow) string {
		return string(blockByID(t, w, "first/ask").Status) + "," + string(blockByID(t, w, "second/ask2").Status)
	}
	whole := NewTranscriptWindow(0)
	whole.Reset(chunkOf(history...))
	if got := statuses(whole); got != "completed,waiting" {
		t.Fatal("a second wait in the same Turn took the first wait's continuation:", got)
	}
	// A later abandon (after a crash in the continuation) settles only the
	// call still waiting; the answered call stays completed.
	seq++
	abandoned := hub.HistoryEntry{Sequence: seq, TurnID: "turn", AttemptID: "second", Timeline: &hub.TimelineEvent{Kind: hub.TimelineWaitAbandoned}}
	whole.ApplyNewer(chunkOf(abandoned))
	if got := statuses(whole); got != "completed,cancelled" {
		t.Fatal("abandoning the later wait changed the answered call:", got)
	}
	// The same facts split across a page edge settle the same way.
	for cut := 1; cut < len(history); cut++ {
		paged := NewTranscriptWindow(0)
		paged.Reset(chunkOf(append(append([]hub.HistoryEntry(nil), history[cut:]...), abandoned)...))
		paged.ApplyOlder(chunkOf(history[:cut]...))
		if got := statuses(paged); got != "completed,cancelled" {
			t.Fatal("page edge at", cut, "settled", got)
		}
	}
}

// longLabelEngine asks one native question whose first option label is
// longer than the view shows, and reports what the product sent.
type longLabelEngine struct {
	ready    chan struct{}
	received chan engine.UserInputResponse
	request  engine.UserInputRequest
}

func (e *longLabelEngine) Execute(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
	for _, event := range []engine.Event{
		{Kind: engine.SessionStarted, SessionID: "long-native"},
		{Kind: engine.UserInputRequested, SessionID: "long-native", ID: "long-question", Question: &e.request},
	} {
		if err := emit(event); err != nil {
			return err
		}
	}
	close(e.ready)
	<-ctx.Done()
	return ctx.Err()
}
func (*longLabelEngine) Cancel(context.Context, string) error { return nil }
func (*longLabelEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (e *longLabelEngine) RespondUserInput(_ context.Context, _, _ string, response engine.UserInputResponse) error {
	e.received <- response
	return nil
}
func (*longLabelEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Cancel: true, UserInputCallback: true}
}

func TestRespondInteractionSendsWholeOptionLabels(t *testing.T) {
	long := strings.Repeat("选", 90) // 270 bytes: offered whole, shown clipped
	for _, test := range []struct {
		name   string
		answer InteractionAnswer
	}{
		{"by-index", InteractionAnswer{Options: []int{0}}},
		{"by-shown-label", InteractionAnswer{Choices: []string{strings.Repeat("选", 85)}}},
	} {
		t.Run(test.name, func(t *testing.T) {
			s, _ := fixture(t)
			owner, err := s.Create(t.Context(), "long labels", "")
			if err != nil {
				t.Fatal(err)
			}
			target := s.DefaultTarget
			target.EngineID = "long-labels"
			s.Targets = append(s.Targets, target)
			if _, err := s.Store.Switch(t.Context(), owner.ID, target, "local fixture"); err != nil {
				t.Fatal(err)
			}
			e := &longLabelEngine{ready: make(chan struct{}), received: make(chan engine.UserInputResponse, 1), request: engine.UserInputRequest{Questions: []engine.UserInputQuestion{{ID: "pick", Question: "Which?", Options: []engine.UserInputOption{{Label: long}, {Label: "B"}}}}}}
			if err := s.Runtime.Register(target.EngineID, e); err != nil {
				t.Fatal(err)
			}
			if _, _, err := s.Runtime.Start(t.Context(), owner.ID, "fixture"); err != nil {
				t.Fatal(err)
			}
			<-e.ready
			items, err := s.PendingInteractions(t.Context(), owner.ID)
			if err != nil || len(items) != 1 {
				t.Fatal(items, err)
			}
			if shown := items[0].View.Questions[0].Options[0]; len(shown) != 255 || !items[0].View.Truncated {
				t.Fatalf("view did not clip the long label for display: %d bytes", len(shown))
			}
			if err := s.RespondInteraction(t.Context(), owner.ID, items[0], InteractionResponse{Answers: map[string]InteractionAnswer{"pick": test.answer}}); err != nil {
				t.Fatal("whole label rejected", err)
			}
			if sent := <-e.received; len(sent.Answers["pick"]) != 1 || sent.Answers["pick"][0] != long || sent.ValidateFor(e.request) != nil {
				t.Fatal("the whole label was not sent", sent.Answers)
			}
			if err := s.Runtime.CancelAndWait(t.Context(), owner.ID); err != nil {
				t.Fatal(err)
			}
		})
	}
}
