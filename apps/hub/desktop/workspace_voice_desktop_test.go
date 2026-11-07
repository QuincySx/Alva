// INPUT: Actual native controls, fake local recorders and explicit delayed cleanup or transcript facts.
// OUTPUT: Draft-only adoption, edit/session isolation, independent Stop/Cancel and real close ownership regressions.
// POS: [L7 Hub UI tests] Never requests microphone permission, records sound or calls an external model.
//go:build desktop

package hubdesktop

import (
	"context"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/system/speech"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type workspaceFakeRecorder struct {
	mu                     sync.Mutex
	value                  speech.Snapshot
	done                   chan struct{}
	once                   sync.Once
	stops, cancels, closes atomic.Int32
	closeFailure           atomic.Bool
}

func newWorkspaceFakeRecorder() *workspaceFakeRecorder {
	return &workspaceFakeRecorder{done: make(chan struct{}), value: speech.Snapshot{State: speech.Requesting}}
}
func (r *workspaceFakeRecorder) Snapshot() speech.Snapshot {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.value
}
func (r *workspaceFakeRecorder) Stop()                 { r.stops.Add(1) }
func (r *workspaceFakeRecorder) Cancel()               { r.cancels.Add(1) }
func (r *workspaceFakeRecorder) Done() <-chan struct{} { return r.done }
func (r *workspaceFakeRecorder) Close(ctx context.Context) error {
	r.closes.Add(1)
	if r.closeFailure.Load() {
		return context.DeadlineExceeded
	}
	select {
	case <-r.done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
func (r *workspaceFakeRecorder) update(value speech.Snapshot, finished bool) {
	r.mu.Lock()
	r.value = value
	r.mu.Unlock()
	if finished {
		r.once.Do(func() { close(r.done) })
	}
}

func workspaceVoiceFixture(t *testing.T) (*NativePage, *ui.Tester, *workspaceFakeRecorder, *atomic.Int32) {
	t.Helper()
	s, _ := fixture(t)
	p := NewNativePage(s)
	p.selected, p.generation, p.model.Draft = "original-session", 7, "original draft"
	r := newWorkspaceFakeRecorder()
	calls := new(atomic.Int32)
	p.voice.start = func(_ context.Context, options speech.Options) (speech.Recorder, error) {
		calls.Add(1)
		if options.MaximumDuration != time.Minute {
			t.Error("recording is not bounded to sixty seconds")
		}
		return r, nil
	}
	tester := ui.NewTester(func(c *ui.Context) {
		p.observeWorkspaceVoice()
		ui.Column(c).FillWidth().Padding(16).Gap(8).Children(func() {
			ui.TextAreaBase(c.Key("draft"), &p.model.Draft).Label("Voice draft").FillWidth().Height(80)
			p.buildWorkspaceVoiceNotice(c)
			ui.Row(c).FillWidth().Gap(8).Children(func() { p.buildWorkspaceVoice(c) })
		})
	}, 960, 680)
	return p, tester, r, calls
}

func TestWorkspaceVoiceColdControlsStopAndAutoAdoptOnlyOriginalDraft(t *testing.T) {
	p, tester, recorder, calls := workspaceVoiceFixture(t)
	tester.Frame()
	mic, ok := tester.Find("Start dictation")
	if !ok || mic.W != 28 || mic.H != 28 || calls.Load() != 0 {
		t.Fatal("cold mic is not an explicit 32px action", mic)
	}
	connectionClick(t, tester, "Start dictation")
	if calls.Load() != 1 || p.voice.recorder == nil {
		t.Fatal("explicit mic did not create one owner")
	}
	recorder.update(speech.Snapshot{State: speech.Recording, StartedAt: time.Now()}, false)
	tester.Frame()
	pill, ok := tester.Find("Stop dictation")
	if !ok || pill.H != 28 || pill.W > 160 {
		t.Fatal("recording pill escaped the composer scale", pill)
	}
	connectionClick(t, tester, "Stop dictation")
	if recorder.stops.Load() != 1 || recorder.cancels.Load() != 0 || p.model.Draft != "original draft" || p.Service.Runtime.HasActive() {
		t.Fatal("recording stop cancelled a task, discarded or sent text")
	}
	recorder.update(speech.Snapshot{State: speech.Completed, Text: "系统本地文字"}, true)
	tester.Frame()
	if p.voice.recorder != nil || p.voice.pending != "" || p.model.Draft != "original draft\n系统本地文字" || recorder.closes.Load() != 1 {
		t.Fatal("completed voice did not merge and join its owner", p.voice, p.model.Draft)
	}
	sessions, err := p.Service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 {
		t.Fatal("dictation created or sent a conversation", err)
	}
}

func TestWorkspaceVoiceEditedOrNavigatedDraftRequiresBoundExplicitAdoption(t *testing.T) {
	for _, navigate := range []bool{false, true} {
		name := "edited"
		if navigate {
			name = "different-session"
		}
		t.Run(name, func(t *testing.T) {
			p, tester, recorder, _ := workspaceVoiceFixture(t)
			connectionClick(t, tester, "Start dictation")
			p.model.Draft = "new user draft"
			if navigate {
				p.finishWorkspaceVoiceForNavigation()
				p.selected, p.generation = "another-session", p.generation+1
			}
			recorder.update(speech.Snapshot{State: speech.Completed, Text: "retained voice"}, true)
			tester.Frame()
			if p.model.Draft != "new user draft" || p.voice.pending != "retained voice" {
				t.Fatal("late voice replaced an edited/other-session draft")
			}
			if navigate {
				connectionClick(t, tester, "Apply dictation text")
				if p.model.Draft != "new user draft" || recorder.stops.Load() != 1 {
					t.Fatal("different session adopted voice or navigation discarded capture")
				}
				p.selected, p.generation = "original-session", p.generation+1
				tester.Frame()
			}
			connectionClick(t, tester, "Apply dictation text")
			if p.model.Draft != "new user draft\nretained voice" || p.voice.pending != "" || p.Service.Runtime.HasActive() {
				t.Fatal("explicit adoption overwrote, sent or lost voice")
			}
		})
	}
}

func TestWorkspaceVoiceCancelSuppressesLateTextAndCloseRetainsPhysicalOwner(t *testing.T) {
	p, tester, recorder, _ := workspaceVoiceFixture(t)
	connectionClick(t, tester, "Start dictation")
	connectionClick(t, tester, "Cancel dictation")
	if p.voice.recorder == nil || recorder.cancels.Load() != 1 {
		t.Fatal("cancel forgot an unfinished native owner")
	}
	event := new(mygo.CloseEvent)
	if !p.guardWorkspaceVoiceClose(event) || !event.DefaultPrevented() || p.voice.recorder == nil {
		t.Fatal("close released an unfinished recorder")
	}
	recorder.update(speech.Snapshot{State: speech.Completed, Text: "late cancelled text"}, true)
	recorder.closeFailure.Store(true)
	event = new(mygo.CloseEvent)
	if !p.guardWorkspaceVoiceClose(event) || !event.DefaultPrevented() || p.voice.recorder == nil {
		t.Fatal("close ignored failed native join")
	}
	recorder.closeFailure.Store(false)
	event = new(mygo.CloseEvent)
	if p.guardWorkspaceVoiceClose(event) || event.DefaultPrevented() || p.voice.recorder != nil {
		t.Fatal("completed native cleanup could not close")
	}
	tester.Frame()
	if p.model.Draft != "original draft" || p.voice.pending != "" {
		t.Fatal("cancelled late result changed draft")
	}
	recorder.update(speech.Snapshot{State: speech.Completed, Text: "even later text"}, false)
	tester.Frame()
	if p.model.Draft != "original draft" {
		t.Fatal("detached recorder changed the next draft")
	}
}

func TestWorkspaceVoicePermissionEmptyResultAndDraftBudgetAreExplicit(t *testing.T) {
	t.Run("permission-denied", func(t *testing.T) {
		p, tester, _, _ := workspaceVoiceFixture(t)
		p.voice.start = func(context.Context, speech.Options) (speech.Recorder, error) { return nil, speech.ErrPermissionDenied }
		connectionClick(t, tester, "Start dictation")
		if p.voice.recorder != nil || !strings.Contains(p.voice.problem, "隐私设置") || p.model.Draft != "original draft" {
			t.Fatal("permission rejection fabricated a recording or text")
		}
	})
	t.Run("failed-start-retains-returned-owner", func(t *testing.T) {
		p, tester, recorder, _ := workspaceVoiceFixture(t)
		p.voice.start = func(context.Context, speech.Options) (speech.Recorder, error) {
			return recorder, speech.ErrPermissionDenied
		}
		connectionClick(t, tester, "Start dictation")
		if p.voice.recorder == nil || recorder.cancels.Load() != 1 || p.voice.problem == "" {
			t.Fatal("failed start forgot its returned physical owner")
		}
		recorder.update(speech.Snapshot{State: speech.Completed, Text: "late failed start"}, true)
		tester.Frame()
		if p.voice.recorder != nil || p.model.Draft != "original draft" || p.voice.pending != "" {
			t.Fatal("failed start adopted late text or leaked owner")
		}
	})
	for _, state := range []speech.State{speech.Completed, speech.Failed} {
		t.Run(string(state), func(t *testing.T) {
			p, tester, recorder, _ := workspaceVoiceFixture(t)
			connectionClick(t, tester, "Start dictation")
			recorder.update(speech.Snapshot{State: state, Problem: "local recognition failed"}, true)
			tester.Frame()
			if p.voice.problem == "" || p.voice.pending != "" || p.model.Draft != "original draft" {
				t.Fatal("empty/failed result reported success")
			}
		})
	}
	t.Run("combined-budget", func(t *testing.T) {
		p, tester, recorder, _ := workspaceVoiceFixture(t)
		base := strings.Repeat("x", view.MaxDraftBytes-1)
		p.model.Draft = base
		connectionClick(t, tester, "Start dictation")
		recorder.update(speech.Snapshot{State: speech.Completed, Text: "voice"}, true)
		tester.Frame()
		if p.model.Draft != base || p.voice.pending != "voice" || p.voice.problem == "" {
			t.Fatal("oversized merge truncated or replaced the draft")
		}
		connectionClick(t, tester, "Apply dictation text")
		if p.voice.pending != "voice" || p.model.Draft != base {
			t.Fatal("oversized explicit adoption lost text")
		}
		p.model.Draft = ""
		connectionClick(t, tester, "Apply dictation text")
		if p.model.Draft != "voice" || p.voice.pending != "" {
			t.Fatal("shortened draft could not adopt voice")
		}
	})
	t.Run("invalid-transcript", func(t *testing.T) {
		p, tester, recorder, _ := workspaceVoiceFixture(t)
		connectionClick(t, tester, "Start dictation")
		recorder.update(speech.Snapshot{State: speech.Completed, Text: strings.Repeat("x", view.MaxDraftBytes+1)}, true)
		tester.Frame()
		if p.voice.pending != "" || p.voice.problem == "" || p.model.Draft != "original draft" {
			t.Fatal("unbounded transcript escaped native contract")
		}
	})
}

func TestWorkspaceVoiceToolbarKeepsSendAtNarrowWidths(t *testing.T) {
	for _, tc := range []struct {
		name  string
		width float32
	}{{"960-window", 640}, {"resource-narrow-composer", 360}} {
		t.Run(tc.name, func(t *testing.T) {
			s, _ := fixture(t)
			p := NewNativePage(s)
			p.model.Draft = "draft remains sendable"
			recorder := newWorkspaceFakeRecorder()
			p.voice.start = func(context.Context, speech.Options) (speech.Recorder, error) { return recorder, nil }
			tester := ui.NewTester(func(c *ui.Context) {
				p.observeWorkspaceVoice()
				ui.Column(c).Width(tc.width).Children(func() { p.buildWorkspaceComposer(c) })
			}, 960, 680)
			connectionClick(t, tester, "Start dictation")
			recorder.update(speech.Snapshot{State: speech.Recording, StartedAt: time.Now()}, false)
			tester.Frame()
			composer, ok := tester.Find("Task composer")
			send, found := tester.Find("Send")
			pill, recorded := tester.Find("Stop dictation")
			if !ok || !found || !recorded || send.W != 28 || send.H != 28 || pill.H != 28 || pill.W > 160 || send.X+send.W > composer.X+composer.W || pill.X+pill.W > send.X || send.Y < 0 || send.Y+send.H > 680 {
				t.Fatal("recording displaced the complete Send action", composer, pill, send)
			}
			recorder.update(speech.Snapshot{State: speech.Cancelled}, true)
			tester.Frame()
		})
	}
}

func TestWorkspaceVoiceStopAndRunningTaskStopRemainIndependent(t *testing.T) {
	s, _ := fixture(t)
	owner := &settingsRunningEngine{entered: make(chan struct{}), release: make(chan struct{})}
	if err := s.Runtime.Register("alva", owner); err != nil {
		t.Fatal(err)
	}
	session, err := s.Create(t.Context(), "record while running", "")
	if err != nil {
		t.Fatal(err)
	}
	_, done, err := s.Runtime.Start(t.Context(), session.ID, "local controlled execution")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { close(owner.release); <-done }()
	select {
	case <-owner.entered:
	case <-time.After(time.Second):
		t.Fatal("controlled execution did not start")
	}
	p := NewNativePage(s)
	p.selected = session.ID
	p.detail, err = s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	p.model.Draft = "next prompt"
	recorder := newWorkspaceFakeRecorder()
	p.voice.start = func(context.Context, speech.Options) (speech.Recorder, error) { return recorder, nil }
	tester := ui.NewTester(workspaceStaticView(p), 960, 680)
	connectionClick(t, tester, "Start dictation")
	recorder.update(speech.Snapshot{State: speech.Recording, StartedAt: time.Now()}, false)
	tester.Frame()
	stop, found := tester.Find("Stop")
	voice, recorded := tester.Find("Stop dictation")
	if !found || !recorded || stop.W != 28 || stop.H != 28 || voice.H != 28 || voice.X+voice.W > stop.X {
		t.Fatal("task and microphone stop are not independently reachable", voice, stop)
	}
	connectionClick(t, tester, "Stop dictation")
	if recorder.stops.Load() != 1 || recorder.cancels.Load() != 0 || owner.cancels.Load() != 0 || !s.Runtime.HasActive() {
		t.Fatal("voice Stop changed the Agent owner")
	}
	connectionClick(t, tester, "Stop")
	waitWorkspace(t, p, tester, func() bool { return !s.Runtime.HasActive() })
	if owner.cancels.Load() != 1 || recorder.stops.Load() != 1 || recorder.cancels.Load() != 0 || p.voice.recorder == nil {
		t.Fatal("task Stop cancelled or detached the microphone")
	}
	recorder.update(speech.Snapshot{State: speech.Completed, Text: "next draft text"}, true)
	tester.Frame()
	if p.model.Draft != "next prompt" || p.voice.pending != "next draft text" {
		t.Fatal("task lifecycle replaced the bound voice draft")
	}
}

func TestWorkspaceVoiceSettingsNavigationStopsCaptureAndRetainsBoundText(t *testing.T) {
	s := preferenceFixture(t)
	session, err := s.Create(t.Context(), "voice settings origin", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.selected = session.ID
	p.detail, err = s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	p.model.Draft = "original session draft"
	recorder := newWorkspaceFakeRecorder()
	p.voice.start = func(context.Context, speech.Options) (speech.Recorder, error) { return recorder, nil }
	tester, _ := preferenceTester(p)
	connectionClick(t, tester, "Start dictation")
	recorder.update(speech.Snapshot{State: speech.Recording, StartedAt: time.Now()}, false)
	openPreferencePage(t, p, tester)
	if p.pane != "preferences" || recorder.stops.Load() != 1 || recorder.cancels.Load() != 0 || p.voice.recorder == nil {
		t.Fatal("actual Settings navigation did not stop and retain the local owner")
	}
	recorder.update(speech.Snapshot{State: speech.Completed, Text: "voice completed in settings"}, true)
	tester.Frame()
	if p.voice.pendingSession != session.ID || p.voice.pending != "voice completed in settings" || p.model.Draft != "original session draft" {
		t.Fatal("Settings completion replaced the hidden draft or lost source scope")
	}
	connectionClick(t, tester, "Back to workspace")
	connectionClick(t, tester, "Apply dictation text")
	if p.model.Draft != "original session draft\nvoice completed in settings" || p.voice.pending != "" || s.Runtime.HasActive() {
		t.Fatal("Settings return could not explicitly adopt its retained text")
	}
}

func TestWorkspaceVoiceRecordingBlocksHomeSendAndDirectAdmission(t *testing.T) {
	s, _ := fixture(t)
	p := NewNativePage(s)
	p.model.Draft = "home prompt stays here"
	recorder := newWorkspaceFakeRecorder()
	p.voice.start = func(context.Context, speech.Options) (speech.Recorder, error) { return recorder, nil }
	tester := ui.NewTester(workspaceStaticView(p), 960, 680)
	connectionClick(t, tester, "Start dictation")
	connectionClick(t, tester, "Send")
	p.sendWorkspaceDraft()
	p.sendPreparedSnapshot("direct attempted bypass", nil, nil)
	tester.Frame()
	sessions, err := s.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 || p.selected != "" || p.busy || p.voice.problem == "" || p.model.Draft != "home prompt stays here" || s.Runtime.HasActive() {
		t.Fatal("active dictation allowed Send to create or execute", sessions, err)
	}
	recorder.update(speech.Snapshot{State: speech.Cancelled}, true)
	tester.Frame()
}

type workspaceVoiceSendEngine struct{ requests chan engine.Request }

func (e workspaceVoiceSendEngine) Execute(_ context.Context, request engine.Request, emit func(engine.Event) error) error {
	e.requests <- request
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "voice-draft-submit-fixture"}); err != nil {
		return err
	}
	return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
}
func (workspaceVoiceSendEngine) Cancel(context.Context, string) error { return engine.ErrUnsupported }
func (workspaceVoiceSendEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (workspaceVoiceSendEngine) Capabilities() engine.Capabilities { return engine.Capabilities{} }

func TestWorkspaceVoiceHomePendingFirstSendBindsOnlyItsCreatedSession(t *testing.T) {
	for _, navigate := range []bool{false, true} {
		name := "same-home"
		if navigate {
			name = "return-after-session-navigation"
		}
		t.Run(name, func(t *testing.T) {
			s, _ := fixture(t)
			requests := make(chan engine.Request, 1)
			if err := s.Runtime.Register("alva", workspaceVoiceSendEngine{requests: requests}); err != nil {
				t.Fatal(err)
			}
			p := NewNativePage(s)
			p.model.Draft = "recording base"
			recorder := newWorkspaceFakeRecorder()
			p.voice.start = func(context.Context, speech.Options) (speech.Recorder, error) { return recorder, nil }
			tester := ui.NewTester(workspaceStaticView(p), 960, 680)
			connectionClick(t, tester, "Start dictation")
			p.model.Draft = "send this edited text only"
			p.voice.problem = "请先结束录音并确认文字，再发送。"
			recorder.update(speech.Snapshot{State: speech.Completed, Text: "retained home voice"}, true)
			tester.Frame()
			if p.voice.pending != "retained home voice" || p.voice.pendingSession != "" || p.voice.problem != "" {
				t.Fatal("edited Home result was not retained")
			}
			if navigate {
				other, err := s.Create(t.Context(), "other existing session", "")
				if err != nil {
					t.Fatal(err)
				}
				p.selectSession(other.ID)
				waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == other.ID })
				p.startWorkspaceTask()
				tester.Frame()
				if p.model.Draft != "send this edited text only" || p.voice.pendingSession != "" {
					t.Fatal("navigation lost the same unsent Home draft or pending text")
				}
			}
			connectionClick(t, tester, "Send")
			waitWorkspace(t, p, tester, func() bool {
				return p.selected != "" && !p.busy && !p.polling && !s.Runtime.HasActive() && p.model.Status == view.Completed
			})
			created := p.selected
			if p.voice.pendingSession != created || p.voice.sessionID != created || p.voice.pending != "retained home voice" || p.model.Draft != "" {
				t.Fatal("first Send did not migrate Home voice independently of the submitted draft", p.voice, p.model.Draft)
			}
			select {
			case request := <-requests:
				if request.Prompt != "send this edited text only" {
					t.Fatal("pending voice was sent without adoption", request.Prompt)
				}
			default:
				t.Fatal("explicit Send did not reach the actual runtime")
			}
			p.startWorkspaceTask()
			p.model.Draft = "future unrelated Home draft"
			tester.Frame()
			connectionClick(t, tester, "Apply dictation text")
			p.bindWorkspaceVoiceHome("unrelated-future-session")
			if p.model.Draft != "future unrelated Home draft" || p.voice.pendingSession != created {
				t.Fatal("migrated voice remained adoptable by future Home")
			}
			p.selectSession(created)
			waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == created })
			connectionClick(t, tester, "Apply dictation text")
			if p.model.Draft != "retained home voice" || p.voice.pending != "" || len(requests) != 0 || s.Runtime.HasActive() {
				t.Fatal("bound explicit adoption changed another draft or sent voice")
			}
		})
	}
}

func TestWorkspaceVoiceHomeBindingRejectsUnrelatedOrStaleActiveSources(t *testing.T) {
	p, _, recorder, _ := workspaceVoiceFixture(t)
	p.voice.recorder, p.voice.sessionID, p.voice.generation = recorder, "", p.generation
	p.bindWorkspaceVoiceHome("new-session")
	if p.voice.sessionID != "" {
		t.Fatal("non-Home caller rebound an active recording")
	}
	p.selected = ""
	p.voice.generation--
	p.bindWorkspaceVoiceHome("new-session")
	if p.voice.sessionID != "" {
		t.Fatal("stale active Home source was rebound")
	}
	p.voice.generation = p.generation
	p.bindWorkspaceVoiceHome("")
	if p.voice.sessionID != "" {
		t.Fatal("failed Create rebound a recording")
	}
	p.bindWorkspaceVoiceHome("new-session")
	if p.voice.sessionID != "new-session" || !p.voice.deferResult {
		t.Fatal("valid Home recording was not bound as a pending-only result")
	}
	recorder.update(speech.Snapshot{State: speech.Cancelled}, true)
	p.observeWorkspaceVoice()
}
