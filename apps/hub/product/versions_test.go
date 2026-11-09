// INPUT: A scripted stub engine with and without native fork in a real product service, plus hand-built version lines
// OUTPUT: Fork degradation to the Turn index with a visible notice, native fork intent reaching the engine, a refused orphan fork intent and the version switcher derivation
// POS: [L7 Hub product] Proves the product rules of edit/regenerate/fork independent of any real engine: what an engine without fork is told, and how the "‹2/3›" control is numbered
package product

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func sendOnce(t *testing.T, stub *stubEngine, s *HubService, id, prompt string) TurnStart {
	t.Helper()
	start, err := s.Turns.Send(context.Background(), id, prompt, nil)
	if err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, prompt)
	stub.gate <- struct{}{}
	if settled(t, s, id) != engine.Succeeded {
		t.Fatal("run must succeed")
	}
	return start
}

func TestEditOnAnEngineWithoutForkTellsTheModelOnlyTheIndexAndSaysSo(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true}, started: make(chan string, 8), gate: make(chan struct{}, 8)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	sendOnce(t, stub, s, c.ID, "first")
	second := sendOnce(t, stub, s, c.ID, "second")
	if _, err := s.Turns.Edit(ctx, c.ID, second.TurnID, "second-edited"); err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "second-edited")
	stub.gate <- struct{}{}
	settled(t, s, c.ID)
	prompts := stub.promptList()
	edited := prompts[2]
	if !strings.Contains(edited, "[Hub handoff") || !strings.Contains(edited, `"prompt":"first"`) || strings.Contains(edited, `"prompt":"second"`) {
		t.Fatal("the new version of an engine without fork gets the index of the kept Turns only", edited)
	}
	if stub.forks[2] != nil {
		t.Fatal("an engine without the capability is never asked to fork", stub.forks[2])
	}
	chunk, err := (&TranscriptService{Store: s.Store}).Tail(ctx, c.ID)
	if err != nil {
		t.Fatal(err)
	}
	window := NewTranscriptWindow(0)
	window.Reset(chunk)
	notice := false
	for _, b := range window.Blocks() {
		notice = notice || b.Kind == BlockKindStatus && b.Name == "notice" && strings.Contains(b.Text, "此引擎不能原生分叉")
	}
	if !notice {
		t.Fatal("the reader must be told the model sees only a summary")
	}
	versions, err := s.Conversations.Versions(ctx, c.ID)
	if err != nil || len(versions) != 1 {
		t.Fatal(versions, err)
	}
}

func TestEditOnAForkingEngineAsksForTheCutCopyAndNeverTheIndex(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true, Fork: true}, started: make(chan string, 8), gate: make(chan struct{}, 8)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	sendOnce(t, stub, s, c.ID, "first")
	second := sendOnce(t, stub, s, c.ID, "second")
	sendOnce(t, stub, s, c.ID, "third")
	if _, err := s.Turns.Edit(ctx, c.ID, second.TurnID, "second-edited"); err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "second-edited")
	stub.gate <- struct{}{}
	settled(t, s, c.ID)
	fork := stub.forks[3]
	if fork == nil || fork.Session != "stub-native-1" || fork.AnchorTurn != "stub-turn-2" || fork.AnchorKeep || fork.OwnerID != c.ID {
		t.Fatal("the engine must be asked to cut before the edited Turn", fork)
	}
	if strings.Contains(stub.promptList()[3], "[Hub handoff") {
		t.Fatal("a native fork needs no index")
	}
	// The fork intent is spent once the engine reported its new native session:
	// the next message on the new version resumes it, no fork again.
	sendOnce(t, stub, s, c.ID, "fourth")
	if stub.forks[4] != nil {
		t.Fatal("the fork is requested once", stub.forks[4])
	}
	detail, err := s.Conversations.Detail(ctx, c.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, _ := ActiveBranch(detail)
	if branch.Fork != nil || branch.NativeSessionID != "stub-native-2" {
		t.Fatal("the new version has its own native session", branch)
	}
}

func TestForkOfAnEngineWithoutForkIsDegradedAndCarriesTheIndex(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true}, started: make(chan string, 8), gate: make(chan struct{}, 8)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	sendOnce(t, stub, s, c.ID, "first")
	second := sendOnce(t, stub, s, c.ID, "second")
	sendOnce(t, stub, s, c.ID, "third")
	forked, err := s.Conversations.Fork(ctx, c.ID, second.TurnID)
	if err != nil {
		t.Fatal(err)
	}
	if !forked.Degraded || forked.Session.ForkedFrom == nil || forked.Session.ForkedFrom.TurnID != second.TurnID {
		t.Fatal("an engine without fork must report a degraded fork", forked)
	}
	sendOnce(t, stub, s, forked.Session.ID, "new topic")
	prompt := stub.promptList()[3]
	if !strings.Contains(prompt, `"prompt":"first"`) || !strings.Contains(prompt, `"prompt":"second"`) || strings.Contains(prompt, `"prompt":"third"`) || !strings.HasSuffix(prompt, "new topic") {
		t.Fatal("the fork's first message carries the index of the kept Turns, not later ones", prompt)
	}
	// Later messages need no index: the native session carries the context.
	sendOnce(t, stub, s, forked.Session.ID, "again")
	if strings.Contains(stub.promptList()[4], "[Hub handoff") {
		t.Fatal("the index is delivered once")
	}
}

func TestOrphanForkIntentIsRefusedInsteadOfSilentlyLosingContext(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true}, started: make(chan string, 8), gate: make(chan struct{}, 8)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	first := sendOnce(t, stub, s, c.ID, "first")
	// Written for an engine that could fork, then run on one that cannot.
	forked, err := s.Store.ForkSession(ctx, c.ID, hub.SessionFork{TurnID: first.TurnID, NativeFork: true})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Turns.Send(ctx, forked.ID, "hello", nil); err != nil {
		t.Fatal(err)
	}
	if got := settled(t, s, forked.ID); got != engine.Failed {
		t.Fatal("the Turn must fail loudly", got)
	}
	if stub.executed != 1 {
		t.Fatal("the engine must not run a fresh conversation in place of the fork", stub.executed)
	}
}

func TestVersionSwitchesNumberDistinctTurnsPerPosition(t *testing.T) {
	// line1: T1 T2 T3 ; line2 edits T2 -> T2b ; line3 regenerates line2's T3b.
	lines := []hub.Line{
		{ID: "l1", TouchedAt: 10, Groups: []hub.LineGroup{{Group: "g2", TurnID: "T2", Seq: 2}}},
		{ID: "l2", TouchedAt: 20, Groups: []hub.LineGroup{{Group: "g2", TurnID: "T2b", Seq: 5}, {Group: "g3", TurnID: "T3b", Seq: 6}}},
		{ID: "l3", TouchedAt: 30, Groups: []hub.LineGroup{{Group: "g2", TurnID: "T2b", Seq: 5}, {Group: "g3", TurnID: "T3c", Seq: 7}}},
	}
	got := versionSwitches(lines, "l3")
	two, three := got["T2b"], got["T3c"]
	if two.Count != 2 || two.Index != 2 || len(two.Choices) != 2 || two.Choices[0].TurnID != "T2" || two.Choices[0].LineID != "l1" || two.Choices[1].LineID != "l3" || !two.Choices[1].Active {
		t.Fatal("position two", two)
	}
	if three.Count != 2 || three.Index != 2 || three.Choices[0].TurnID != "T3b" || three.Choices[0].LineID != "l2" {
		t.Fatal("position three", three)
	}
	if len(got) != 2 {
		t.Fatal("every switcher belongs to a Turn of the shown version", got)
	}
	// From the first line only position two exists, and the shared T2b turn is
	// represented by the most recently used of the lines that show it.
	first := versionSwitches(lines, "l1")
	if len(first) != 1 || first["T2"].Index != 1 || first["T2"].Count != 2 || first["T2"].Choices[1].LineID != "l3" {
		t.Fatal("from the original", first)
	}
	if versionSwitches(lines, "missing") != nil {
		t.Fatal("an unknown active line has no switchers")
	}
	if versionSwitches([]hub.Line{{ID: "solo", Groups: []hub.LineGroup{{Group: "g", TurnID: "T", Seq: 1}}}}, "solo") != nil && len(versionSwitches([]hub.Line{{ID: "solo", Groups: []hub.LineGroup{{Group: "g", TurnID: "T", Seq: 1}}}}, "solo")) != 0 {
		t.Fatal("a single version shows no switcher")
	}
}

func TestVersionAndQueueErrorsAreTyped(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true}, started: make(chan string, 8), gate: make(chan struct{}, 8)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	if _, err := s.Turns.Edit(ctx, c.ID, "turn-none", "x"); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal(err)
	}
	if _, err := s.Turns.Edit(ctx, c.ID, "turn-none", ""); err == nil {
		t.Fatal("empty edit")
	}
	if err := s.Turns.SelectVersion(ctx, c.ID, "line-none"); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal(err)
	}
	if _, err := s.Conversations.Fork(ctx, c.ID, "turn-none"); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal(err)
	}
	if versions, err := s.Conversations.Versions(ctx, c.ID); err != nil || versions != nil {
		t.Fatal("a conversation without versions has no switchers", versions, err)
	}
}

func forkAnchorOf(t *testing.T, s *HubService, id string) *hub.BranchFork {
	t.Helper()
	detail, err := s.Conversations.Detail(context.Background(), id)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	return branch.Fork
}

func TestForkCutIsFixedWhenRecordedEvenIfTheNewVersionFailsAndTheSourceAdvances(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true, Fork: true}, started: make(chan string, 8), gate: make(chan struct{}, 16)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	sendOnce(t, stub, s, c.ID, "first")
	second := sendOnce(t, stub, s, c.ID, "second")
	sendOnce(t, stub, s, c.ID, "third")
	// The new version fails before its engine reports a native session, so the
	// fork intent stays recorded and unspent.
	stub.failNext = true
	edit, err := s.Turns.Edit(ctx, c.ID, second.TurnID, "second-edited")
	if err != nil {
		t.Fatal(err)
	}
	if got := settled(t, s, c.ID); got != engine.Failed {
		t.Fatal("the new version must fail at startup", got)
	}
	recorded := forkAnchorOf(t, s, c.ID)
	if recorded == nil || recorded.AnchorTurn != "stub-turn-2" || recorded.AnchorKeep {
		t.Fatal("the cut is anchored at the second Turn's own native turn", recorded)
	}
	// Back on the original version the conversation moves on by two Turns.
	versions, err := s.Conversations.Versions(ctx, c.ID)
	if err != nil || versions[edit.TurnID].Count != 2 {
		t.Fatal("two versions expected", versions, err)
	}
	original := versions[edit.TurnID].Choices[0].LineID
	edited := versions[edit.TurnID].Choices[1].LineID
	if err = s.Turns.SelectVersion(ctx, c.ID, original); err != nil {
		t.Fatal(err)
	}
	sendOnce(t, stub, s, c.ID, "fourth")
	sendOnce(t, stub, s, c.ID, "fifth")
	if err = s.Turns.SelectVersion(ctx, c.ID, edited); err != nil {
		t.Fatal(err)
	}
	if again := forkAnchorOf(t, s, c.ID); again == nil || *again != *recorded {
		t.Fatal("switching versions must not touch the recorded cut", again, recorded)
	}
	// Retrying on the new version asks the engine for exactly the recorded cut,
	// not one recomputed from the source as it is now (five Turns).
	sendOnce(t, stub, s, c.ID, "retry")
	asked := stub.forks[len(stub.forks)-1]
	if asked == nil || asked.AnchorTurn != "stub-turn-2" || asked.AnchorKeep || asked.Session != "stub-native-1" || asked.OwnerID != c.ID {
		t.Fatal("the engine must be asked for the cut recorded before the source advanced", asked)
	}
}

func TestSessionForkCutIsFixedWhenRecordedEvenIfTheSourceAdvances(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true, Fork: true}, started: make(chan string, 8), gate: make(chan struct{}, 16)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	sendOnce(t, stub, s, c.ID, "first")
	second := sendOnce(t, stub, s, c.ID, "second")
	forked, err := s.Conversations.Fork(ctx, c.ID, second.TurnID)
	if err != nil || forked.Degraded {
		t.Fatal(forked, err)
	}
	recorded := forkAnchorOf(t, s, forked.Session.ID)
	if recorded == nil || recorded.AnchorTurn != "stub-turn-2" || !recorded.AnchorKeep {
		t.Fatal("the fork keeps through the second Turn's own native turn", recorded)
	}
	// The source goes on before the fork sends its first message.
	sendOnce(t, stub, s, c.ID, "third")
	sendOnce(t, stub, s, c.ID, "fourth")
	sendOnce(t, stub, s, forked.Session.ID, "fork topic")
	asked := stub.forks[len(stub.forks)-1]
	if asked == nil || asked.AnchorTurn != "stub-turn-2" || !asked.AnchorKeep || asked.OwnerID != c.ID {
		t.Fatal("the fork must cut where it was recorded, not where the source is now", asked)
	}
}

func TestAnEngineThatCannotReachTheCutDegradesToTheIndexWithANotice(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true, Fork: true}, started: make(chan string, 8), gate: make(chan struct{}, 16), checkErr: errors.New("cut lies before the readable tail")}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	sendOnce(t, stub, s, c.ID, "first")
	second := sendOnce(t, stub, s, c.ID, "second")
	if _, err := s.Turns.Edit(ctx, c.ID, second.TurnID, "second-edited"); err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "second-edited")
	stub.gate <- struct{}{}
	settled(t, s, c.ID)
	if stub.forks[2] != nil || !strings.Contains(stub.promptList()[2], `"prompt":"first"`) {
		t.Fatal("an unreachable cut must not fork; the model gets the index", stub.forks[2])
	}
	if !transcriptHasNotice(t, s, c.ID, "此引擎不能原生分叉") {
		t.Fatal("the reader must be told the model sees only a summary")
	}
}

func transcriptHasNotice(t *testing.T, s *HubService, id, text string) bool {
	t.Helper()
	chunk, err := (&TranscriptService{Store: s.Store}).Tail(context.Background(), id)
	if err != nil {
		t.Fatal(err)
	}
	window := NewTranscriptWindow(0)
	window.Reset(chunk)
	for _, b := range window.Blocks() {
		if b.Kind == BlockKindStatus && b.Name == "notice" && strings.Contains(b.Text, text) {
			return true
		}
	}
	return false
}

func TestDegradedSessionForkShowsTheNoticeOnItsFirstTurn(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true, Fork: true}, started: make(chan string, 8), gate: make(chan struct{}, 16), checkErr: errors.New("cut lies before the readable tail")}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	first := sendOnce(t, stub, s, c.ID, "first")
	forked, err := s.Conversations.Fork(ctx, c.ID, first.TurnID)
	if err != nil || !forked.Degraded {
		t.Fatal("a refused cut must degrade the fork", forked, err)
	}
	if transcriptHasNotice(t, s, forked.Session.ID, "此引擎不能原生分叉") {
		t.Fatal("the notice belongs to the first Turn, not to an empty conversation")
	}
	sendOnce(t, stub, s, forked.Session.ID, "new topic")
	if !strings.Contains(stub.promptList()[1], `"prompt":"first"`) || stub.forks[1] != nil || !transcriptHasNotice(t, s, forked.Session.ID, "此引擎不能原生分叉") {
		t.Fatal("the first message carries the index and the reader is told", stub.promptList()[1])
	}
}

func TestAForkTheEngineCannotReachAtExecuteTimeDegradesInPlaceAndTheTurnSucceeds(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true, Fork: true}, started: make(chan string, 8), gate: make(chan struct{}, 16)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	sendOnce(t, stub, s, c.ID, "first")
	second := sendOnce(t, stub, s, c.ID, "second")
	// The preflight agrees, but at Execute time the anchor is gone (the source
	// was rolled back or replaced): the same Turn must still succeed, from the
	// index, with the notice.
	stub.mu.Lock()
	stub.unreachable = true
	stub.mu.Unlock()
	if _, err := s.Turns.Edit(ctx, c.ID, second.TurnID, "second-edited"); err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "second-edited")
	stub.gate <- struct{}{}
	if got := settled(t, s, c.ID); got != engine.Succeeded {
		t.Fatal("the Turn must be rescued in place, not fail", got)
	}
	prompts := stub.promptList()
	if len(prompts) != 4 || stub.forks[2] == nil || stub.forks[3] != nil {
		t.Fatal("one refused fork attempt, then one run without it", len(prompts), stub.forks)
	}
	if !strings.Contains(prompts[3], `"prompt":"first"`) || strings.Contains(prompts[3], `"prompt":"second"`) || !strings.HasSuffix(prompts[3], "second-edited") {
		t.Fatal("the rerun carries the index of the kept Turns", prompts[3])
	}
	if !transcriptHasNotice(t, s, c.ID, "此引擎不能原生分叉") {
		t.Fatal("the reader must be told")
	}
	if fork := forkAnchorOf(t, s, c.ID); fork != nil {
		t.Fatal("the unspendable fork intent must be gone, so later Turns do not retry it", fork)
	}
	// The next message on this version just resumes its native session.
	stub.mu.Lock()
	stub.unreachable = false
	stub.mu.Unlock()
	sendOnce(t, stub, s, c.ID, "next")
	if stub.forks[len(stub.forks)-1] != nil {
		t.Fatal("no further fork attempt")
	}
}

func TestADegradedSessionForkRescuedAtExecuteTimeCarriesTheSourceIndex(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true, Fork: true}, started: make(chan string, 8), gate: make(chan struct{}, 16)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	first := sendOnce(t, stub, s, c.ID, "first")
	sendOnce(t, stub, s, c.ID, "second")
	forked, err := s.Conversations.Fork(ctx, c.ID, first.TurnID)
	if err != nil || forked.Degraded {
		t.Fatal(forked, err)
	}
	stub.mu.Lock()
	stub.unreachable = true
	stub.mu.Unlock()
	sendOnce(t, stub, s, forked.Session.ID, "topic")
	prompts := stub.promptList()
	last := prompts[len(prompts)-1]
	if !strings.Contains(last, `"prompt":"first"`) || strings.Contains(last, `"prompt":"second"`) || !strings.HasSuffix(last, "topic") {
		t.Fatal("the rescued fork carries the source's kept Turns only", last)
	}
	if !transcriptHasNotice(t, s, forked.Session.ID, "此引擎不能原生分叉") {
		t.Fatal("the reader must be told")
	}
}
