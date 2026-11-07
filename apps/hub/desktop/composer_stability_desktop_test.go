// INPUT: Opt-in duration, generated private files, actual bootstrap/SQLite and MyGo headless composer controls.
// OUTPUT: Wall-clock soak receipts, memory/process samples and explicit Send/Stop/draft/restart cleanup acceptance.
// POS: [L7 Hub acceptance] Exercises disposable local-only product lifecycles; never claims a short run is one-hour or real-window acceptance.
//go:build desktop

package hubdesktop

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

// ALVA_COMPOSER_SOAK_SECONDS=3600 go test -mod=readonly -tags desktop -race
// -run '^TestComposerStabilitySoak$' -count=1 -timeout 65m ./apps/hub/desktop
// ALVA_COMPOSER_SOAK_OUTPUT optionally selects a retained evidence directory.
// The private fixture tree is removed after the real application/server join.
func TestComposerStabilitySoak(t *testing.T) {
	raw := os.Getenv("ALVA_COMPOSER_SOAK_SECONDS")
	if raw == "" {
		t.Skip("opt-in local composer soak; set ALVA_COMPOSER_SOAK_SECONDS=3600 for one real hour")
	}
	seconds, err := strconv.Atoi(raw)
	if err != nil || seconds < 10 || seconds > 7200 {
		t.Fatal("ALVA_COMPOSER_SOAK_SECONDS must be 10..7200")
	}
	output := os.Getenv("ALVA_COMPOSER_SOAK_OUTPUT")
	if output == "" {
		output, err = os.MkdirTemp("", "alva-composer-soak-evidence-")
	} else {
		err = os.MkdirAll(output, 0700)
	}
	if err != nil {
		t.Fatal(err)
	}
	data, err := os.MkdirTemp(output, "disposable-")
	if err != nil {
		t.Fatal(err)
	}
	samples, err := os.OpenFile(filepath.Join(output, "samples.jsonl"), os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0600)
	if err != nil {
		t.Fatal(err)
	}
	s := &composerSoak{t: t, started: time.Now(), duration: time.Duration(seconds) * time.Second, output: output, data: data, samples: json.NewEncoder(samples)}
	defer samples.Close()
	defer s.finish()
	s.server = httptest.NewServer(http.HandlerFunc(s.respond))
	s.config = bootstrap.Config{DataDirectory: filepath.Join(data, "private"), OpenStores: bootstrap.LocalStores, ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: s.server.URL + "/chat", Model: "composer-local-soak", HTTPClient: s.server.Client()}}
	s.short = []byte("Explicit short selected file: SHORT-SELECTED-COMPOSER\n")
	s.long = append(bytes.Repeat([]byte("inert generated long text line\n"), 11000), []byte("LONG-END-SELECTED-COMPOSER\n")...)
	for name, body := range map[string][]byte{"short.unusual": s.short, "long.unusual": s.long, "inert.pdf": composerSoakPDF()} {
		if err = os.WriteFile(filepath.Join(data, name), body, 0600); err != nil {
			t.Fatal(err)
		}
	}
	s.open()
	s.sample("opened", false)
	deadline := s.started.Add(s.duration)
	for time.Now().Before(deadline) {
		cycleStart := time.Now()
		s.cycle()
		s.cycles++
		if s.cycles%10 == 0 {
			s.restart()
		}
		if time.Since(s.lastSample) >= 10*time.Second {
			s.sample("cycle", false)
		}
		// Bound CPU/IO and history production while continuing to pump both pages.
		until := cycleStart.Add(2 * time.Second)
		for time.Now().Before(until) && time.Now().Before(deadline) {
			s.frame()
			for _, p := range s.pages {
				p.poll()
			}
			time.Sleep(25 * time.Millisecond)
		}
	}
	// Even a 10-second smoke includes the actual Close/reopen recovery boundary.
	s.restart()
	s.completed = true
	t.Logf("composer soak measured %.3fs, requested %ds: cycles=%d sends=%d stops=%d restarts=%d; evidence=%s", time.Since(s.started).Seconds(), seconds, s.cycles, s.sends, s.stops, s.restarts, output)
}

type composerSoak struct {
	t                         *testing.T
	started, lastSample       time.Time
	duration                  time.Duration
	output, data              string
	samples                   *json.Encoder
	config                    bootstrap.Config
	server                    *httptest.Server
	app                       *bootstrap.Application
	pages                     []*NativePage
	testers                   []*ui.Tester
	cancels                   []context.CancelFunc
	short, long               []byte
	files                     []hub.ComposerFile
	session                   string
	cycles, sends, stops      int
	restarts, pdfs, conflicts int
	requests, active          atomic.Int64
	latestRequest             atomic.Pointer[json.RawMessage]
	hold                      atomic.Bool
	completed, cleaned        bool
	baselineHeap              uint64
	baselineGoroutines        int
	retained                  []uint64
	maxHeap, maxRSS, maxPDF   uint64
	pdfSamples                int
}

func (s *composerSoak) respond(w http.ResponseWriter, r *http.Request) {
	s.requests.Add(1)
	s.active.Add(1)
	defer s.active.Add(-1)
	var request json.RawMessage
	if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 2<<20)).Decode(&request); err != nil {
		http.Error(w, "invalid local fixture request", 400)
		return
	}
	s.latestRequest.Store(&request)
	if s.hold.Load() {
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, ": local request running; explicit Stop required\n\n")
		w.(http.Flusher).Flush()
		<-r.Context().Done()
		// The test separately waits for this real handler teardown, as well as
		// Runtime.SessionActive becoming false; HTTP cancellation alone is no join.
		time.Sleep(25 * time.Millisecond)
		return
	}
	w.Header().Set("Content-Type", "text/event-stream")
	fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Local deterministic composer answer\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
}

func (s *composerSoak) open() {
	s.t.Helper()
	app, err := bootstrap.Open(s.t.Context(), s.config)
	s.app = app // A partial failed construction still owns its cleanup.
	if err != nil {
		s.t.Fatal(err)
	}
	for range 2 {
		p := NewNativePage(app.Service)
		ctx, cancel := context.WithCancel(s.t.Context())
		p.ctx = ctx
		s.pages = append(s.pages, p)
		s.cancels = append(s.cancels, cancel)
		s.testers = append(s.testers, ui.NewTester(workspaceStaticView(p), 1240, 850))
		p.loadWorkspaceDraft()
	}
	s.await("initial draft recovery", func() bool {
		return s.pages[0].workspaceDraftEntry(s.pages[0].workspaceComposerDraftScope()).known && s.pages[1].workspaceDraftEntry(s.pages[1].workspaceComposerDraftScope()).known
	})
}

func (s *composerSoak) frame() {
	for i, p := range s.pages {
		s.testers[i].Frame()
		p.observeWorkspaceDraft()
	}
}

func (s *composerSoak) await(label string, done func() bool) {
	s.t.Helper()
	deadline := time.Now().Add(10 * time.Second)
	for time.Now().Before(deadline) {
		s.frame()
		if done() {
			return
		}
		if s.t.Context().Err() != nil {
			s.t.Fatal(label, s.t.Context().Err())
		}
		for _, p := range s.pages {
			p.poll()
		}
		time.Sleep(20 * time.Millisecond)
	}
	for _, p := range s.pages {
		e := p.workspaceDraftEntry(p.workspaceComposerDraftScope())
		s.t.Logf("page selected=%q draft=%q busy=%v polling=%v status=%s problem=%q draft-problem=%q dirty=%v saving=%v conflict=%v", p.selected, p.model.Draft, p.busy, p.polling, p.model.Status, p.problem, e.problem, e.dirty, e.saving, e.conflict)
		for scope, entry := range p.durableDrafts.entries {
			s.t.Logf("scope=%+v local=%q known=%v loading=%v dirty=%v saving=%v conflict=%v queued=%d problem=%q", scope, entry.local.Text, entry.known, entry.loading, entry.dirty, entry.saving, entry.conflict, len(entry.queue), entry.problem)
		}
	}
	s.t.Fatal("composer soak did not settle:", label)
}

func (s *composerSoak) edit(index int, text string) {
	s.t.Helper()
	p := s.pages[index]
	s.await("draft scope ready before editing", func() bool {
		e := p.workspaceDraftEntry(p.workspaceComposerDraftScope())
		current, ok := p.Service.ComposerDrafts.Snapshot(p.workspaceComposerDraftScope())
		return e.known && !e.loading && !e.saving && !e.dirty && !e.conflict && (!ok || current.Revision == e.committed.Revision)
	})
	// These are the same edit callback and CAS flush used by the native editor.
	p.model.Draft = text
	p.markWorkspaceDraftEdited()
	p.flushWorkspaceDraft()
	s.await("draft commit", func() bool {
		e := p.workspaceDraftEntry(p.workspaceComposerDraftScope())
		return !e.dirty && !e.saving && e.committed.Text == text
	})
}

func (s *composerSoak) click(index int, name string) {
	s.t.Helper()
	s.testers[index].Frame()
	if err := s.testers[index].Click(name); err != nil {
		s.t.Fatal(err)
	}
}

func (s *composerSoak) cycle() {
	s.t.Helper()
	p, other := s.pages[0], s.pages[1]
	if s.session == "" {
		p.startWorkspaceTask()
		other.startWorkspaceTask()
		s.await("home scope", func() bool { return p.selected == "" && other.selected == "" && !p.busy && !other.busy })
		before := s.requests.Load()
		p.captureWorkspaceFilePaths([]string{filepath.Join(s.data, "short.unusual"), filepath.Join(s.data, "long.unusual")})
		s.await("explicit native file capture", func() bool { return !p.busy && len(p.composerFiles[""]) == 2 })
		s.files = nil
		for _, selection := range p.composerFiles[""] {
			s.files = append(s.files, p.composerFileRecords[selection.ID])
		}
		if s.requests.Load() != before || s.app.Runtime.HasActive() {
			s.t.Fatal("file selection executed a model")
		}
	}
	s.readFiles()
	text := fmt.Sprintf("explicit successful Send cycle %d", s.cycles+1)
	s.edit(0, text)
	s.await("cross-window same-scope draft", func() bool { return other.model.Draft == text })
	// Both explicit saves share one CAS base. Either window may win; the loser
	// must preserve its own text until the visible explicit reload is chosen.
	if s.cycles%5 == 0 {
		s.conflict(text + " with next revision")
	}
	s.hold.Store(false)
	before := s.requests.Load()
	s.click(0, "Send")
	s.await("actual completed Send", func() bool {
		return !p.busy && !s.app.Runtime.HasActive() && p.model.Status == view.Completed && s.requests.Load() == before+1
	})
	s.sends++
	if s.session == "" {
		s.session = p.selected
		request := s.latestRequest.Load()
		if request == nil || !bytes.Contains(*request, []byte("SHORT-SELECTED-COMPOSER")) || bytes.Contains(*request, []byte("LONG-END-SELECTED-COMPOSER")) {
			s.t.Fatal("actual HTTP request lost selected short text or inlined the long document")
		}
		for _, file := range s.files {
			if err := s.app.Service.ValidateComposerFileOwner(s.t.Context(), s.session, file); err != nil {
				s.t.Fatal("first-send adoption grant missing", err)
			}
		}
		other.selectSession(s.session)
		s.await("second observer session", func() bool { return other.detail.Session.ID == s.session && !other.chat.Busy })
	}
	s.edit(0, fmt.Sprintf("explicit running Stop cycle %d", s.cycles+1))
	s.hold.Store(true)
	before = s.requests.Load()
	s.click(0, "Send")
	s.await("actual local HTTP execution running", func() bool {
		return s.active.Load() == 1 && s.requests.Load() == before+1 && !p.busy && p.model.Status == view.Running
	})
	s.sends++
	s.click(0, "Stop")
	s.await("actual Stop and both execution/HTTP joins", func() bool {
		return !s.app.Runtime.HasActive() && s.active.Load() == 0 && p.model.Status == view.Cancelled && p.stopRequests[s.session] == ""
	})
	s.stops++
	s.hold.Store(false)
	if p.model.Draft != "" || p.problem != "" {
		s.t.Fatal("successful admission retained old input or error", p.model.Draft, p.problem)
	}
	if s.cycles%10 == 0 && runtime.GOOS == "darwin" {
		s.pdf()
	}
}

func (s *composerSoak) conflict(primaryText string) {
	s.t.Helper()
	p, other := s.pages[0], s.pages[1]
	a := p.workspaceDraftEntry(p.workspaceComposerDraftScope())
	b := other.workspaceDraftEntry(other.workspaceComposerDraftScope())
	s.await("same clean CAS base", func() bool {
		return a.known && b.known && !a.loading && !b.loading && !a.saving && !b.saving && !a.dirty && !b.dirty && !a.conflict && !b.conflict && a.committed.Revision == b.committed.Revision
	})
	texts := []string{primaryText, "keep this window's conflicting input"}
	// No frame/debounce tick is pumped between the two edit and explicit Save
	// callbacks. Both requests therefore capture the same committed revision.
	for i, page := range s.pages {
		page.model.Draft = texts[i]
		page.markWorkspaceDraftEdited()
		page.flushWorkspaceDraft()
	}
	s.await("actual CAS winner and loser", func() bool {
		return !a.saving && !b.saving && a.conflict != b.conflict
	})
	loser, winner := 1, 0
	if a.conflict {
		loser, winner = 0, 1
	}
	for i, page := range s.pages {
		if page.model.Draft != texts[i] {
			s.t.Fatal("CAS completion replaced a window's captured input before explicit reload")
		}
	}
	winnerText := texts[winner]
	winnerEntry := s.pages[winner].workspaceDraftEntry(s.pages[winner].workspaceComposerDraftScope())
	if winnerEntry.dirty || winnerEntry.committed.Text != winnerText {
		s.t.Fatal("CAS winner was not actually committed")
	}
	s.click(loser, "Reload shared draft")
	converged := func(text string) bool {
		return p.model.Draft == text && other.model.Draft == text && !a.loading && !b.loading && !a.saving && !b.saving && !a.dirty && !b.dirty && !a.conflict && !b.conflict && a.committed.Revision == b.committed.Revision && a.committed.Text == text && b.committed.Text == text
	}
	s.await("explicit loser reload and real revision convergence", func() bool { return converged(winnerText) })
	// A subsequent explicit Save must advance the actual winning revision and
	// synchronize both windows, rather than silently overwriting the loser.
	revision := a.committed.Revision
	s.edit(0, primaryText+" resolved")
	s.await("explicit post-conflict Save and convergence", func() bool { return a.committed.Revision > revision && converged(primaryText+" resolved") })
	s.conflicts++
}

func (s *composerSoak) readFiles() {
	s.t.Helper()
	for _, file := range s.files {
		selection := product.SelectedComposerFile(file)
		expected, needle := s.short, "SHORT-SELECTED-COMPOSER"
		if file.Name == "long.unusual" {
			expected, needle = s.long, "LONG-END-SELECTED-COMPOSER"
		}
		hash := sha256.New()
		for offset := int64(0); offset < file.Bytes; {
			page, err := s.app.Service.ReadComposerFile(s.t.Context(), selection, false, offset, hub.ComposerFileReadBytes)
			if err != nil || len(page.Data) > hub.ComposerFileReadBytes || page.NextOffset <= offset {
				s.t.Fatal("authenticated bounded selected-file read", err)
			}
			hash.Write(page.Data)
			offset = page.NextOffset
		}
		digest := sha256.Sum256(expected)
		if hex.EncodeToString(hash.Sum(nil)) != hex.EncodeToString(digest[:]) || file.SHA256 != hex.EncodeToString(digest[:]) {
			s.t.Fatal("selected file contents changed")
		}
		found, err := s.app.Service.SearchComposerFile(s.t.Context(), selection, false, needle, 0, 5)
		if err != nil || len(found.Matches) != 1 || found.Matches[0].Offset != int64(bytes.Index(expected, []byte(needle))) {
			s.t.Fatal("bounded selected-file literal search", found, err)
		}
	}
}

func (s *composerSoak) pdf() {
	s.t.Helper()
	done := make(chan struct{})
	go func() {
		defer close(done)
		_, err := s.app.Service.CaptureComposerFile(s.t.Context(), hub.ComposerDraftScope{Kind: "home", ID: "soak-pdf"}, filepath.Join(s.data, "inert.pdf"))
		// Capture records extraction failure as a processing receipt. The actual
		// derivative below proves this fixture was parsed, rather than accepted raw.
		if err != nil {
			s.t.Error(err)
		}
	}()
	for {
		select {
		case <-done:
			files, err := s.app.Store.ListComposerFiles(s.t.Context(), hub.ComposerDraftScope{Kind: "home", ID: "soak-pdf"}, 1)
			if err != nil || len(files) != 1 || files[0].TextKey == "" {
				s.t.Fatal("actual PDF extraction was unavailable", files, err)
			}
			page, err := s.app.Service.ReadComposerFile(s.t.Context(), product.SelectedComposerFile(files[0]), true, 0, 4096)
			if err != nil || !bytes.Contains(page.Data, []byte("Soak inert PDF text")) {
				s.t.Fatal("actual PDF derivative missing", err)
			}
			s.pdfs++
			return
		case <-time.After(50 * time.Millisecond):
			self, pdf, children := s.rss()
			s.maxRSS = max(s.maxRSS, self)
			if self > 1<<30 || pdf > 1<<30 {
				s.t.Fatal("1 GiB observed memory ceiling exceeded during PDF processing")
			}
			if children > 0 {
				s.pdfSamples++
				s.maxPDF = max(s.maxPDF, pdf)
				if err := s.samples.Encode(map[string]any{"phase": "owned_pdf_active", "elapsed_seconds": time.Since(s.started).Seconds(), "self_rss_bytes": self, "owned_pdf_rss_bytes": pdf, "owned_pdf_processes": children}); err != nil {
					s.t.Fatal(err)
				}
			}
		}
	}
}

func (s *composerSoak) restart() {
	s.t.Helper()
	if s.session == "" {
		return
	}
	s.edit(0, "unsent session survives application Close and reopen")
	s.pages[1].startWorkspaceTask()
	s.await("separate home scope", func() bool {
		return s.pages[1].selected == "" && s.pages[1].workspaceDraftEntry(s.pages[1].workspaceComposerDraftScope()).known
	})
	s.edit(1, "unsent home survives application Close and reopen")
	before := s.requests.Load()
	s.close()
	s.sample("closed", true)
	s.open()
	s.await("cold home manual draft", func() bool { return s.pages[0].model.Draft == "unsent home survives application Close and reopen" })
	s.pages[0].selectSession(s.session)
	s.await("cold session manual draft", func() bool {
		return s.pages[0].model.Draft == "unsent session survives application Close and reopen" && !s.pages[0].chat.Busy
	})
	s.readFiles()
	for _, file := range s.files {
		if err := s.app.Service.ValidateComposerFileOwner(s.t.Context(), s.session, file); err != nil {
			s.t.Fatal("SQLite reopen lost immutable file ownership", err)
		}
	}
	if s.requests.Load() != before || s.app.Runtime.HasActive() || s.pages[1].selected != "" {
		s.t.Fatal("restoring manual drafts created work or sent a model request")
	}
	s.restarts++
	// Bound each native conversation's context while retaining its durable facts.
	s.pages[0].startWorkspaceTask()
	s.session = ""
}

func (s *composerSoak) close() {
	s.t.Helper()
	for _, p := range s.pages {
		p.flushWorkspaceDraft()
	}
	s.await("native close draft flush", func() bool {
		for _, p := range s.pages {
			if p.guardWorkspaceDraftClose(&mygo.CloseEvent{}) || p.polling || p.busy {
				return false
			}
		}
		return true
	})
	for i, p := range s.pages {
		p.referenceOwner.close()
		p.closeWorkspaceResources()
		s.cancels[i]()
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	if err := s.app.Close(ctx); err != nil {
		s.t.Fatal("actual Application.Close", err)
	}
	if !s.app.Service.Documents.CleanupComplete() || s.app.Runtime.HasActive() || s.active.Load() != 0 {
		s.t.Fatal("application shutdown returned before physical cleanup")
	}
	s.app, s.pages, s.testers, s.cancels = nil, nil, nil, nil
}

// RSS observations are restricted to this test process and its direct PDFKit
// osascript children (PPID and command both checked). Short workers can exit
// between probes; max_pdf_rss is observed, never a claimed process peak.
func (s *composerSoak) rss() (self, pdf uint64, children int) {
	s.t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	raw, err := exec.CommandContext(ctx, "/bin/ps", "-axo", "pid=,ppid=,rss=,comm=").Output()
	if err != nil {
		s.t.Fatal("local RSS observation", err)
	}
	for _, line := range strings.Split(string(raw), "\n") {
		fields := strings.Fields(line)
		if len(fields) < 4 {
			continue
		}
		pid, _ := strconv.Atoi(fields[0])
		parent, _ := strconv.Atoi(fields[1])
		kib, _ := strconv.ParseUint(fields[2], 10, 64)
		if pid == os.Getpid() {
			self = kib * 1024
		} else if parent == os.Getpid() && filepath.Base(fields[3]) == "osascript" {
			pdf += kib * 1024
			children++
		}
	}
	if self == 0 {
		s.t.Fatal("own-process RSS sample missing")
	}
	return
}

func (s *composerSoak) sample(phase string, gc bool) {
	s.t.Helper()
	if gc {
		runtime.GC()
	}
	var memory runtime.MemStats
	runtime.ReadMemStats(&memory)
	rss, pdf, children := s.rss()
	goroutines := runtime.NumGoroutine()
	var disk int64
	err := filepath.WalkDir(s.data, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if !entry.IsDir() {
			info, err := entry.Info()
			if err != nil {
				return err
			}
			disk += info.Size()
		}
		return nil
	})
	if err != nil {
		s.t.Fatal(err)
	}
	s.maxHeap, s.maxRSS, s.maxPDF = max(s.maxHeap, memory.HeapAlloc), max(s.maxRSS, rss), max(s.maxPDF, pdf)
	if err := s.samples.Encode(map[string]any{"elapsed_seconds": time.Since(s.started).Seconds(), "phase": phase, "forced_gc": gc, "heap_alloc_bytes": memory.HeapAlloc, "heap_inuse_bytes": memory.HeapInuse, "runtime_sys_bytes": memory.Sys, "goroutines": goroutines, "self_rss_bytes": rss, "owned_pdf_rss_bytes": pdf, "owned_pdf_processes": children, "fixture_disk_bytes": disk, "cycles": s.cycles, "sends": s.sends, "stops": s.stops, "restarts": s.restarts, "pdfs": s.pdfs, "http_requests": s.requests.Load(), "http_active": s.active.Load()}); err != nil {
		s.t.Fatal(err)
	}
	s.lastSample = time.Now()
	if memory.HeapAlloc > 1<<30 || rss > 1<<30 || pdf > 1<<30 {
		s.t.Fatal("1 GiB conservative observed memory ceiling exceeded")
	}
	if gc {
		if s.baselineHeap == 0 {
			s.baselineHeap, s.baselineGoroutines = memory.HeapAlloc, goroutines
		}
		s.retained = append(s.retained, memory.HeapAlloc)
		// Ordinary SQLite/native facts and selected originals deliberately grow
		// on disk. Only retained Go heap/goroutines enter the leak trend gate.
		if time.Since(s.started) >= 5*time.Minute && goroutines > s.baselineGoroutines+128 {
			s.t.Fatal("post-close goroutines grew by more than 128 after warmup")
		}
		if time.Since(s.started) >= 20*time.Minute && len(s.retained) >= 4 && memory.HeapAlloc > s.baselineHeap+(256<<20) {
			n := len(s.retained)
			if s.retained[n-4] < s.retained[n-3] && s.retained[n-3] < s.retained[n-2] && s.retained[n-2] < s.retained[n-1] {
				s.t.Fatal("four increasing post-close GC samples retain more than 256 MiB over warmup")
			}
		}
	}
}

func (s *composerSoak) finish() {
	// Fatal paths still close the exact original owners before fixture removal.
	if s.app != nil {
		for i, p := range s.pages {
			p.referenceOwner.close()
			if i < len(s.cancels) {
				s.cancels[i]()
			}
		}
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		err := s.app.Close(ctx)
		cancel()
		if err != nil {
			s.t.Error("final application physical cleanup", err)
		} else {
			s.cleaned = s.app.Service.Documents.CleanupComplete() && !s.app.Runtime.HasActive()
		}
	}
	if s.server != nil {
		s.server.CloseClientConnections()
		s.server.Close()
	}
	s.cleaned = s.cleaned && s.active.Load() == 0
	if s.cleaned {
		if err := os.RemoveAll(s.data); err != nil {
			s.t.Error("remove generated private fixture tree", err)
			s.cleaned = false
		}
		_, err := os.Stat(s.data)
		s.cleaned = s.cleaned && errors.Is(err, os.ErrNotExist)
	}
	result := map[string]any{"requested_seconds": s.duration.Seconds(), "elapsed_wall_seconds": time.Since(s.started).Seconds(), "completed_requested_duration": s.completed, "passed": s.completed && !s.t.Failed() && s.cleaned, "physical_cleanup_complete": s.cleaned, "disposable_data_removed": s.cleaned, "cycles": s.cycles, "sends": s.sends, "stops": s.stops, "application_restarts": s.restarts, "draft_conflicts_resolved": s.conflicts, "pdf_extractions": s.pdfs, "http_requests": s.requests.Load(), "http_active": s.active.Load(), "max_heap_alloc_bytes": s.maxHeap, "max_self_rss_bytes": s.maxRSS, "max_observed_owned_pdf_rss_bytes": s.maxPDF, "owned_pdf_rss_samples": s.pdfSamples, "scope": "local HTTP + actual Alva harness/shared product + SQLite + MyGo headless; no real GUI, account or vendor acceptance", "growth_policy": "Durable history, sessions and immutable originals intentionally grow on disk; contexts rotate every 10 cycles. Raw heap/self RSS ceiling 1 GiB; post-close forced-GC goroutine gate +128 after 5min; four increasing retained heap samples above warmup+256MiB after 20min. RSS polling is observational, not a hard cap or guaranteed peak."}
	raw, err := json.MarshalIndent(result, "", "  ")
	if err == nil {
		err = os.WriteFile(filepath.Join(s.output, "result.json"), append(raw, '\n'), 0600)
	}
	if err != nil {
		s.t.Error("persist final soak evidence", err)
	}
	if !s.cleaned {
		s.t.Error("soak cleanup incomplete; disposable evidence retained", s.data)
	}
}

func composerSoakPDF() []byte {
	content := "BT /F1 12 Tf 72 720 Td (Soak inert PDF text) Tj ET\n"
	objects := []string{"<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>", "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>", fmt.Sprintf("<< /Length %d >>\nstream\n%sendstream", len(content), content), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"}
	var pdf bytes.Buffer
	io.WriteString(&pdf, "%PDF-1.4\n")
	offsets := []int{0}
	for i, body := range objects {
		offsets = append(offsets, pdf.Len())
		fmt.Fprintf(&pdf, "%d 0 obj\n%s\nendobj\n", i+1, body)
	}
	xref := pdf.Len()
	fmt.Fprintf(&pdf, "xref\n0 %d\n0000000000 65535 f \n", len(offsets))
	for _, offset := range offsets[1:] {
		fmt.Fprintf(&pdf, "%010d 00000 n \n", offset)
	}
	fmt.Fprintf(&pdf, "trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n", len(offsets), xref)
	return pdf.Bytes()
}
