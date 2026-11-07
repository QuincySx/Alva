// INPUT: Explicit captured PNG, local HTTP model fixture and production Alva media assembly
// OUTPUT: Actual selected-image payload, native provenance/receipts, default tools off and no history replay regressions
// POS: [L7 Hub assembly tests] No external model, decoder, installed CLI or account is executed
package bootstrap

import (
	"bytes"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestCapturedImagesReachNativeModelOnceWithMediaToolsDisabled(t *testing.T) {
	for _, capable := range []bool{false, true} {
		t.Run(fmt.Sprint(capable), func(t *testing.T) {
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				body, err := io.ReadAll(io.LimitReader(r.Body, 2<<20))
				if err != nil {
					t.Error(err)
				}
				round := calls.Add(1)
				want := 0
				if round == 1 {
					want = 1
				}
				if count := bytes.Count(body, []byte(`"type":"image_url"`)); count != want {
					t.Errorf("round %d got %d images want %d", round, count, want)
				}
				if bytes.Contains(body, []byte(`"name":"hub_media"`)) || bytes.Contains(body, []byte("hub_media explicitly")) || bytes.Contains(body, []byte("data:image/png")) {
					t.Error("broad media Tool or original PNG entered request")
				}
				w.Header().Set("Content-Type", "text/event-stream")
				fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"selected image received\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
			}))
			defer server.Close()
			a, err := Open(t.Context(), Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "data"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "selected-image-route"}, ImageCapable: capable})
			if err != nil {
				t.Fatal(err)
			}
			defer closeApp(t, a)
			workspace := t.TempDir()
			picture := image.NewRGBA(image.Rect(0, 0, 12, 8))
			picture.Set(1, 1, color.RGBA{R: 255, A: 255})
			var encoded bytes.Buffer
			if err := png.Encode(&encoded, picture); err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(workspace, "selected.png")
			if err := os.WriteFile(path, encoded.Bytes(), 0600); err != nil {
				t.Fatal(err)
			}
			captured, err := a.Service.CaptureComposerFile(t.Context(), hub.ComposerDraftScope{Kind: "home", ID: "selected-images"}, path)
			if err != nil {
				t.Fatal(err)
			}
			draft, err := a.Service.ComposerDrafts.Save(t.Context(), hub.ComposerDraft{Scope: captured.Scope, Text: "Inspect the explicitly selected image", References: []hub.ComposerDraftReference{product.SelectedComposerFile(captured).DraftSelection()}}, 0)
			if err != nil {
				t.Fatal(err)
			}
			owner, _, err := a.Service.CreateComposerSession(t.Context(), draft, "selected images", workspace)
			if err != nil {
				t.Fatal(err)
			}
			settings, err := a.Store.Settings(t.Context(), owner.ID)
			if err != nil || settings.MediaEnabled {
				t.Fatal("media tools were enabled before selection", err)
			}
			if _, err := a.Service.SubmitComposerFileDraft(t.Context(), owner.ID, "Inspect the explicitly selected image", nil, nil, []product.ComposerFileSelection{product.SelectedComposerFile(captured)}); err != nil {
				t.Fatal(err)
			}
			detail := awaitMediaTerminal(t, a, owner.ID)
			if !capable {
				if calls.Load() != 0 || detail.Session.Status != engine.Failed {
					t.Fatal("unknown image route pretended to accept image", calls.Load(), detail.Session.Status)
				}
				return
			}
			if detail.Session.Status != engine.Succeeded || calls.Load() != 1 {
				t.Fatal("selected image failed actual model execution", detail.Session.Status, calls.Load())
			}
			branch, err := product.ActiveBranch(detail)
			if err != nil {
				t.Fatal(err)
			}
			sources, err := a.Repository.PageArtifacts(t.Context(), owner.ID, branch.NativeSessionID, "input-attachment", "", 0, 0, 4)
			if err != nil || len(sources.Entries) != 1 || sources.Entries[0].Info.SHA256 != captured.SHA256 {
				t.Fatal("native provenance copy missing", err)
			}
			selections, err := a.Repository.PageArtifacts(t.Context(), owner.ID, branch.NativeSessionID, "media-selection", "", 0, 0, 4)
			if err != nil || len(selections.Entries) != 1 {
				t.Fatal("selected image evidence missing", err)
			}
			receipts, err := a.Repository.PageArtifacts(t.Context(), owner.ID, branch.NativeSessionID, "media-receipt", selections.Entries[0].Info.Name, 0, 0, 4)
			if err != nil || len(receipts.Entries) != 3 {
				t.Fatal("selected image receipt chain missing", err)
			}
			settings, _ = a.Store.Settings(t.Context(), owner.ID)
			if settings.MediaEnabled {
				t.Fatal("selected image persisted broad media enablement")
			}
			if _, err := a.Service.Submit(t.Context(), owner.ID, "ordinary later text", nil); err != nil {
				t.Fatal(err)
			}
			if after := awaitMediaTerminal(t, a, owner.ID); after.Session.Status != engine.Succeeded || calls.Load() != 2 {
				t.Fatal("ordinary continuation replayed or failed", calls.Load())
			}
			privatePath, err := a.Service.ComposerFilePrivatePath(t.Context(), product.SelectedComposerFile(captured), false)
			if err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(privatePath, []byte("changed captured bytes"), 0600); err != nil {
				t.Fatal(err)
			}
			if _, err := a.Service.SubmitComposerFileDraft(t.Context(), owner.ID, "changed source must fail", nil, nil, []product.ComposerFileSelection{product.SelectedComposerFile(captured)}); err == nil || !strings.Contains(err.Error(), "changed") {
				t.Fatal("changed source was admitted", err)
			}
			if calls.Load() != 2 {
				t.Fatal("changed source triggered model request")
			}
		})
	}
}
