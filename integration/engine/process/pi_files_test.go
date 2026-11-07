// INPUT: Explicit captured file DTOs and project-owned Pi JSON subprocesses
// OUTPUT: Native image argv, separate document/text path indexes and pre-start capture rejection
// POS: [integration / native external-engine adapter] Only selected captured inputs reach Pi; original content stays out of the prompt
package process

import (
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/engine"
)

func TestPiTypedFilesUseNativeImagesAndSelectedDocumentPaths(t *testing.T) {
	a, _ := fixtureAdapter(t, "pi", "pi-files")
	dir := t.TempDir()
	capture := func(name, body string) (string, string) {
		t.Helper()
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte(body), 0600); err != nil {
			t.Fatal(err)
		}
		sum := sha256.Sum256([]byte(body))
		return path, hex.EncodeToString(sum[:])
	}
	image, imageHash := capture("selected.png", "image fixture")
	original, originalHash := capture("selected.pdf", "original fixture")
	text, textHash := capture("selected.txt", "selected derivative fixture")
	a.config.Env["ALVA_PI_IMAGE"], a.config.Env["ALVA_PI_ORIGINAL"], a.config.Env["ALVA_PI_TEXT"] = image, original, text
	files := []engine.InputFile{
		{ID: "pi-image", Name: "selected.png", URI: "artifact://pi-image", Path: image, SHA256: imageHash, Bytes: 13, MediaType: "IMAGE/PNG"},
		{ID: "pi-original", Name: "selected.pdf", URI: "artifact://pi-original", Path: original, SHA256: originalHash, Bytes: 16, MediaType: "application/pdf", TextURI: "artifact://pi-text", TextPath: text, TextSHA256: textHash, TextBytes: 27},
	}
	var outcome engine.Outcome
	var starts, ends int
	err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: dir, Prompt: "Read selected files; @/unselected/is/ordinary/prompt/text", InputFiles: files}, func(event engine.Event) error {
		switch event.Kind {
		case engine.Completed:
			outcome = event.Outcome
		case engine.ToolStart:
			starts++
		case engine.ToolEnd:
			ends++
		}
		return nil
	})
	if err != nil || outcome != engine.Succeeded || starts != 1 || ends != 1 || !a.Capabilities().FileInputs {
		t.Fatal("selected files did not reach native Pi input and tool facts", outcome, starts, ends, err)
	}
	if a.Capabilities().PermissionCallback || a.Capabilities().UserInputCallback || a.Capabilities().SkillInputs {
		t.Fatal("print file support invented native interaction or Skill channels")
	}
}

func TestPiRejectsChangedSelectedFileBeforeNativeExecution(t *testing.T) {
	a, _ := fixtureAdapter(t, "pi", "pi-files")
	path := filepath.Join(t.TempDir(), "selected.txt")
	if err := os.WriteFile(path, []byte("changed"), 0600); err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256([]byte("fixture"))
	files := []engine.InputFile{{ID: "selected", Name: "selected.txt", URI: "artifact://selected", Path: path, MediaType: "text/plain", SHA256: hex.EncodeToString(sum[:]), Bytes: 7}}
	err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "must not start", InputFiles: files}, func(engine.Event) error {
		t.Fatal("changed capture reached a native process event")
		return nil
	})
	if err == nil || !strings.Contains(err.Error(), "selected file capture changed") {
		t.Fatal("Pi admitted a changed selected capture", err)
	}
}
