// INPUT: Explicit JS/Python/WASM manifests and real private Hub product settings.
// OUTPUT: Runtime binding, inspect-without-execute, approval and immutable pin acceptance.
// POS: [L7 Hub product] Selection controls components; invalid modes never fall back to native execution.
package product

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/system/privatefs"
)

func TestAEPRuntimeSelectionIsExplicitAndInspectionDoesNotExecute(t *testing.T) {
	s, requests := fixture(t)
	session, err := s.Create(t.Context(), "portable plugins", "")
	if err != nil {
		t.Fatal(err)
	}
	for _, runtime := range []string{"javascript", "python", "wasm"} {
		t.Run(runtime, func(t *testing.T) {
			source := t.TempDir()
			entry := map[string]string{"javascript": "main.mjs", "python": "main.py", "wasm": "plugin.wasm"}[runtime]
			manifest := "name='runtime-fixture'\nversion='1'\nruntime='" + runtime + "'\nentry='" + entry + "'\n"
			if err := os.WriteFile(filepath.Join(source, "alva.toml"), []byte(manifest), 0600); err != nil {
				t.Fatal(err)
			}
			// Deliberately invalid program: inspection pins bytes, never executes
			// or compiles them, including in the embedded runtime.
			if err := os.WriteFile(filepath.Join(source, entry), []byte("not executable code"), 0600); err != nil {
				t.Fatal(err)
			}
			selected := hub.AEPBundle{ID: "portable", SourceDirectory: source, Enabled: true, HostAccessApproved: true}
			if runtime != "wasm" {
				selected.Interpreter = filepath.Join(source, "explicit-not-started-interpreter")
			}
			raw, _ := json.Marshal([]hub.AEPBundle{selected})
			if err := s.SaveAEPDraft(t.Context(), session.ID, string(raw)); err != nil {
				t.Fatal("inspection executed or rejected bounded bytes", err)
			}
			settings, err := s.Store.Settings(t.Context(), session.ID)
			if err != nil || len(settings.AEP) != 1 {
				t.Fatal(err)
			}
			pin := settings.AEP[0]
			if pin.Enabled || pin.HostAccessApproved || len(pin.SHA256) != 64 {
				t.Fatal("inspection self-approved or omitted code pin", pin)
			}
			parent, err := privatefs.EnsureDir(filepath.Join(t.TempDir(), "private"))
			if err != nil {
				t.Fatal(err)
			}
			if _, err = StagePinnedAEP(t.Context(), pin, parent); err == nil {
				t.Fatal("unapproved pin staged for execution")
			}
			if err = s.Store.SetAEPEnabled(t.Context(), session.ID, pin.ID, true, true); err != nil {
				t.Fatal(err)
			}
			pin.Enabled, pin.HostAccessApproved = true, true
			bundle, err := StagePinnedAEP(t.Context(), pin, parent)
			if err != nil {
				t.Fatal(err)
			}
			if err = bundle.Close(); err != nil {
				t.Fatal(err)
			}
			if err = os.WriteFile(filepath.Join(source, entry), []byte("changed after approval"), 0600); err != nil {
				t.Fatal(err)
			}
			if _, err = StagePinnedAEP(t.Context(), pin, parent); err == nil {
				t.Fatal("changed code accepted with old approval")
			}
			bad := selected
			if runtime == "wasm" {
				bad.Interpreter = filepath.Join(source, "must-not-run")
			} else {
				bad.Interpreter = ""
			}
			raw, _ = json.Marshal([]hub.AEPBundle{bad})
			if err = s.SaveAEPDraft(t.Context(), session.ID, string(raw)); err == nil {
				t.Fatal("runtime/interpreter mismatch accepted")
			}
			settings, err = s.Store.Settings(t.Context(), session.ID)
			if err != nil || settings.AEP[0].SHA256 != pin.SHA256 || !settings.AEP[0].Enabled {
				t.Fatal("rejected draft changed accepted settings", err)
			}
			if s.Runtime.HasActive() || len(requests) != 0 {
				t.Fatal("inspection/approval/staging started an Agent")
			}
		})
	}
}
