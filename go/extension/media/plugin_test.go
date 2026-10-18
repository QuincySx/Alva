// INPUT: Actual SDK model/tool loop, private native artifacts and real explicit Guard decisions
// OUTPUT: Source preservation, selected-image one-request scope, truthful receipts and fail-closed regressions
// POS: [L4 extension] Media policy acceptance never invokes a paid model, user decoder or automatic raw upload input
package media

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"image"
	"image/color"
	"image/png"
	"math"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/harness"
	"github.com/QuincySx/Alva/go/platform/storage/session"
	nativemedia "github.com/QuincySx/Alva/go/platform/system/media"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (fn modelFunc) Stream(c context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(c, r, emit)
}
func (modelFunc) ProviderID() string { return "fixture-provider" }
func (modelFunc) ModelID() string    { return "fixture-image-model" }

type mediaFixture struct {
	repo      *session.Repository
	log       *session.Session
	plugin    *Plugin
	cwd       string
	source    session.ArtifactInfo
	original  []byte
	approvals int
}

func newFixture(t *testing.T, allow, imageCapable bool) *mediaFixture {
	t.Helper()
	ctx := context.Background()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := repo.Close(); err != nil {
			t.Error(err)
		}
	})
	log, err := repo.Create(ctx, "owner", "actual", "")
	if err != nil {
		t.Fatal(err)
	}
	f := &mediaFixture{repo: repo, log: log, cwd: t.TempDir()}
	permissions := security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(_ context.Context, a security.ApprovalRequest) (security.ApprovalDecision, error) {
		f.approvals++
		if a.SessionID != "actual" || a.ArgumentsSHA256 == "" {
			t.Error("lost actual approval identity")
		}
		return security.ApprovalDecision{Allowed: allow, Scope: security.Once}, nil
	}))
	guard, err := security.NewGuard(f.cwd, nil, nil, permissions)
	if err != nil {
		t.Fatal(err)
	}
	p, err := New(ctx, Config{Repository: repo, Owner: "owner", SessionID: log.ID(), Cwd: f.cwd, Route: "configured fixture route", ProviderID: "fixture-provider", ModelID: "fixture-image-model", Guard: guard, ImageCapable: imageCapable})
	if err != nil {
		t.Fatal(err)
	}
	f.plugin = p
	im := image.NewNRGBA(image.Rect(0, 0, 640, 320))
	for y := 0; y < 320; y++ {
		for x := 0; x < 640; x++ {
			im.SetNRGBA(x, y, color.NRGBA{R: uint8(x), G: uint8(y), B: 20, A: 255})
		}
	}
	var raw bytes.Buffer
	if err = png.Encode(&raw, im); err != nil {
		t.Fatal(err)
	}
	f.original = raw.Bytes()
	f.source, err = repo.PutArtifact(ctx, "owner", log.ID(), session.ArtifactInput{Kind: "input-attachment", Name: "original.png", MediaType: "image/png", Processing: "Original bytes retained; no compression, extraction or model delivery", Data: f.original})
	if err != nil {
		t.Fatal(err)
	}
	return f
}
func imageCount(request abi.ModelRequest) int {
	count := 0
	for _, m := range request.Messages {
		for _, b := range m.Content {
			if b.Image != nil {
				count++
			}
			if b.ToolResult != nil {
				for _, c := range b.ToolResult.Content {
					if c.Image != nil {
						count++
					}
				}
			}
		}
	}
	return count
}
func emitCall(emit func(wire.StreamChunk) error, id string, args any) error {
	data, err := json.Marshal(args)
	if err != nil {
		return err
	}
	if err = emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: id, Name: "hub_media", Arguments: data}}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}
func emitFinal(emit func(wire.StreamChunk) error) error {
	if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "actual response"}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}
func (f *mediaFixture) manifest(t *testing.T) wire.Reference {
	t.Helper()
	page, err := f.plugin.library.ListArtifacts(t.Context(), f.source.Reference().URI, 0, 0)
	if err != nil || len(page.Entries) != 1 {
		t.Fatal("missing actual derivative manifest", err)
	}
	return page.Entries[0].Info.Reference()
}
func (f *mediaFixture) direct(t *testing.T) (abi.RuntimeContext, abi.ToolExecutionContext) {
	t.Helper()
	model := modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil })
	state, err := abi.NewAgentState(model, f.log, []abi.Tool{f.plugin.Tool()})
	if err != nil {
		t.Fatal(err)
	}
	r := abi.RuntimeContext{RunID: "direct-run", State: state, Workspace: f.cwd, InvocationModel: model}
	if err = f.plugin.OnRunStart(t.Context(), r); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := f.plugin.OnRunEnd(context.Background(), r, nil); err != nil {
			t.Error(err)
		}
	})
	return r, abi.ToolExecutionContext{RunID: r.RunID, SessionID: f.log.ID(), Session: f.log, Workspace: f.cwd, ActualTool: f.plugin.Tool()}
}
func (f *mediaFixture) invoke(t *testing.T, e abi.ToolExecutionContext, id string, args any) (wire.ToolResult, error) {
	t.Helper()
	data, err := json.Marshal(args)
	if err != nil {
		t.Fatal(err)
	}
	e.CallID = id
	return f.plugin.Tool().Execute(t.Context(), e, wire.ToolCall{ID: id, Name: "hub_media", Arguments: data})
}

func TestActualSDKSelectedImageOnlyOneRequestAndReceipt(t *testing.T) {
	f := newFixture(t, true, true)
	round := 0
	model := modelFunc(func(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		round++
		if round != 3 && imageCount(request) != 0 {
			t.Fatal("raw/unselected/replayed media entered model request", round)
		}
		switch round {
		case 1:
			return emitCall(emit, "process", map[string]any{"action": "process_image", "source_uri": f.source.Reference().URI})
		case 2:
			return emitCall(emit, "select", map[string]any{"action": "select", "manifest_uris": []string{f.manifest(t).URI}, "reason": "inspect requested plot"})
		case 3:
			if imageCount(request) != 1 {
				t.Fatal("selection not present exactly once")
			}
			for _, m := range request.Messages {
				for _, b := range m.Content {
					if b.Image != nil {
						if b.Image.Width != 512 || b.Image.Payload.Provenance == nil || b.Image.Payload.Provenance.Original.SHA256 != f.source.SHA256 || bytes.Equal(b.Image.Payload.Data, f.original) {
							t.Fatal("wrong payload/provenance")
						}
					}
				}
			}
		}
		return emitFinal(emit)
	})
	refs, err := f.repo.BoundReferences(t.Context(), "owner", "actual")
	if err != nil {
		t.Fatal(err)
	}
	agent, err := core.NewBuilder(model, f.log).Workspace(f.cwd).Plugin(f.plugin).Plugin(harness.NewContextRetrievalPlugin("actual", refs)).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "explicitly inspect the uploaded plot reference")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "a later text-only request")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if round != 4 || f.approvals != 2 {
		t.Fatal("wrong model/cost/approval count", round, f.approvals)
	}
	messages, err := f.log.Messages(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if imageCount(abi.ModelRequest{Messages: messages}) != 0 {
		t.Fatal("one-request image bytes persisted to conversation")
	}
	_, original, err := f.repo.ReadArtifactFull(t.Context(), "owner", "actual", f.source.Reference().URI, 20<<20)
	if err != nil || !bytes.Equal(original, f.original) {
		t.Fatal("original mutated", err)
	}
	page, err := f.repo.PageArtifacts(t.Context(), "owner", "actual", "media-selection", "", 0, 0, 4)
	if err != nil || len(page.Entries) != 1 {
		t.Fatal("missing durable selection", err)
	}
	var selected Selection
	_, data, err := f.repo.ReadArtifactFull(t.Context(), "owner", "actual", page.Entries[0].Info.Reference().URI, 16<<10)
	if err != nil || decodeJSON(data, &selected) != nil {
		t.Fatal(err)
	}
	receipts, err := f.plugin.library.Receipts(t.Context(), selected.ID, 0, 0)
	if err != nil || len(receipts.Entries) != 3 {
		t.Fatal("receipt sequence incomplete", receipts, err)
	}
	states := []string{"prepared", "model_chain_invocation_intent", "response_observed_not_proof_of_understanding"}
	for i, entry := range receipts.Entries {
		_, raw, e := f.repo.ReadArtifactFull(t.Context(), "owner", "actual", entry.Info.Reference().URI, 16<<10)
		var receipt Receipt
		if e != nil || decodeJSON(raw, &receipt) != nil || receipt.State != states[i] || (i > 0 && (receipt.ProviderID != "fixture-provider" || receipt.ModelID != "fixture-image-model")) {
			t.Fatal("false/unknown receipt", receipt, e)
		}
	}
}

func TestMediaDeniedForeignAndSpoofedSessionCannotDerive(t *testing.T) {
	f := newFixture(t, false, true)
	_, e := f.direct(t)
	if _, err := f.invoke(t, e, "denied", map[string]any{"action": "process_image", "source_uri": f.source.Reference().URI}); !errors.Is(err, security.ErrPermissionDenied) {
		t.Fatal("processing bypassed consent", err)
	}
	page, err := f.repo.PageArtifacts(t.Context(), "owner", "actual", "media-derivative", "", 0, 0, 4)
	if err != nil || len(page.Entries) != 0 {
		t.Fatal("denied operation left a derivative", err)
	}
	if _, err = f.repo.Create(t.Context(), "foreign", "other", ""); err != nil {
		t.Fatal(err)
	}
	foreign, err := NewLibrary(t.Context(), f.repo, "foreign", "other")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err = foreign.Original(t.Context(), f.source.Reference().URI); err == nil {
		t.Fatal("URI knowledge granted original access")
	}
	e.Session = core.NewMemorySession("actual")
	if _, err = f.invoke(t, e, "spoof", map[string]any{"action": "list_artifacts", "source_uri": f.source.Reference().URI}); err == nil {
		t.Fatal("same ID memory session spoofed actual repository authority")
	}
	e.Session = f.log
	e.ActualTool = &mediaTool{plugin: f.plugin}
	if _, err = f.invoke(t, e, "borrowed-tool", map[string]any{"action": "list_artifacts", "source_uri": f.source.Reference().URI}); err == nil {
		t.Fatal("same-name different media instance borrowed authority")
	}
}

func TestSelectionUnknownRouteCancellationAndFailureNeverReplay(t *testing.T) {
	for _, mode := range []string{"unknown", "cancel", "provider-failure", "run-end", "changed-model"} {
		t.Run(mode, func(t *testing.T) {
			f := newFixture(t, true, mode != "unknown")
			r, e := f.direct(t)
			if _, err := f.invoke(t, e, "process", map[string]any{"action": "process_image", "source_uri": f.source.Reference().URI}); err != nil {
				t.Fatal(err)
			}
			result, err := f.invoke(t, e, "choose", map[string]any{"action": "select", "manifest_uris": []string{f.manifest(t).URI}, "reason": "explicit fixture"})
			if mode == "unknown" {
				if err == nil {
					t.Fatal("unknown capability submitted")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			var prepared struct {
				ID string `json:"selection_id"`
			}
			if json.Unmarshal([]byte(result.Content[0].Text), &prepared) != nil || prepared.ID == "" {
				t.Fatal("missing selection identity")
			}
			if mode == "cancel" {
				if _, err = f.invoke(t, e, "cancel", map[string]any{"action": "cancel_selection", "selection_id": prepared.ID}); err != nil {
					t.Fatal(err)
				}
			}
			if mode == "run-end" {
				if err = f.plugin.OnRunEnd(t.Context(), r, nil); err != nil {
					t.Fatal(err)
				}
				r.RunID = "later-run"
				if err = f.plugin.OnRunStart(t.Context(), r); err != nil {
					t.Fatal(err)
				}
				defer f.plugin.OnRunEnd(context.Background(), r, nil)
			}
			if mode == "changed-model" {
				if err = r.State.SetModel(unidentifiedModel{}); err != nil {
					t.Fatal(err)
				}
				r.InvocationModel = unidentifiedModel{}
			}
			calls := 0
			failure := errors.New("actual private provider failure")
			next := func(_ context.Context, _ abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				calls++
				expected := 0
				if mode == "provider-failure" {
					expected = 1
				}
				if imageCount(request) != expected {
					t.Fatal("wrong image request scope", mode)
				}
				if mode == "provider-failure" {
					return failure
				}
				return emitFinal(emit)
			}
			err = f.plugin.WrapModel(t.Context(), r, abi.ModelRequest{}, func(wire.StreamChunk) error { return nil }, next)
			if mode == "provider-failure" && !errors.Is(err, failure) {
				t.Fatal("lost actual provider failure", err)
			}
			if mode == "changed-model" {
				if err == nil || calls != 0 {
					t.Fatal("changed actual model still received media", err, calls)
				}
				return
			}
			if mode != "provider-failure" && err != nil {
				t.Fatal(err)
			}
			if err = f.plugin.WrapModel(t.Context(), r, abi.ModelRequest{}, func(wire.StreamChunk) error { return nil }, func(_ context.Context, _ abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				if imageCount(request) != 0 {
					t.Fatal("selection automatically replayed")
				}
				return emitFinal(emit)
			}); err != nil {
				t.Fatal(err)
			}
			receipts, err := f.plugin.library.Receipts(t.Context(), prepared.ID, 0, 0)
			if err != nil || len(receipts.Entries) < 2 {
				t.Fatal(err)
			}
			last := receipts.Entries[len(receipts.Entries)-1]
			_, raw, err := f.repo.ReadArtifactFull(t.Context(), "owner", "actual", last.Info.Reference().URI, 16<<10)
			if err != nil || (mode == "provider-failure" && !strings.Contains(string(raw), "acceptance_unknown")) {
				t.Fatal("missing true error receipt", err)
			}
		})
	}
}

type unidentifiedModel struct{}

func (unidentifiedModel) Stream(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error {
	return nil
}

type retryWrapper struct {
	failure error
}

func (retryWrapper) Name() string { return "explicit-retry-fixture" }
func (w retryWrapper) WrapModel(ctx context.Context, r abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
	err := next(ctx, r, request, emit)
	if errors.Is(err, w.failure) {
		return next(ctx, r, request, emit)
	}
	return err
}

type trailingPlugin struct{}

func (trailingPlugin) Name() string { return "trailing-fixture" }
func (trailingPlugin) Register(_ context.Context, r *core.Registrar) error {
	return r.Middleware(retryWrapper{}, math.MaxInt)
}

func TestOuterRetryCannotReplaySelectedMediaAndInnerWrapperRejected(t *testing.T) {
	f := newFixture(t, true, true)
	failure := errors.New("fixture first model attempt failed")
	images, round := 0, 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		round++
		images += imageCount(request)
		switch round {
		case 1:
			return emitCall(emit, "process", map[string]any{"action": "process_image", "source_uri": f.source.Reference().URI})
		case 2:
			return emitCall(emit, "choose", map[string]any{"action": "select", "manifest_uris": []string{f.manifest(t).URI}, "reason": "inspect exactly once"})
		case 3:
			if imageCount(request) != 1 {
				t.Fatal("first attempt lost explicit selected image")
			}
			return failure
		default:
			if imageCount(request) != 0 {
				t.Fatal("outer retry received/replayed injected image")
			}
			return emitFinal(emit)
		}
	})
	agent, err := core.NewBuilder(model, f.log).Workspace(f.cwd).Middleware(retryWrapper{failure: failure}).Plugin(f.plugin).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "explicit local fixture")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if images != 1 || round != 4 {
		t.Fatal("media/model replay count", images, round)
	}
	if _, err = core.NewBuilder(model, f.log).Workspace(f.cwd).Plugin(f.plugin).Plugin(trailingPlugin{}).Build(t.Context()); err == nil {
		t.Fatal("innermost unsafe model wrapper was accepted")
	}
}

func TestDecoderFailureEvidenceRetainsOnlyKnownRedactedStream(t *testing.T) {
	f := newFixture(t, true, true)
	cause := errors.New("real decoder failure")
	err := f.plugin.recordDecoderFailure(t.Context(), wire.ToolCall{ID: "failed-call"}, f.source.Reference().URI, &nativemedia.DecoderError{Err: cause, Log: []byte("api_key=must-not-be-retained\n")})
	if err != nil {
		t.Fatal(err)
	}
	page, err := f.repo.PageArtifacts(t.Context(), "owner", "actual", "media-decoder-log", "failed-call", 0, 0, 4)
	if err != nil || len(page.Entries) != 1 {
		t.Fatal(err)
	}
	_, raw, err := f.repo.ReadArtifactFull(t.Context(), "owner", "actual", page.Entries[0].Info.Reference().URI, 128<<10)
	if err != nil || bytes.Contains(raw, []byte("must-not-be-retained")) || !bytes.Contains(raw, []byte("redacted")) {
		t.Fatal("unsafe decoder evidence", string(raw), err)
	}
}

func TestManifestAndSelectionRejectOriginalMediaAndReadCorruption(t *testing.T) {
	f := newFixture(t, true, true)
	_, e := f.direct(t)
	if _, err := f.invoke(t, e, "raw-select", map[string]any{"action": "select", "manifest_uris": []string{f.source.Reference().URI}, "reason": "not derived"}); err == nil {
		t.Fatal("original image selected instead of derived manifest")
	}
	out, err := nativemedia.ProcessImage(t.Context(), f.original, "image/png")
	if err != nil {
		t.Fatal(err)
	}
	refs, err := f.plugin.library.saveOutputs(t.Context(), f.source, []nativemedia.Output{out})
	if err != nil || len(refs) != 1 {
		t.Fatal(err)
	}
	manifest, data, err := f.plugin.library.ReadDerivative(t.Context(), refs[0].URI)
	if err != nil || manifest.Source.SHA256 != f.source.SHA256 || !bytes.Equal(data, out.Data) {
		t.Fatal(err)
	}
	if _, err = f.invoke(t, e, "double-select", map[string]any{"action": "select", "manifest_uris": []string{refs[0].URI, refs[0].URI}, "reason": "duplicate"}); err == nil {
		t.Fatal("duplicate image selection accepted")
	}
}
