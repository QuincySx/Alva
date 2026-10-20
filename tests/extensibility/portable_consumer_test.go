// INPUT: context, JSON/os/exec/path/runtime/testing/time and independent portable consumer source.
// OUTPUT: Actual native CLI subprocess plan/question/schema execution regressions.
// POS: [verification] Real public-SDK embedding proof; portable binaries are separately executed by Node and wazero in CI.
//go:build !js && !wasip1

package extensibility_test

import (
	"context"
	"encoding/json"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func TestIndependentPortableCLIConsumer(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 2*time.Minute)
	defer cancel()
	name := "consumer"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	binary := filepath.Join(t.TempDir(), name)
	build := exec.CommandContext(ctx, "go", "build", "-mod=readonly", "-o", binary, "./testdata/portable-agent")
	if output, err := build.CombinedOutput(); err != nil {
		t.Fatalf("consumer build: %v\n%s", err, output)
	}
	for _, mode := range []string{"plan", "question", "schema"} {
		t.Run(mode, func(t *testing.T) {
			output, err := exec.CommandContext(ctx, binary, mode).CombinedOutput()
			var receipt map[string]json.RawMessage
			if err != nil || json.Unmarshal(output, &receipt) != nil {
				t.Fatalf("consumer %s: %v\n%s", mode, err, output)
			}
			key := map[string]string{"plan": "saved", "question": "answer_value", "schema": "schema"}[mode]
			if len(receipt[key]) == 0 {
				t.Fatalf("missing actual output %s: %s", key, output)
			}
		})
	}
}
