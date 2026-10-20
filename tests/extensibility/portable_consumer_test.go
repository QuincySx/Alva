// INPUT: context, JSON/os/exec/path/runtime/testing/time and independent portable consumer source.
// OUTPUT: Actual native CLI subprocess plan/question/schema execution regressions.
// POS: [verification] Real public-SDK embedding proof; portable binaries are separately executed by Node and wazero in CI.
//go:build !js && !wasip1

package extensibility_test

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
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

func TestExternalModuleSDKConsumer(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 2*time.Minute)
	defer cancel()
	dir := filepath.Join("..", "..", "examples", "sdk-agent")
	command := func(args ...string) *exec.Cmd {
		cmd := exec.CommandContext(ctx, "go", args...)
		cmd.Dir = dir
		cmd.Env = append(os.Environ(), "GOWORK=off")
		return cmd
	}
	output, err := command("list", "-mod=readonly", "-m").CombinedOutput()
	if err != nil || strings.TrimSpace(string(output)) != "example.com/alva-sdk-agent" {
		t.Fatalf("consumer is not an independent module: %v\n%s", err, output)
	}
	output, err = command("list", "-mod=readonly", "-deps", "-f", "{{if not .Standard}}{{.ImportPath}}{{end}}", ".").CombinedOutput()
	if err != nil {
		t.Fatalf("consumer dependencies: %v\n%s", err, output)
	}
	for _, dependency := range strings.Fields(string(output)) {
		if dependency != "example.com/alva-sdk-agent" && !strings.HasPrefix(dependency, "github.com/QuincySx/Alva/sdk/") {
			t.Fatalf("SDK consumer acquired a non-SDK dependency: %s", dependency)
		}
	}
	name := "external-agent"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	binary := filepath.Join(t.TempDir(), name)
	if output, err := command("build", "-mod=readonly", "-o", binary, ".").CombinedOutput(); err != nil {
		t.Fatalf("external consumer build: %v\n%s", err, output)
	}
	for _, mode := range []string{"run", "cancel"} {
		t.Run(mode, func(t *testing.T) {
			output, err := exec.CommandContext(ctx, binary, mode).CombinedOutput()
			var receipt struct {
				Mode       string `json:"mode"`
				Answer     string `json:"answer"`
				ModelCalls int    `json:"model_calls"`
				ToolCalls  int    `json:"tool_calls"`
			}
			if err != nil || json.Unmarshal(output, &receipt) != nil || receipt.Mode != mode || receipt.ToolCalls != 1 {
				t.Fatalf("external consumer receipt: %v\n%s", err, output)
			}
			if mode == "run" && (receipt.ModelCalls != 2 || receipt.Answer != "Hello, Alva") || mode == "cancel" && (receipt.ModelCalls != 1 || receipt.Answer != "") {
				t.Fatalf("unexpected settled receipt: %s", output)
			}
		})
	}
}
