// INPUT: testing, real private product/native stores, complete local Alva harness and regular file fixtures
// OUTPUT: Reference-only admission, durable original ownership, upload budgets and typed timeline regressions
// POS: [L7 Hub product] Headless product integration tests without windows, credentials, external engines or model billing
package product

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	productstore "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/engine/alva"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func fixture(t *testing.T) (*HubService, chan engine.Request) {
	t.Helper()
	ctx := context.Background()
	directory := filepath.Join(t.TempDir(), "private")
	store, err := productstore.Open(ctx, filepath.Join(directory, "product"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	repo, err := session.Open(ctx, filepath.Join(directory, "native"))
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
	requests := make(chan engine.Request, 8)
	adapter := alva.New(func(ctx context.Context, id string, request engine.Request, resume bool) (harness.Runner, error) {
		requests <- request
		var log *session.Session
		var err error
		if resume {
			log, err = repo.OpenSession(ctx, request.OwnerID, id)
		} else {
			log, err = repo.Create(ctx, request.OwnerID, id, "")
		}
		if err != nil {
			return nil, err
		}
		plugins, err := harness.NativeSessionPlugins(ctx, repo, request.OwnerID, id)
		if err != nil {
			return nil, err
		}
		builder := harness.NewDemoBuilder(log).Workspace(request.Workspace)
		for _, plugin := range plugins {
			builder.Plugin(plugin)
		}
		return builder.Build(ctx)
	})
	inspector, err := alva.NewRepositoryInspector(repo)
	if err != nil {
		t.Fatal(err)
	}
	descriptor := engine.DescribeRuntime(adapter)
	descriptor.Inspector = inspector
	if err = runtime.RegisterDescriptor("alva", descriptor); err != nil {
		t.Fatal(err)
	}
	target := hub.Target{EngineID: "alva", ModelID: "fixture", ProviderID: "local", AccountSource: "test", BillingRoute: "no-billing"}
	service, err := NewHubService(store, runtime, repo, target, directory, true)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if service.Teams != nil {
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			_ = service.Teams.Close(ctx)
		}
	})
	return service, requests
}
func waitIdle(t *testing.T, s *HubService, id string) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		detail, err := s.Store.Detail(context.Background(), id)
		if err != nil {
			t.Fatal(err)
		}
		if detail.Session.Status != hub.Running && !s.Runtime.HasActive() {
			if detail.Session.Status != engine.Succeeded {
				t.Fatalf("unexpected terminal: %s", detail.Session.Status)
			}
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("run did not terminate")
}
func TestUploadStoresOriginalWithoutStartingAndSubmitsOnlyReference(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	product, err := s.Create(ctx, "files", "")
	if err != nil {
		t.Fatal(err)
	}
	secret := "RAW-LOG-BODY-MUST-NOT-ENTER-THE-PROMPT"
	path := filepath.Join(t.TempDir(), "service.log")
	if err = os.WriteFile(path, []byte(secret), 0600); err != nil {
		t.Fatal(err)
	}
	upload, err := s.Upload(ctx, product.ID, path)
	if err != nil {
		t.Fatal(err)
	}
	if s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("upload started an engine")
	}
	if upload.Info.Bytes != int64(len(secret)) || upload.Reference.URI == "" || !strings.Contains(upload.Info.Processing, "No compression") {
		t.Fatal("missing truthful original manifest")
	}
	page, err := s.Repository.ReadArtifact(ctx, product.ID, upload.Info.SessionID, upload.Reference.URI, 0, 4096)
	if err != nil || string(page.Data) != secret {
		t.Fatal("original not durably retained", err)
	}
	other, err := s.Create(ctx, "other", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Repository.ReadArtifact(ctx, other.ID, upload.Info.SessionID, upload.Reference.URI, 0, 4096); err == nil {
		t.Fatal("foreign product read another native artifact")
	}
	if _, err = s.Submit(ctx, product.ID, "Find the relevant errors in the selected file"); err != nil {
		t.Fatal(err)
	}
	request := <-requests
	if strings.Contains(request.Prompt, secret) || !strings.Contains(request.Prompt, upload.Reference.URI) || !strings.Contains(request.Prompt, "Original bytes were saved; not delivered") {
		t.Fatal("file bytes leaked or reference/processing manifest missing")
	}
	if request.OwnerID != product.ID || request.ResumeSession != upload.Info.SessionID {
		t.Fatal("actual product/native binding lost")
	}
	waitIdle(t, s, product.ID)
	if len(s.DraftAttachments(product.ID)) != 0 {
		t.Fatal("admitted draft not consumed")
	}
	history, err := s.Store.History(ctx, product.ID, 0, 128)
	if err != nil || len(history) < 4 {
		t.Fatal("complete local harness did not persist timeline", err)
	}
}
func TestUploadBudgetAndParkedBranchKeepDraft(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	product, err := s.Create(ctx, "files", "")
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(t.TempDir(), "large.log")
	file, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0600)
	if err != nil {
		t.Fatal(err)
	}
	if err = file.Truncate(20<<20 + 1); err != nil {
		t.Fatal(err)
	}
	file.Close()
	if _, err = s.Upload(ctx, product.ID, path); err == nil {
		t.Fatal("overlarge upload accepted")
	}
	if err = os.WriteFile(path, []byte("small"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Upload(ctx, product.ID, path); err != nil {
		t.Fatal(err)
	}
	target := s.DefaultTarget
	target.ModelID = "other-model"
	if _, err = s.Store.Switch(ctx, product.ID, target, "test explicit route"); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Submit(ctx, product.ID, "look"); err == nil {
		t.Fatal("parked branch references submitted elsewhere")
	}
	if len(s.DraftAttachments(product.ID)) != 1 || s.Runtime.HasActive() {
		t.Fatal("failed admission consumed draft or executed")
	}
}

type switchBeforeAdmission struct {
	hub.Store
	target hub.Target
}

func (s switchBeforeAdmission) BeginTurnOnBranch(ctx context.Context, id, branch, prompt string) (hub.Claim, error) {
	if _, err := s.Store.Switch(ctx, id, s.target, "concurrent explicit route change"); err != nil {
		return hub.Claim{}, err
	}
	return s.Store.BeginTurnOnBranch(ctx, id, branch, prompt)
}
func TestSelectedFileManifestCannotRaceIntoAnotherBranch(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	p, err := s.Create(ctx, "route race", "")
	if err != nil {
		t.Fatal(err)
	}
	attachment, err := s.UploadReader(ctx, p.ID, "log.txt", strings.NewReader("private original"))
	if err != nil {
		t.Fatal(err)
	}
	target := s.DefaultTarget
	target.ModelID = "different route"
	wrapped := switchBeforeAdmission{Store: s.Store, target: target}
	s.Store = wrapped
	s.Runtime = hub.New(wrapped)
	t.Cleanup(func() { _ = s.Runtime.Close(ctx) })
	if _, err = s.Submit(ctx, p.ID, "inspect selected reference"); err == nil {
		t.Fatal("stale branch file manifest admitted")
	}
	if len(s.DraftAttachments(p.ID)) != 1 || s.DraftAttachments(p.ID)[0].BranchID != attachment.BranchID || len(requests) != 0 {
		t.Fatal("route race consumed or sent saved draft")
	}
	entries, err := s.Store.History(ctx, p.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if entry.Input != "" {
			t.Fatal("stale manifest reached a committed model Turn")
		}
	}
}

func TestHistoryToolBoundNativeOwnershipAndExplicitRanges(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	product, err := s.Create(ctx, "history", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Submit(ctx, product.ID, "a relevant marker"); err != nil {
		t.Fatal(err)
	}
	request := <-requests
	waitIdle(t, s, product.ID)
	detail, err := s.Store.Detail(ctx, product.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	if request.OwnerID != product.ID {
		t.Fatal("wrong fixture owner")
	}
	tool := &HistoryTool{Store: s.Store, ProductID: product.ID, NativeID: branch.NativeSessionID}
	execution := abi.ToolExecutionContext{SessionID: branch.NativeSessionID, CallID: "history", ActualTool: tool}
	call := wire.ToolCall{ID: "history", Name: "hub_history", Arguments: []byte(`{"action":"search","query":"relevant marker"}`)}
	for _, kind := range []string{"missing actual tool", "missing call ID", "duplicate action", "case alias"} {
		bad, input := execution, call
		switch kind {
		case "missing actual tool":
			bad.ActualTool = nil
		case "missing call ID":
			bad.CallID, input.ID = "", ""
		case "duplicate action":
			input.Arguments = []byte(`{"action":"list","action":"list"}`)
		case "case alias":
			input.Arguments = []byte(`{"Action":"list"}`)
		}
		if _, err := tool.Execute(ctx, bad, input); err == nil {
			t.Errorf("%s admitted", kind)
		}
	}
	result, err := tool.Execute(ctx, execution, call)
	if err != nil || !strings.Contains(result.Content[0].Text, "scanned_through") {
		t.Fatal("bounded literal history search failed", err)
	}
	call.Arguments = []byte(`{"action":"read","sequence":1,"offset":0,"limit":20}`)
	result, err = tool.Execute(ctx, execution, call)
	if err != nil || !strings.Contains(result.Content[0].Text, "next_offset") {
		t.Fatal("explicit range read failed", err)
	}
	execution.SessionID = "foreign"
	if _, err = tool.Execute(ctx, execution, call); err == nil {
		t.Fatal("foreign native history access accepted")
	}
}
func TestInspectorReadsOnlyCommittedBoundBranchWithoutStarting(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	product, err := s.Create(ctx, "inspect", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Submit(ctx, product.ID, "small input"); err != nil {
		t.Fatal(err)
	}
	<-requests
	waitIdle(t, s, product.ID)
	detail, err := s.Store.Detail(ctx, product.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	page, err := s.Inspector(ctx, product.ID, branch.ID, "", 0, 0)
	if err != nil || len(page.Entries) == 0 || len(page.Entries) > 10 {
		t.Fatal("bounded metadata unavailable", err)
	}
	body, err := s.InspectorRead(ctx, product.ID, branch.ID, page.Entries[0].Value.URI, 0)
	if err != nil || len(body.Text) > 2048 {
		t.Fatal("bounded body read failed", err)
	}
	if _, err = s.Inspector(ctx, product.ID, "foreign-branch", "", 0, 0); err == nil {
		t.Fatal("unknown branch authorized")
	}
	other, err := s.Create(ctx, "other", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.InspectorRead(ctx, other.ID, branch.ID, page.Entries[0].Value.URI, 0); err == nil {
		t.Fatal("foreign native reference was read")
	}
	if s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("Inspector resumed or started execution")
	}
}
