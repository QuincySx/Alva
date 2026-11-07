// INPUT: Actual private native record storage, cold Inspector adapter and headless MyGo transcript controls.
// OUTPUT: Canonical light/dark message geometry, explicit tool record reads and stale binding rejection.
// POS: [L7 Hub UI tests] Exercises retained original evidence without launching a model, process or OS window.
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/json"
	"math"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/engine/alva"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
)

type transcriptColdRuntime struct{ engine.Runtime }

func (transcriptColdRuntime) Capabilities() engine.Capabilities { return engine.Capabilities{} }
func (transcriptColdRuntime) Execute(context.Context, engine.Request, func(engine.Event) error) error {
	panic("transcript observation must not execute")
}

type transcriptInspector struct {
	engine.Inspector
	pages, reads atomic.Int32
	entered      chan struct{}
	release      <-chan struct{}
	readEntered  chan struct{}
	readRelease  <-chan struct{}
}

func (i *transcriptInspector) Page(ctx context.Context, scope engine.InspectionScope, query engine.InspectionQuery) (engine.InspectionPage, error) {
	i.pages.Add(1)
	if i.entered != nil {
		i.entered <- struct{}{}
		select {
		case <-i.release:
		case <-ctx.Done():
			return engine.InspectionPage{}, ctx.Err()
		}
	}
	return i.Inspector.Page(ctx, scope, query)
}

func (i *transcriptInspector) Read(ctx context.Context, scope engine.InspectionScope, uri string, offset int64) (engine.InspectionText, error) {
	i.reads.Add(1)
	if i.readEntered != nil {
		i.readEntered <- struct{}{}
		select {
		case <-i.readRelease:
		case <-ctx.Done():
			return engine.InspectionText{}, ctx.Err()
		}
	}
	return i.Inspector.Read(ctx, scope, uri, offset)
}

func transcriptPageFixture(t *testing.T, failed, noReference bool) (*NativePage, *transcriptInspector, engine.Reference) {
	t.Helper()
	s, _ := fixture(t)
	session, err := s.Create(t.Context(), "Transcript", "")
	if err != nil {
		t.Fatal(err)
	}
	native := "native-transcript"
	log, err := s.Repository.Create(t.Context(), session.ID, native, "")
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := json.Marshal(map[string]string{"text": "exact retained original tool result"})
	records, err := log.AppendRecords(t.Context(), []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "fixture", Type: "tool_result", SchemaVersion: 1, Payload: raw}})
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Store.BindNative(t.Context(), session.ID, session.ActiveBranchID, native); err != nil {
		t.Fatal(err)
	}
	nativeInspector, err := alva.NewRepositoryInspector(s.Repository)
	if err != nil {
		t.Fatal(err)
	}
	inspector := &transcriptInspector{Inspector: nativeInspector}
	if err = s.Runtime.RegisterDescriptor("alva", engine.Descriptor{Runtime: transcriptColdRuntime{}, Inspector: inspector, Config: engine.ConfigSurface{ProviderProtocol: "native"}}); err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.ctx = t.Context()
	p.selected = session.ID
	p.detail, err = s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	reference := engine.Reference{SessionID: native, RecordID: records[0].ID, Sequence: records[0].Sequence}
	var ref *engine.Reference
	if !noReference {
		ref = &reference
	}
	p.transcript.Replace([]hub.HistoryEntry{
		{Sequence: 1, SessionID: session.ID, TurnID: "turn", Input: "User paragraph"},
		{Sequence: 2, SessionID: session.ID, AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, SessionID: native, ID: "tool", Name: "demo_echo", Text: "Original result summary", Reference: ref, IsError: failed}},
		{Sequence: 3, SessionID: session.ID, AttemptID: "attempt", Event: engine.Event{Kind: engine.Message, ID: "reply", Text: "Assistant paragraph"}},
	})
	p.applyDetail()
	return p, inspector, reference
}

func transcriptPreferences(t *testing.T, p *NativePage) *product.PreferencesService {
	t.Helper()
	store, ok := p.Service.Store.(hub.PreferencesStore)
	if !ok {
		t.Fatal("fixture has no preference store")
	}
	preferences, err := product.NewPreferencesService(t.Context(), store)
	if err != nil {
		t.Fatal(err)
	}
	p.Service.Preferences = preferences
	t.Cleanup(func() { _ = preferences.Close(context.Background()) })
	return preferences
}

// The resource composition fixture reads these committed records through the
// same selectSession/history path as a reopened product session. No runtime runs.
func seedWorkspaceResourceConversation(t *testing.T, app *bootstrap.Application, session hub.Session) {
	t.Helper()
	claim, err := app.Service.Store.BeginTurn(t.Context(), session.ID, "把模型连接和伙伴群聊的流程整理成一份可实现的方案。")
	if err != nil {
		t.Fatal(err)
	}
	for _, event := range []engine.Event{
		{Kind: engine.SessionStarted, SessionID: "native-workspace-render-fixture"},
		{Kind: engine.Message, ID: "reply", Text: "模型连接、伙伴协作和执行反馈已分好结构，正在补充交互细节。"},
		{Kind: engine.Completed, Outcome: engine.Succeeded},
	} {
		if err := app.Service.Store.Record(t.Context(), claim.Attempt.ID, event, false); err != nil {
			t.Fatal(err)
		}
	}
	if err := app.Service.Store.FinishTurn(t.Context(), claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
}

func TestWorkspaceTranscriptToolDisclosureReadsActualQualifiedRecord(t *testing.T) {
	for _, failed := range []bool{false, true} {
		t.Run(map[bool]string{false: "completed", true: "failed"}[failed], func(t *testing.T) {
			p, inspector, _ := transcriptPageFixture(t, failed, false)
			tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
			if inspector.pages.Load() != 0 || inspector.reads.Load() != 0 || tester.HasText("Original result summary") || strings.Contains(strings.Join(tester.Texts(), "\n"), "Reference:") {
				t.Fatal("collapsed tool read or dumped original evidence", tester.Texts())
			}
			activity, ok := tester.Find("Tool activity attempt/tool")
			if !ok || math.Abs(float64(activity.H-37)) > .01 || activity.W > 760 {
				t.Fatal("tool disclosure lost compact reading geometry", activity)
			}
			for _, label := range []string{"Tool activity icon attempt/tool", "Tool disclosure icon attempt/tool"} {
				icon, ok := tester.Find(label)
				if !ok || icon.W != 16 || icon.H != 16 {
					t.Fatal("tool icon does not use canonical sixteen pixel geometry", label, icon)
				}
			}
			connectionClick(t, tester, "Tool activity attempt/tool")
			if !tester.HasText("Original result summary") || inspector.pages.Load() != 0 || inspector.reads.Load() != 0 {
				t.Fatal("disclosure lost original summary or read without the record action")
			}
			connectionClick(t, tester, "Inspect tool result attempt/tool")
			settlePage(t, tester, p)
			if p.problem != "" || p.inspectURI == "" || p.inspectText.URI != p.inspectURI || !strings.Contains(p.inspectText.Text, "exact retained original tool result") || inspector.pages.Load() != 1 || inspector.reads.Load() != 1 {
				t.Fatal("tool details did not resolve and read the actual qualified reference", p.problem, p.inspectURI, p.inspectText, inspector.pages.Load(), inspector.reads.Load())
			}
			if p.Service.Runtime.HasActive() || p.transcript.Rows[1].Text != "Original result summary" {
				t.Fatal("inspection executed or replaced retained evidence")
			}
		})
	}
}

func TestWorkspaceTranscriptNoReferenceAndStaleBindings(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, true, true)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	connectionClick(t, tester, "Tool activity attempt/tool")
	if !tester.HasText("Original result summary") {
		t.Fatal("failure without a reference lost its inspectable summary")
	}
	oldBinding, oldTool := p.transcriptBinding(), p.transcript.tools["attempt/tool"]
	p.generation++
	p.inspectTranscriptTool(oldBinding, "attempt/tool", oldTool)
	tester.Frame()
	if inspector.pages.Load() != 0 || inspector.reads.Load() != 0 || tester.HasText("Original result summary") {
		t.Fatal("stale observation retained disclosure or started a read")
	}
	connectionClick(t, tester, "Tool activity attempt/tool")
	connectionClick(t, tester, "Inspect tool result attempt/tool")
	settlePage(t, tester, p)
	if p.pane != "inspector" || inspector.pages.Load() != 1 || inspector.reads.Load() != 0 || p.transcript.Rows[1].Status != view.Failed {
		t.Fatal("missing-reference fallback fabricated a body or hid failure", p.pane, inspector.pages.Load(), inspector.reads.Load())
	}
	entries := append([]hub.HistoryEntry(nil), p.transcript.Entries...)
	for _, change := range []func(){
		func() { p.generation++ },
		func() { p.selected = "other-session" },
		func() { p.detail.Session.ActiveBranchID = "other-branch" },
		func() { p.transcript.Replace(nil) },
	} {
		p.selected, p.detail.Session.ActiveBranchID = oldBinding.session, oldBinding.branch
		p.transcript.Replace(entries)
		binding := p.transcriptBinding()
		change()
		p.inspectTranscriptTool(binding, "attempt/tool", oldTool)
	}
	if inspector.pages.Load() != 1 || inspector.reads.Load() != 0 {
		t.Fatal("stale session, branch or removed row targeted a later view")
	}
}

func TestWorkspaceTranscriptUnsettledToolInspectsOriginalOwnerAfterBranchSwitch(t *testing.T) {
	for _, source := range []string{"event", "start-reference"} {
		t.Run(source, func(t *testing.T) {
			p, inspector, reference := transcriptPageFixture(t, true, true)
			originalBranch := p.detail.Session.ActiveBranchID
			target := p.Service.DefaultTarget
			target.ModelID = "other-model"
			branch, err := p.Service.Store.Switch(t.Context(), p.selected, target, "next branch after cancelled tool")
			if err != nil {
				t.Fatal(err)
			}
			if err := p.Service.Store.BindNative(t.Context(), p.selected, branch.ID, "native-b"); err != nil {
				t.Fatal(err)
			}
			p.detail, err = p.Service.Store.Detail(t.Context(), p.selected)
			if err != nil {
				t.Fatal(err)
			}
			start := engine.Event{Kind: engine.ToolStart, ID: "tool", Name: "demo_echo"}
			if source == "event" {
				start.SessionID = reference.SessionID
			} else {
				start.Reference = &engine.Reference{SessionID: reference.SessionID, RecordID: "tool-start", Sequence: reference.Sequence}
			}
			p.transcript.Replace([]hub.HistoryEntry{
				{Sequence: 1, SessionID: p.selected, AttemptID: "attempt", Event: start},
				{Sequence: 2, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "tool", Text: "tool cancelled before settlement", IsError: true}},
			})
			p.applyDetail()
			tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
			connectionClick(t, tester, "Tool activity attempt/tool")
			connectionClick(t, tester, "Inspect tool result attempt/tool")
			settlePage(t, tester, p)
			tool := p.transcript.tools["attempt/tool"]
			if p.detail.Session.ActiveBranchID != branch.ID || branch.ID == originalBranch || p.inspectBranch != originalBranch || p.problem != "" || p.pane != "inspector" || len(p.inspectPage.Entries) != 1 || p.inspectPage.Entries[0].ID != reference.RecordID {
				t.Fatal("unsettled tool inspection followed the active branch instead of its native owner", p.problem, p.inspectBranch, p.detail.Session.ActiveBranchID, p.inspectPage)
			}
			if tool.hasRef || tool.reference != (engine.Reference{}) || p.inspectURI != "" || p.inspectText.Text != "" || inspector.pages.Load() != 1 || inspector.reads.Load() != 0 || p.Service.Runtime.HasActive() {
				t.Fatal("missing result reference fabricated a body read or execution instead of a cold original index", tool, p.inspectURI, p.inspectText, inspector.pages.Load(), inspector.reads.Load())
			}
		})
	}
}

func TestWorkspaceTranscriptUnknownToolOwnerNeverGuessesActiveBranch(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, true, true)
	p.transcript.Replace([]hub.HistoryEntry{{Sequence: 1, SessionID: p.selected, AttemptID: "unknown-attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "tool", Name: "unavailable", Text: "failure without retained owner", IsError: true}}})
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	connectionClick(t, tester, "Tool activity unknown-attempt/tool")
	connectionClick(t, tester, "Inspect tool result unknown-attempt/tool")
	settlePage(t, tester, p)
	if p.pane == "inspector" || p.problem == "" || p.inspectBranch != "" || inspector.pages.Load() != 0 || inspector.reads.Load() != 0 {
		t.Fatal("unknown tool owner guessed the currently active branch", p.pane, p.problem, p.inspectBranch, inspector.pages.Load(), inspector.reads.Load())
	}
}

func TestWorkspaceTranscriptLateReferenceResolutionCannotReadReplacementView(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, false, false)
	inspector.entered = make(chan struct{}, 1)
	release := make(chan struct{})
	inspector.release = release
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	connectionClick(t, tester, "Tool activity attempt/tool")
	connectionClick(t, tester, "Inspect tool result attempt/tool")
	select {
	case <-inspector.entered:
	case <-time.After(time.Second):
		t.Fatal("explicit detail action did not start metadata resolution")
	}
	p.generation++
	p.inspectURI, p.inspectText.Text = "replacement URI", "replacement reading"
	p.problem = "replacement problem"
	close(release)
	settlePage(t, tester, p)
	if inspector.reads.Load() != 0 || p.inspectURI != "replacement URI" || p.inspectText.Text != "replacement reading" || p.problem != "replacement problem" {
		t.Fatal("late metadata resolution read or replaced a later observation", inspector.reads.Load(), p.inspectURI, p.inspectText, p.problem)
	}
}

func TestWorkspaceTranscriptMissingReferenceClearsPreviouslyReadToolBody(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, false, false)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	connectionClick(t, tester, "Tool activity attempt/tool")
	connectionClick(t, tester, "Inspect tool result attempt/tool")
	settlePage(t, tester, p)
	if !strings.Contains(p.inspectText.Text, "exact retained original tool result") {
		t.Fatal("first tool did not read its actual original body", p.problem, p.inspectText)
	}
	p.setPane("")
	p.transcript.Append([]hub.HistoryEntry{{Sequence: 4, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, SessionID: "native-transcript", ID: "without-ref", Name: "unavailable", Text: "Original failure without reference", IsError: true}}})
	p.applyDetail()
	tester.Frame()
	connectionClick(t, tester, "Tool activity attempt/without-ref")
	connectionClick(t, tester, "Inspect tool result attempt/without-ref")
	settlePage(t, tester, p)
	if p.inspectURI != "" || p.inspectText.Text != "" || inspector.reads.Load() != 1 || tester.HasText("exact retained original tool result") {
		t.Fatal("missing-reference tool inherited the previous tool's selected body", p.inspectURI, p.inspectText, inspector.reads.Load())
	}
}

func TestWorkspaceTranscriptLateBodyReadCannotRefillClearedSelection(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, false, false)
	inspector.readEntered = make(chan struct{}, 1)
	release := make(chan struct{})
	inspector.readRelease = release
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	connectionClick(t, tester, "Tool activity attempt/tool")
	connectionClick(t, tester, "Inspect tool result attempt/tool")
	deadline := time.Now().Add(time.Second)
	entered := false
	for !entered && time.Now().Before(deadline) {
		tester.Frame()
		select {
		case <-inspector.readEntered:
			entered = true
		default:
			time.Sleep(time.Millisecond)
		}
	}
	if !entered {
		close(release)
		t.Fatal("explicit action never reached the owned body read")
	}
	p.inspectURI, p.inspectText.Text = "", "replacement detail"
	p.problem = "replacement problem"
	close(release)
	settlePage(t, tester, p)
	if p.inspectURI != "" || p.inspectText.Text != "replacement detail" || p.problem != "replacement problem" {
		t.Fatal("late body read refilled a cleared or replaced tool selection", p.inspectURI, p.inspectText, p.problem)
	}
}

func TestWorkspaceTranscriptCanonicalMessagesInBothAppearances(t *testing.T) {
	for _, dark := range []bool{false, true} {
		t.Run(map[bool]string{false: "light", true: "dark"}[dark], func(t *testing.T) {
			p, _, _ := transcriptPageFixture(t, false, false)
			preferences := transcriptPreferences(t, p)
			size := 16
			if _, err := preferences.Update(t.Context(), hub.PreferenceUpdate{ExpectedRevision: preferences.Snapshot().Revision, FontSize: &size}); err != nil {
				t.Fatal(err)
			}
			tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
			tester.SetDark(dark)
			bubble, bubbleOK := tester.Find("User message turn/input")
			turn, turnOK := tester.Find("User turn turn/input")
			user, userOK := tester.Find("User paragraph")
			assistant, assistantOK := tester.Find("Assistant paragraph")
			avatar, avatarOK := tester.Find("Message avatar attempt/reply")
			tool, toolOK := tester.Find("Tool activity attempt/tool")
			if !bubbleOK || !turnOK || !userOK || !assistantOK || !avatarOK || !toolOK || bubble.W != 560 || turn.W != 760 || math.Abs(float64(bubble.X+bubble.W-turn.X-turn.W)) > .01 ||
				math.Abs(float64(user.X-bubble.X-12)) > .01 || math.Abs(float64(user.Y-bubble.Y-12)) > .01 || math.Abs(float64(bubble.H-user.H-24)) > .01 ||
				avatar.W != 32 || avatar.H != 32 || math.Abs(float64(assistant.X-avatar.X-44)) > .01 || math.Abs(float64(tool.X-assistant.X)) > .01 {
				t.Fatal("right-aligned user bubble or avatar/reply/tool columns differ from formal22", turn, bubble, user, avatar, assistant, tool)
			}
			header, headerOK := tester.Find("Workspace header")
			transcript, transcriptOK := tester.Find("Conversation transcript")
			history, historyOK := tester.Find("Message history")
			if !headerOK || !transcriptOK || !historyOK || transcript.Y != header.Y+header.H+workspaceRegionGap || history.Y != transcript.Y+16 || turn.Y != history.Y ||
				math.Abs(float64(tool.Y-turn.Y-turn.H-24)) > .01 || math.Abs(float64(avatar.Y-tool.Y-tool.H-24)) > .01 {
				t.Fatal("conversation lost formal22 top16 and row24 spacing below the unchanged header", header, transcript, history, turn, tool, avatar)
			}
			want := ui.Hex("#F5F6F8") // Formal22 UserMessage binds ui.color.subtle.
			if dark {
				want = ui.Hex("#252932")
			}
			got := tester.Image().RGBAAt(int(bubble.X+20), int(bubble.Y+3))
			if got.R != want.R || got.G != want.G || got.B != want.B {
				t.Fatal("user bubble does not use canonical subtle", got, want)
			}
			if math.Abs(float64(user.H-assistant.H)) > .01 {
				t.Fatal("plain user text and ordinary assistant Markdown have different body typography", user, assistant)
			}
			reference := ui.NewTester(func(c *ui.Context) {
				p.applyPreferences(c)
				ui.Text(c, "Body reference").FontSize(16).FontWeight(400).LineHeight(1.75)
			}, 760, 100)
			bodyReference, ok := reference.Find("Body reference")
			if !ok || math.Abs(float64(user.H-bodyReference.H)) > .01 {
				t.Fatal("message body differs from native sixteen point, 1.75 line-height text", user, bodyReference)
			}
			messageHeader, ok := tester.Find("Message header attempt/reply")
			if !ok || messageHeader.H != 32 || messageHeader.Y != avatar.Y || messageHeader.X != assistant.X || math.Abs(float64(assistant.Y-messageHeader.Y-messageHeader.H-12)) > .01 {
				t.Fatal("assistant header or body gap differs from the canonical layout", messageHeader, assistant)
			}
		})
	}
}

func TestWorkspaceTranscriptSavedEngineIdentityNeverUsesComposerTarget(t *testing.T) {
	for _, target := range []hub.Target{
		{EngineID: "codex", ModelID: "engine-default", ProviderID: "engine-default", AccountSource: "fixture", BillingRoute: "fixture"},
		{EngineID: "pi", ModelID: "saved-pi-model", ProviderID: "saved-provider", AccountSource: "fixture", BillingRoute: "fixture"},
		{EngineID: "alva", ModelID: "saved-alva-model", ProviderID: "saved-provider", AccountSource: "fixture", BillingRoute: "fixture"},
		{EngineID: strings.Repeat("engine", 25), ModelID: strings.Repeat("saved-model", 20), ProviderID: "saved-provider", AccountSource: "fixture", BillingRoute: "fixture"},
	} {
		t.Run(target.EngineID, func(t *testing.T) {
			p, inspector, _ := transcriptPageFixture(t, false, false)
			branch, err := p.Service.Store.Switch(t.Context(), p.selected, target, "saved reply fixture")
			if err != nil {
				t.Fatal(err)
			}
			if err := p.Service.Store.BindNative(t.Context(), p.selected, branch.ID, "saved-native"); err != nil {
				t.Fatal(err)
			}
			if _, err := p.Service.Store.Switch(t.Context(), p.selected, p.Service.DefaultTarget, "next-turn fixture branch"); err != nil {
				t.Fatal(err)
			}
			p.detail, err = p.Service.Store.Detail(t.Context(), p.selected)
			if err != nil {
				t.Fatal(err)
			}
			p.composer.target = hub.Target{EngineID: "claude", ModelID: "unrelated-next-model"}
			p.transcript.Replace([]hub.HistoryEntry{
				{Sequence: 1, SessionID: p.selected, AttemptID: "saved-attempt", Event: engine.Event{Kind: engine.SessionStarted, SessionID: "saved-native"}},
				{Sequence: 2, SessionID: p.selected, AttemptID: "saved-attempt", Event: engine.Event{Kind: engine.Message, ID: "reply", Text: "Actual saved response"}},
			})
			p.applyDetail()
			tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
			author, model := p.transcriptAuthor(p.transcript.Rows[0])
			wantAuthor := map[string]string{"codex": "Codex", "pi": "Pi", "alva": "Alva"}[target.EngineID]
			if wantAuthor == "" {
				wantAuthor = target.EngineID
			}
			if author != wantAuthor || model != strings.ReplaceAll(target.ModelID, "engine-default", "") || tester.HasText("unrelated-next-model") || tester.HasText("engine-default") {
				t.Fatal("reply identity came from next-turn target or a fabricated CLI model", author, model, tester.Texts())
			}
			if _, ok := tester.Find("Message model saved-attempt/reply"); ok != (target.ModelID != "engine-default") {
				t.Fatal("actual model availability does not control its identity label")
			}
			header, _ := tester.Find("Message header saved-attempt/reply")
			authorBox, _ := tester.Find("Message author saved-attempt/reply")
			actions, _ := tester.Find("Message actions saved-attempt/reply")
			if authorBox.X < header.X || authorBox.X+authorBox.W > actions.X || actions.W != 28 || actions.H != 28 || actions.X+actions.W > header.X+header.W+.01 {
				t.Fatal("long saved identity displaced the content action or left its header", header, authorBox, actions)
			}
			p.transcript.Replace([]hub.HistoryEntry{{Sequence: 3, SessionID: p.selected, AttemptID: "unknown-attempt", Event: engine.Event{Kind: engine.Message, ID: "reply", Text: "No retained origin"}}})
			p.applyDetail()
			tester.Frame()
			if _, ok := tester.Find("Message model unknown-attempt/reply"); ok || inspector.pages.Load() != 0 || inspector.reads.Load() != 0 || p.Service.Runtime.HasActive() {
				t.Fatal("missing native identity fabricated a model or started an evidence read")
			}
		})
	}
}

func TestWorkspaceTranscriptStreamingCopyAndErrorStayInReplyColumn(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, false, false)
	p.transcript.Replace([]hub.HistoryEntry{
		{Sequence: 1, SessionID: p.selected, TurnID: "turn", Input: strings.Repeat("长消息 mixed text ", 90)},
		{Sequence: 2, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.SessionStarted, SessionID: "native-transcript"}},
		{Sequence: 3, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "reply", Text: "Streaming paragraph"}},
	})
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 960, 1200)
	avatar, avatarOK := tester.Find("Message avatar attempt/reply")
	if !avatarOK {
		t.Fatal("streaming reply has no identity column")
	}
	p.transcript.Append([]hub.HistoryEntry{{Sequence: 4, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "reply", Text: " continued"}}})
	p.applyDetail()
	tester.Frame()
	updated, ok := tester.Find("Message avatar attempt/reply")
	if !ok || updated.X != avatar.X {
		t.Fatal("stream append replaced its keyed reply column", avatar, updated)
	}
	connectionClick(t, tester, "Copy message attempt/reply")
	if tester.Clipboard() != "Streaming paragraph continued" {
		t.Fatal("copy did not use the current retained reply content", tester.Clipboard())
	}
	connectionClick(t, tester, "Message actions attempt/reply")
	if err := tester.ChooseMenuItem("复制消息"); err != nil {
		t.Fatal(err)
	}
	p.transcript.Append([]hub.HistoryEntry{{Sequence: 5, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.Error, Text: "Actual engine error"}}})
	p.applyDetail()
	tester.Frame()
	errorBody, found := tester.Find("Actual engine error")
	if !found || errorBody.X != updated.X+44 || inspector.pages.Load() != 0 || inspector.reads.Load() != 0 || p.Service.Runtime.HasActive() {
		t.Fatal("error lost reply-column geometry or presentation performed I/O", errorBody, updated)
	}
}

func TestWorkspaceTranscriptRendererRetainsSharedMarkdownOptions(t *testing.T) {
	p, _, _ := transcriptPageFixture(t, false, false)
	theme := &markdown.Theme{FontSize: 29, LineHeight: 1.2}
	calls := 0
	options := &markdown.Options{Theme: theme, Code: map[string]func(*ui.Context, *markdown.CodeBlockView){"fixture": func(c *ui.Context, block *markdown.CodeBlockView) {
		calls++
		ui.Text(c, "Registered module result")
	}}}
	render := newWorkspaceTranscriptRenderer(p, options)
	tester := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Children(func() {
			render(c, view.Message{ID: "module", Role: "Alva", Text: "```fixture\nactual\n```", Status: view.Completed})
		})
	}, 760, 400)
	if !tester.HasText("Registered module result") || calls == 0 || options.Theme != theme || theme.FontSize != 29 || theme.LineHeight != 1.2 {
		t.Fatal("Hub presentation replaced shared Markdown modules or mutated caller options")
	}
}

func TestWorkspaceTranscriptBodyFollowsSavedGeneralSizeAndSystemScale(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, false, false)
	preferences := transcriptPreferences(t, p)
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	assertBodySize := func(size float32) {
		t.Helper()
		for _, label := range []string{"User paragraph", "Assistant paragraph"} {
			body, ok := tester.Find(label)
			if !ok || math.Abs(float64(body.H-size*1.75)) > .01 {
				t.Fatal("actual transcript did not consume saved body size and system scale", size, label, body)
			}
		}
	}
	assertBodySize(14)
	openPreferencePage(t, p, tester)
	choosePreference(t, tester, "Body size", "16")
	connectionClick(t, tester, "Save preferences")
	settlePreferences(t, p, tester)
	if preferences.Snapshot().FontSize != 16 {
		t.Fatal("General did not save the selected body size")
	}
	connectionClick(t, tester, "Back to workspace")
	assertBodySize(16)
	tester.SetPreferences(ui.Preferences{TextScale: 1.5})
	assertBodySize(24)
	if inspector.pages.Load() != 0 || inspector.reads.Load() != 0 || p.Service.Runtime.HasActive() {
		t.Fatal("body preference presentation read evidence or started execution")
	}
}
