// INPUT: context, encoding/json, errors, os, os/exec, path/filepath, runtime, strings, sync, testing, time, kernel/abi, llm/wire, sandbox/abi
// OUTPUT: Actual compiled WASIp1 worker/host integration and isolation regressions
// POS: [platform / WASM host or guest] Executes the real shared SDK guest rather than treating cross-compilation as sandbox proof.
package host

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	kernelabi "github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"github.com/QuincySx/Alva/sdk/sandbox/abi"
)

var fixtureOnce sync.Once
var workerModule, probeModule []byte
var fixtureErr error
var testRunner = NewRunner()

func TestMain(m *testing.M) {
	code := m.Run()
	if err := testRunner.Close(context.Background()); err != nil && code == 0 {
		code = 1
	}
	os.Exit(code)
}
func runFixture(ctx context.Context, request Request, caps Capabilities) (Outcome, error) {
	return testRunner.Run(ctx, request, caps)
}

func modules(t *testing.T) ([]byte, []byte) {
	t.Helper()
	fixtureOnce.Do(func() {
		directory, err := os.MkdirTemp("", "alva-wasi-fixtures-")
		if err != nil {
			fixtureErr = err
			return
		}
		defer os.RemoveAll(directory)
		for _, item := range []struct {
			name, target string
			dest         *[]byte
		}{{"worker", "./cmd/alva-worker-wasm", &workerModule}, {"probe", "./platform/sandbox/host/testdata/probe", &probeModule}} {
			output := filepath.Join(directory, item.name+".wasm")
			command := exec.Command(filepath.Join(runtime.GOROOT(), "bin", "go"), "build", "-mod=readonly", "-o", output, item.target)
			command.Dir = "../../.."
			command.Env = append(os.Environ(), "GOOS=wasip1", "GOARCH=wasm", "GOPROXY=off")
			if data, err := command.CombinedOutput(); err != nil {
				fixtureErr = errors.New(err.Error() + ": " + string(data))
				return
			}
			data, err := os.ReadFile(output)
			if err != nil {
				fixtureErr = err
				return
			}
			*item.dest = data
		}
	})
	if fixtureErr != nil {
		t.Fatal(fixtureErr)
	}
	return workerModule, probeModule
}
func requestFor(t *testing.T, probe bool, mode string) Request {
	t.Helper()
	worker, fixture := modules(t)
	module := worker
	if probe {
		module = fixture
	}
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "in.txt"), []byte("allowed input"), 0600); err != nil {
		t.Fatal(err)
	}
	return Request{JobID: "test-job", Module: module, Args: []string{"fixture", mode}, Grants: []Grant{{Host: dir, Guest: "/work", Access: ReadWrite}}, Context: abi.Context{Version: abi.Version, SessionID: "guest-session", Workspace: "/work", Task: "local explicit task", MaxIterations: 8, Policy: abi.Policy{AllowFileWrite: true}}, Limits: Limits{WallClock: time.Minute}}
}

type modelFunc func(context.Context, kernelabi.ModelRequest, func(wire.StreamChunk) error) error

func (fn modelFunc) Stream(ctx context.Context, request kernelabi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(ctx, request, emit)
}
func emitCall(emit func(wire.StreamChunk) error, id, name, args string) error {
	if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: id, Name: name, Arguments: json.RawMessage(args)}}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: "tool_calls"})
}
func emitText(emit func(wire.StreamChunk) error, text string) error {
	if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: text}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: "stop"})
}

func TestWASIActualSDKModelToolModelAndReferenceRetrieval(t *testing.T) {
	request := requestFor(t, false, "")
	request.Context.SystemPrompt = "explicit host environment, guest paths only"
	var observed []IOEvent
	turn := 0
	var reference string
	model := modelFunc(func(_ context.Context, actual kernelabi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		serialized, _ := json.Marshal(actual)
		if strings.Contains(string(serialized), "HOST_KEY_ONLY") {
			t.Error("host secret reached guest model request")
		}
		if turn == 1 {
			if !strings.Contains(string(serialized), request.Context.SystemPrompt) {
				t.Error("host environment missing")
			}
			return emitCall(emit, "write", "write_file", `{"path":"created.txt","content":"guest-only contents"}`)
		}
		if turn == 2 {
			return emitCall(emit, "read", "read_file", `{"path":"created.txt"}`)
		}
		if turn == 3 {
			for _, message := range actual.Messages {
				for _, block := range message.Content {
					if block.ToolResult != nil && block.ToolResult.CallID == "read" {
						for _, content := range block.ToolResult.Content {
							if content.Reference != nil {
								reference = content.Reference.URI
							}
						}
					}
				}
			}
			if reference == "" || strings.Contains(string(serialized), `"Text":"guest-only contents"`) {
				t.Error("file not projected to reference")
			}
			args, _ := json.Marshal(map[string]any{"uri": reference, "offset": 0, "limit": 100})
			return emitCall(emit, "retrieve", "context_read", string(args))
		}
		if turn == 4 {
			if !strings.Contains(string(serialized), "guest-only contents") {
				t.Error("explicit retrieval did not return selected text")
			}
			return emitText(emit, "sandbox complete")
		}
		return errors.New("unexpected model turn")
	})
	outcome, err := runFixture(context.Background(), request, Capabilities{Model: model, Observer: ObserverFunc(func(_ context.Context, event IOEvent) error { observed = append(observed, event); return nil })})
	if err != nil || outcome.ExitCode != 0 || string(outcome.Stdout) != "sandbox complete" || turn != 4 {
		t.Fatal("real SDK guest failed", outcome, turn, err)
	}
	data, err := os.ReadFile(filepath.Join(request.Grants[0].Host, "created.txt"))
	if err != nil || string(data) != "guest-only contents" {
		t.Fatal("guest file effect missing", string(data), err)
	}
	modelRequests, audits := 0, 0
	for index, event := range observed {
		if event.Sequence != uint64(index+1) {
			t.Fatal("observer order", event.Sequence)
		}
		if event.Kind == "request" && event.Operation == abi.OpModel {
			modelRequests++
		}
		if event.Kind == "request" && event.Operation == abi.OpAudit {
			audits++
		}
	}
	if modelRequests != 4 || audits != 3 {
		t.Fatal("full proxy/audit evidence missing", modelRequests, audits)
	}
}

func TestWASIFilesystemNoTraversalSymlinkOrReadOnlyEscape(t *testing.T) {
	for _, readOnly := range []bool{false, true} {
		t.Run(map[bool]string{true: "read-only", false: "read-write"}[readOnly], func(t *testing.T) {
			request := requestFor(t, true, "files")
			outside := t.TempDir()
			if err := os.WriteFile(filepath.Join(outside, "secret.txt"), []byte("outside-secret"), 0600); err != nil {
				t.Fatal(err)
			}
			if err := os.Symlink(filepath.Join(outside, "secret.txt"), filepath.Join(request.Grants[0].Host, "escape")); err != nil {
				t.Fatal(err)
			}
			if err := os.Symlink(outside, filepath.Join(request.Grants[0].Host, "linkdir")); err != nil {
				t.Fatal(err)
			}
			if readOnly {
				request.Grants[0].Access = ReadOnly
			}
			outcome, err := runFixture(context.Background(), request, Capabilities{})
			if err != nil || outcome.ExitCode != 0 || !strings.Contains(string(outcome.Stdout), "escape=denied") {
				t.Fatal("real WASI escape check failed", string(outcome.Stdout), string(outcome.Stderr), err)
			}
			_, statErr := os.Stat(filepath.Join(request.Grants[0].Host, "out.txt"))
			if readOnly && !errors.Is(statErr, os.ErrNotExist) {
				t.Fatal("read-only mount permitted write", statErr)
			}
			if !readOnly && statErr != nil {
				t.Fatal("real mounted write failed", statErr)
			}
		})
	}
}

func TestWASIDoesNotInheritEnvironmentOrOtherJobMounts(t *testing.T) {
	t.Setenv("ALVA_SECRET_TEST", "HOST_KEY_ONLY")
	request := requestFor(t, true, "environment")
	outcome, err := runFixture(context.Background(), request, Capabilities{})
	if err != nil || outcome.ExitCode != 0 || strings.Contains(string(outcome.Stdout), "HOST_KEY_ONLY") || !strings.Contains(string(outcome.Stdout), "PWD=/work") {
		t.Fatal("ambient environment leak", string(outcome.Stdout), err)
	}
	request = requestFor(t, true, "files")
	request.Grants = nil
	if _, err := runFixture(context.Background(), request, Capabilities{}); err == nil {
		t.Fatal("previous job filesystem authority leaked")
	}
}

func TestWASICancelWaitsForActualHostModelExitAndKeepsFailure(t *testing.T) {
	request := requestFor(t, false, "")
	entered := make(chan struct{})
	canceled := make(chan struct{})
	release := make(chan struct{})
	disk := errors.New("private model cleanup failure")
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	model := modelFunc(func(ctx context.Context, _ kernelabi.ModelRequest, _ func(wire.StreamChunk) error) error {
		close(entered)
		<-ctx.Done()
		close(canceled)
		<-release
		return errors.Join(ctx.Err(), disk)
	})
	done := make(chan error, 1)
	go func() { _, err := runFixture(ctx, request, Capabilities{Model: model}); done <- err }()
	select {
	case <-entered:
	case <-time.After(45 * time.Second):
		t.Fatal("actual guest never called model")
	}
	cancel()
	<-canceled
	select {
	case err := <-done:
		t.Fatal("cancellation fabricated completion while callback still running", err)
	case <-time.After(30 * time.Millisecond):
	}
	close(release)
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) || !errors.Is(err, disk) || strings.Contains(err.Error(), "private model cleanup failure") {
			t.Fatal("failure evidence lost/leaked", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("callback exited but guest not reclaimed")
	}
}

func TestWASISpinDeadlineAndMemoryOutputCeilingsAreReal(t *testing.T) {
	if _, err := runFixture(context.Background(), requestFor(t, true, "environment"), Capabilities{}); err != nil {
		t.Fatal("warm code only", err)
	}
	for _, mode := range []string{"spin", "memory", "output"} {
		t.Run(mode, func(t *testing.T) {
			request := requestFor(t, true, mode)
			request.Limits.WallClock = 2 * time.Second
			request.Limits.MemoryBytes = 64 << 20
			request.Limits.OutputBytes = 1024
			outcome, err := runFixture(context.Background(), request, Capabilities{})
			if err == nil && outcome.ExitCode == 0 {
				t.Fatal("resource attack escaped actual limiter", mode)
			}
			if mode == "spin" && (!errors.Is(err, context.DeadlineExceeded) || !kernelabi.OnlyCancellation(err) || !strings.Contains(string(outcome.Stdout), "spin entered")) {
				t.Fatal("did not prove guest spin actually interrupted", string(outcome.Stdout), err)
			}
			if len(outcome.Stdout) > 1024 || len(outcome.Stderr) > 1024 {
				t.Fatal("output capture exceeded bound")
			}
		})
	}
}

func TestWASIMalformedABINeverReachesHostModel(t *testing.T) {
	request := requestFor(t, true, "bad-abi")
	calls := 0
	_, err := runFixture(context.Background(), request, Capabilities{Model: modelFunc(func(context.Context, kernelabi.ModelRequest, func(wire.StreamChunk) error) error { calls++; return nil })})
	if err == nil || calls != 0 {
		t.Fatal("malformed ABI reached capability", err, calls)
	}
}

func TestWASIObserverFailureIsFatalPrivateAndNotFalseSuccess(t *testing.T) {
	request := requestFor(t, false, "")
	disk := &os.PathError{Op: "append", Path: "/private/journal/key", Err: errors.New("secret disk reason")}
	_, err := runFixture(context.Background(), request, Capabilities{Model: modelFunc(func(_ context.Context, _ kernelabi.ModelRequest, emit func(wire.StreamChunk) error) error {
		return emitText(emit, "answer")
	}), Observer: ObserverFunc(func(_ context.Context, event IOEvent) error {
		if event.Kind == "response" && event.Operation == abi.OpModel {
			return disk
		}
		return nil
	})})
	if !errors.Is(err, disk) || strings.Contains(err.Error(), "/private/") || strings.Contains(err.Error(), "secret disk") {
		t.Fatal("observer evidence lost/leaked", err)
	}
}
