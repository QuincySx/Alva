// INPUT: Actual local OpenAI HTTP loop, private uploaded PNG and explicit native media opt-in
// OUTPUT: Upload exclusion, real derivative, route-bound one-request selection and restart evidence regressions
// POS: [L7 Hub product] Shared production recipe tested without paid provider, decoder discovery or auto media execution
package bootstrap

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/apps/hub/product"
	"github.com/QuincySx/Alva/go/integration/engine"
	"github.com/QuincySx/Alva/go/integration/llm/provider"
	"github.com/QuincySx/Alva/go/platform/storage/session"
)

func TestNativeMediaActualRecipeIsExplicitAndSelectedForOneRequest(t *testing.T) {
	for _, capable := range []bool{false, true} {
		t.Run(fmt.Sprint("image-capable-", capable), func(t *testing.T) {
			ctx := context.Background()
			var calls atomic.Int32
			var a *Application
			var productID, nativeID, branchID, sourceURI string
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				body, err := io.ReadAll(io.LimitReader(r.Body, 1<<20))
				if err != nil {
					t.Error(err)
				}
				round := calls.Add(1)
				images := bytes.Count(body, []byte(`"type":"image_url"`))
				if images != 0 && (round != 3 || !capable) || round == 3 && capable && images != 1 {
					t.Errorf("wrong selected image scope round=%d images=%d", round, images)
				}
				if bytes.Contains(body, []byte("data:image/png")) {
					t.Error("original upload entered model input")
				}
				var delta map[string]any
				finish := "stop"
				if round == 1 || round == 2 {
					args := map[string]any{"action": "process_image", "source_uri": sourceURI}
					if round == 2 {
						page, e := a.Service.MediaIndex(ctx, productID, branchID, sourceURI, 0, 0)
						if e != nil || len(page.Entries) != 1 {
							t.Errorf("no actual derived manifest: %+v %v", page, e)
						}
						manifestURI := "missing"
						if len(page.Entries) == 1 {
							manifestURI = page.Entries[0].Info.Reference().URI
						}
						args = map[string]any{"action": "select", "manifest_uris": []string{manifestURI}, "reason": "explicit requested plot only"}
					}
					encoded, _ := json.Marshal(args)
					delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": fmt.Sprint("media-", round), "type": "function", "function": map[string]any{"name": "hub_media", "arguments": string(encoded)}}}}
					finish = "tool_calls"
				} else {
					delta = map[string]any{"content": "actual media fixture completed"}
				}
				data, _ := json.Marshal(map[string]any{"id": "actual-media-fixture", "choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
				w.Header().Set("Content-Type", "text/event-stream")
				fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
			}))
			defer server.Close()
			dir := filepath.Join(t.TempDir(), "private")
			config := Config{DataDirectory: dir, ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "actual-image-route"}, ImageCapable: capable}
			a, err := Open(ctx, config)
			if err != nil {
				t.Fatal(err)
			}
			defer func() { closeApp(t, a) }()
			s, err := a.Service.Create(ctx, "media", "")
			if err != nil {
				t.Fatal(err)
			}
			productID = s.ID
			if err = a.Store.SetMediaEnabled(ctx, s.ID, true); err != nil {
				t.Fatal(err)
			}
			var original bytes.Buffer
			picture := image.NewRGBA(image.Rect(0, 0, 12, 8))
			picture.Set(1, 1, color.RGBA{R: 255, A: 255})
			if err = png.Encode(&original, picture); err != nil {
				t.Fatal(err)
			}
			upload, err := a.Service.UploadReader(ctx, s.ID, "explicit.png", bytes.NewReader(original.Bytes()))
			if err != nil {
				t.Fatal(err)
			}
			detail, err := a.Store.Detail(ctx, s.ID)
			if err != nil {
				t.Fatal(err)
			}
			branch, err := product.ActiveBranch(detail)
			if err != nil {
				t.Fatal(err)
			}
			nativeID, branchID, sourceURI = branch.NativeSessionID, branch.ID, upload.Reference.URI
			before, err := a.Service.MediaIndex(ctx, s.ID, branch.ID, sourceURI, 0, 0)
			if err != nil || len(before.Entries) != 0 || calls.Load() != 0 {
				t.Fatal("upload/preference executed media", err)
			}
			if _, err = a.Service.Submit(ctx, s.ID, "derive uploaded image, inspect only the selected derivative"); err != nil {
				t.Fatal(err)
			}
			terminal := awaitMediaTerminal(t, a, s.ID)
			wantCalls, wantOutcome := int32(3), engine.Succeeded
			if !capable {
				wantCalls, wantOutcome = 2, engine.Failed
			}
			if calls.Load() != wantCalls || terminal.Session.Status != wantOutcome {
				t.Fatal("wrong actual media terminal", calls.Load(), terminal.Session.Status)
			}
			entries, err := a.Service.MediaIndex(ctx, s.ID, branch.ID, sourceURI, 0, 0)
			if err != nil || len(entries.Entries) != 1 {
				t.Fatal("manifest missing", err)
			}
			manifest, preview, err := a.Service.MediaPreview(ctx, s.ID, branch.ID, entries.Entries[0].Info.Reference().URI)
			if err != nil || len(preview) == 0 || manifest.Artifact.MediaType != "image/jpeg" || len(manifest.Output.Losses) == 0 || manifest.Source.SHA256 != upload.Info.SHA256 {
				t.Fatal("actual provenance/preview missing", err)
			}
			_, saved, err := a.Repository.ReadArtifactFull(ctx, s.ID, nativeID, sourceURI, session.MaxArtifactBytes)
			if err != nil || !bytes.Equal(saved, original.Bytes()) {
				t.Fatal("original bytes changed", err)
			}
			selections, err := a.Repository.PageArtifacts(ctx, s.ID, nativeID, "media-selection", "", 0, 0, 4)
			if err != nil {
				t.Fatal(err)
			}
			if capable && len(selections.Entries) != 1 || !capable && len(selections.Entries) != 0 {
				t.Fatal("unknown image route admitted selection", selections)
			}
			if capable {
				receipts, e := a.Service.MediaReceipts(ctx, s.ID, branch.ID, selections.Entries[0].Info.Name, 0, 0)
				if e != nil || len(receipts.Entries) != 3 {
					t.Fatal("actual receipt chain absent", e)
				}
				_, receipt, e := a.Repository.ReadArtifactFull(ctx, s.ID, nativeID, receipts.Entries[1].Info.Reference().URI, 16<<10)
				if e != nil || !strings.Contains(string(receipt), "model_chain_invocation_intent") || !strings.Contains(string(receipt), "actual-image-route") {
					t.Fatal("actual invocation identity absent", e, string(receipt))
				}
			}
			closeApp(t, a)
			a, err = Open(ctx, config)
			if err != nil {
				t.Fatal(err)
			}
			if _, _, err = a.Service.MediaPreview(ctx, s.ID, branch.ID, entries.Entries[0].Info.Reference().URI); err != nil {
				t.Fatal("durable media preview lost", err)
			}
			if _, err = a.Service.Submit(ctx, s.ID, "next turn; do not select again"); err != nil {
				t.Fatal(err)
			}
			awaitTerminal(t, a, s.ID)
			if calls.Load() != wantCalls+1 {
				t.Fatal("unexpected selection/model replay", calls.Load())
			}
		})
	}
}

func awaitMediaTerminal(t *testing.T, a *Application, id string) hub.Detail {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		detail, err := a.Store.Detail(context.Background(), id)
		if err != nil {
			t.Fatal(err)
		}
		pending, err := a.Service.PendingApprovals(context.Background(), id)
		if err != nil {
			t.Fatal(err)
		}
		for _, request := range pending {
			if request.ToolName != "hub_media" {
				t.Fatal("unexpected fixture tool", request.ToolName)
			}
			if err = a.Service.RespondApproval(context.Background(), id, request.ID, true); err != nil {
				t.Fatal(err)
			}
		}
		if detail.Session.Status != hub.Running && !a.Runtime.HasActive() {
			return detail
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("actual media loop did not settle")
	return hub.Detail{}
}

func TestDemoCannotClaimOperatorImageCapability(t *testing.T) {
	if _, err := Open(context.Background(), Config{DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true, ImageCapable: true}); err == nil {
		t.Fatal("demo pretended to understand images")
	}
}
