// INPUT: Checked-in JavaScript and independently compiled WASIp1 plugins, pinned Hub settings and real assembly.
// OUTPUT: Inspect/approve without execution, next-turn actual protocol callbacks, durable facts and joined cleanup.
// POS: [L7 Hub product] Verifies both external plugin forms reach the same product recipe without kernel changes.
package bootstrap

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestPortableAEPPinsNextTurnActualLaunchAndCleanup(t *testing.T) {
	for _, runtime := range []string{"javascript", "wasm"} {
		t.Run(runtime, func(t *testing.T) {
			root, err := filepath.Abs("../../..")
			if err != nil {
				t.Fatal(err)
			}
			source := t.TempDir()
			entry := hub.AEPBundle{ID: "portable", SourceDirectory: source}
			if runtime == "javascript" {
				node, err := exec.LookPath("node")
				if err != nil {
					t.Skip("optional Node runtime not installed; CI requires it explicitly")
				}
				entry.Interpreter = node
				for destination, path := range map[string]string{"alva.toml": "examples/alva.toml", "echo.mjs": "examples/echo.mjs", "alva-sdk.mjs": "index.mjs"} {
					data, err := os.ReadFile(filepath.Join(root, "integration/protocol/aep/javascript", path))
					if err != nil {
						t.Fatal(err)
					}
					if err = os.WriteFile(filepath.Join(source, destination), data, 0600); err != nil {
						t.Fatal(err)
					}
				}
			} else {
				data, err := os.ReadFile(filepath.Join(root, "examples/aep-wasm/alva.toml"))
				if err != nil {
					t.Fatal(err)
				}
				if err = os.WriteFile(filepath.Join(source, "alva.toml"), data, 0600); err != nil {
					t.Fatal(err)
				}
				ctx, cancel := context.WithTimeout(t.Context(), 2*time.Minute)
				defer cancel()
				cmd := exec.CommandContext(ctx, "go", "build", "-mod=readonly", "-o", filepath.Join(source, "plugin.wasm"), ".")
				cmd.Dir = filepath.Join(root, "examples/aep-wasm")
				cmd.Env = append(os.Environ(), "GOOS=wasip1", "GOARCH=wasm", "CGO_ENABLED=0", "GOWORK=off", "GOPROXY=off", "GOSUMDB=off")
				if out, err := cmd.CombinedOutput(); err != nil {
					t.Fatalf("WASM author build: %v / %s", err, out)
				}
			}
			a, err := Open(t.Context(), Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true})
			if err != nil {
				t.Fatal(err)
			}
			defer closeApp(t, a)
			session, err := a.Service.Create(t.Context(), "portable plugin", "")
			if err != nil {
				t.Fatal(err)
			}
			raw, _ := json.Marshal([]hub.AEPBundle{entry})
			if err = a.Service.SaveAEPDraft(t.Context(), session.ID, string(raw)); err != nil {
				t.Fatal(err)
			}
			settings, err := a.Store.Settings(t.Context(), session.ID)
			if err != nil || len(settings.AEP) != 1 || settings.AEP[0].Enabled || settings.AEP[0].HostAccessApproved {
				t.Fatal("inspect approved execution", err)
			}
			if a.Runtime.HasActive() {
				t.Fatal("inspection started runtime")
			}
			assertAEPSnapshotsRemoved(t, a)
			if err = a.Store.SetAEPEnabled(t.Context(), session.ID, entry.ID, true, true); err != nil {
				t.Fatal(err)
			}
			if a.Runtime.HasActive() {
				t.Fatal("approval started runtime")
			}
			if _, err = a.Service.Submit(t.Context(), session.ID, "local portable plugin demonstration"); err != nil {
				t.Fatal(err)
			}
			// Cold WASM compilation belongs to actual attempt preparation.
			ctx, stop := context.WithTimeout(t.Context(), 30*time.Second)
			defer stop()
			var detail hub.Detail
			for {
				detail, err = a.Store.Detail(ctx, session.ID)
				if err != nil {
					t.Fatal(err)
				}
				if detail.Session.Status != hub.Running && !a.Runtime.HasActive() {
					break
				}
				select {
				case <-ctx.Done():
					t.Fatal("actual plugin attempt did not finish")
				case <-time.After(time.Millisecond):
				}
			}
			if detail.Session.Status != engine.Succeeded {
				t.Fatal("portable plugin attempt failed", detail.Session.Status)
			}
			assertAEPSnapshotsRemoved(t, a)
			branch, err := product.ActiveBranch(detail)
			if err != nil {
				t.Fatal(err)
			}
			log, err := a.Repository.OpenSession(t.Context(), session.ID, branch.NativeSessionID)
			if err != nil {
				t.Fatal(err)
			}
			records, err := log.Records(t.Context(), 0)
			if err != nil {
				t.Fatal(err)
			}
			callbacks, shutdown := false, false
			for _, record := range records {
				if record.Namespace != "alva.aep" {
					continue
				}
				callbacks = callbacks || strings.Contains(string(record.Payload), `"method":"host/state.get_metadata"`)
				shutdown = shutdown || strings.Contains(string(record.Payload), `"method":"shutdown"`)
			}
			if !callbacks || !shutdown {
				t.Fatal("actual scoped callback or graceful shutdown missing", callbacks, shutdown)
			}
		})
	}
}
