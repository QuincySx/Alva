// INPUT: Granted Unix FIFOs and explicitly invoked QuickJS file/readDir bindings
// OUTPUT: Non-blocking special-file rejection and actual owned engine return
// POS: [platform / WASM host] A granted path does not authorize blocking special-device IO or fabricate timeout cleanup
//go:build unix

package host

import (
	"path/filepath"
	"strings"
	"testing"
	"time"

	"golang.org/x/sys/unix"
)

func TestScriptFileBindingsNeverBlockOpeningGrantedFIFO(t *testing.T) {
	j, dir := scriptFixture(t)
	warmStart := time.Now()
	if warm := evaluate(t, j, `0`); warm.Error != "" {
		t.Fatal("warm engine", warm)
	}
	startup := time.Since(warmStart)
	if err := unix.Mkfifo(filepath.Join(dir, "fifo"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, source := range []string{`readFile('fifo')`, `readDir('fifo')`, `writeFile('fifo','forbidden')`} {
		start := time.Now()
		result := evaluate(t, j, source)
		// Include the measured fresh engine startup; a FIFO rejection must
		// still be prompt and must never rely on the script deadline to return.
		if result.Error == "" || strings.Contains(result.Error, "deadline") || time.Since(start) > startup+3*time.Second {
			t.Fatal("FIFO opened or blocked", source, result, time.Since(start))
		}
	}
}
