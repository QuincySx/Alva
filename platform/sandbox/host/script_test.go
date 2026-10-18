// INPUT: Pinned actual QuickJS WASM, private job roots, local HTTP and the shared SDK guest
// OUTPUT: Real modern JS/resizable typed arrays, bindings/isolation, heap/stack/deadline recovery and shared fuel regressions
// POS: [platform / WASM host] Tests execute the engine without native FFI, paid models or ambient path authority
package host

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	kernelabi "github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"github.com/QuincySx/Alva/sdk/sandbox/abi"
	"github.com/tetratelabs/wazero"
	"github.com/tetratelabs/wazero/api"
)

func scriptFixture(t *testing.T) (*job, string) {
	t.Helper()
	directory := t.TempDir()
	root, err := os.OpenRoot(directory)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := root.Close(); err != nil {
			t.Error(err)
		}
	})
	request := tinyFuelRequest(t, wasmHeader)
	request.Limits.Fuel = 0
	request.Context.ScriptAvailable = true
	request.Context.Policy.AllowFileWrite = true
	request.Grants = []Grant{{Host: directory, Guest: "/work", Access: ReadWrite}}
	if err := normalize(&request); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	t.Cleanup(cancel)
	return &job{request: request, ctx: ctx, stop: cancel, files: &fsBudget{}, cache: testRunner.cache, mounts: []mount{{grant: request.Grants[0], root: root}}}, directory
}
func evaluate(t *testing.T, j *job, script string) abi.ScriptResult {
	t.Helper()
	out, err := j.script(j.ctx, abi.ScriptRequest{Version: abi.Version, Script: script})
	if err != nil {
		t.Fatal("actual engine infrastructure failed", err)
	}
	if err = out.Validate(); err != nil {
		t.Fatal("unbounded script result", out, err)
	}
	return out
}
func TestActualQuickJSBindingsAndFreshNoModuleRuntime(t *testing.T) {
	j, directory := scriptFixture(t)
	result := evaluate(t, j, `writeFile('batch/a.txt','hello'); appendFile('batch/a.txt',' 世界'); copyFile('batch/a.txt','batch/b.txt'); rename('batch/b.txt','batch/renamed.txt'); writeJson('batch/data.json',{answer:42}); print({name:'fixture'}, readFile('batch/renamed.txt')); JSON.stringify({names:readDir('batch').map(x=>x.name), paths:glob('**/*.txt'), data:readJson('batch/data.json'), file:stat('batch/a.txt').isFile, exists:exists('batch/a.txt')})`)
	if result.Error != "" || len(result.Output) != 1 || !strings.Contains(result.Output[0], "hello 世界") || !strings.Contains(result.Result, `"answer":42`) || !strings.Contains(result.Result, "batch/renamed.txt") {
		t.Fatal("actual file bindings failed", result)
	}
	data, err := os.ReadFile(filepath.Join(directory, "batch", "renamed.txt"))
	if err != nil || string(data) != "hello 世界" {
		t.Fatal("no real file effect", string(data), err)
	}
	for _, script := range []string{`globalThis.privateMarker=42; 'created'`, `typeof privateMarker`, `[typeof process,typeof require,typeof std,typeof os,typeof Deno,typeof __alva].join(',')`, `import 'std';`} {
		r := evaluate(t, j, script)
		switch script {
		case `typeof privateMarker`:
			if r.Result != "undefined" || r.Error != "" {
				t.Fatal("previous script state leaked", r)
			}
		case `import 'std';`:
			if r.Error == "" {
				t.Fatal("global eval accepted module", r)
			}
		case `[typeof process,typeof require,typeof std,typeof os,typeof Deno,typeof __alva].join(',')`:
			if r.Result != "undefined,undefined,undefined,undefined,undefined,undefined" {
				t.Fatal("ambient API or raw capabilities exposed", r)
			}
		}
	}
	result = evaluate(t, j, `remove('batch'); exists('batch')`)
	if result.Error != "" || result.Result != "false" {
		t.Fatal("bounded remove failed", result)
	}
}
func TestActualQuickJSModernLanguageAndResizableTypedArrays(t *testing.T) {
	j, _ := scriptFixture(t)
	result := evaluate(t, j, `
const buffer = new ArrayBuffer(8, {maxByteLength: 16});
const view = new Uint8Array(buffer);
view.set([1, 2, 3, 4, 5, 6, 7, 8]);
const initialLast = view.at(-1);
buffer.resize(4);
const shrunk = [view.length, view.at(-1), view.at(4) ?? null];
buffer.resize(12);
const grown = [view.length, view.at(-1)];
class Box { #value = 21n; get doubled() { return this.#value * 2n; } }
const mapped = Iterator.from(new Set([1, 2, 3, 4])).filter(x => x % 2 === 0).map(x => x * 10).toArray();
JSON.stringify({initialLast, shrunk, grown, mapped, bigint: String(new Box().doubled), finalization: typeof FinalizationRegistry});
`)
	const want = `{"initialLast":8,"shrunk":[4,4,null],"grown":[12,0],"mapped":[20,40],"bigint":"42","finalization":"function"}`
	if result.Error != "" || result.Result != want {
		t.Fatal("actual QuickJS language support or resized TypedArray.at semantics regressed", result)
	}
}
func TestScriptGrantsRejectTraversalSymlinkWritesAndUndelegatedCapability(t *testing.T) {
	j, directory := scriptFixture(t)
	outside := t.TempDir()
	secret := filepath.Join(outside, "outside-secret")
	if err := os.WriteFile(secret, []byte("never available"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(directory, "escape")); err != nil {
		t.Fatal(err)
	}
	for _, source := range []string{`readFile('../outside-secret')`, `readFile('/outside-secret')`, `readFile('escape/outside-secret')`, `writeFile('escape/new','no')`, `remove('/work')`, `glob('**/*')`} {
		r := evaluate(t, j, source)
		if r.Error == "" || strings.Contains(r.Error, outside) || strings.Contains(r.Error, directory) {
			t.Fatal("escaped root or exposed host path", source, r)
		}
	}
	if _, err := os.Stat(filepath.Join(outside, "new")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("symlink write escaped", err)
	}
	j.request.Context.Policy.AllowFileWrite = false
	for _, source := range []string{`writeFile('no','x')`, `appendFile('no','x')`, `mkdir('no')`, `remove('escape')`, `rename('escape','no')`, `copyFile('in.txt','no')`} {
		if r := evaluate(t, j, source); r.Error == "" {
			t.Fatal("read-only job acquired script writes", source, r)
		}
	}
	if _, err := os.Stat(filepath.Join(directory, "no")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("read-only rejection changed filesystem", err)
	}
	j.request.Context.ScriptAvailable = false
	if r := evaluate(t, j, `'must not run'`); r.Error == "" {
		t.Fatal("unadvertised script capability executed")
	}
}
func TestScriptFetchHasExistingHostAllowlistAndCatchableFailure(t *testing.T) {
	j, _ := scriptFixture(t)
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		if r.Method != "POST" || r.Header.Get("X-Fixture") != "present" {
			t.Error("fetch request changed")
		}
		w.Write([]byte("actual bounded fetch"))
	}))
	defer server.Close()
	encoded, _ := json.Marshal(server.URL)
	result := evaluate(t, j, `try { fetch(`+string(encoded)+`) } catch (e) { print('denied'); 'caught' }`)
	if result.Error != "" || result.Result != "caught" || calls != 0 || len(result.Output) != 1 {
		t.Fatal("denied fetch was not catchable or reached network", result, calls)
	}
	u, _ := url.Parse(server.URL)
	j.request.AllowedDomains = []string{u.Hostname()}
	result = evaluate(t, j, `fetch(`+string(encoded)+`,{method:'POST',headers:[['X-Fixture','present']],body:'hello'}).body`)
	if result.Error != "" || result.Result != "actual bounded fetch" || calls != 1 || j.outcome.FetchCalls != 2 {
		t.Fatal("actual allowlisted fetch failed", result, calls)
	}
}
func TestScriptHeapStackAndOutputLimitsLeaveNextScriptUsable(t *testing.T) {
	j, _ := scriptFixture(t)
	for _, source := range []string{`let buffers=[];while(true){buffers.push(new ArrayBuffer(1<<20))}`, `print('x'.repeat(65537)); 'must not become successful prefix'`, `'x'.repeat(65537)`} {
		r := evaluate(t, j, source)
		if r.Error == "" {
			t.Fatal("script exceeded its limits", r)
		}
		if strings.Contains(r.Error, "deadline") {
			t.Fatal("script deadline must not masquerade as a heap/output limit", r)
		}
		if next := evaluate(t, j, `40+2`); next.Error != "" || next.Result != "42" {
			t.Fatal("failed script poisoned next instance", next)
		}
	}
	r := evaluate(t, j, `function recur(){return 1+recur()} let caught; try { recur() } catch(e) { caught = 'caught:' + String(e) } function usable() { return 40+2 } caught + ':usable=' + usable()`)
	if r.Error != "" || !strings.Contains(r.Result, "caught:") || !strings.Contains(strings.ToLower(r.Result), "stack") {
		t.Fatal("512KiB software stack check did not throw catchable JS error", r)
	}
	if !strings.HasSuffix(r.Result, ":usable=42") {
		t.Fatal("stack overflow poisoned the same runtime after it was caught", r)
	}
}
func TestScriptActualDeadlineRetainsPartialEvidenceAndDoesNotKillAgentOwner(t *testing.T) {
	j, directory := scriptFixture(t)
	// Measure the execution deadline separately from job-owned cold native
	// compilation; this also keeps this regression valid when run alone.
	if ready := evaluate(t, j, `'ready'`); ready.Error != "" || ready.Result != "ready" {
		t.Fatal("script engine did not become ready", ready)
	}
	started := time.Now()
	r := evaluate(t, j, `writeFile('before-timeout.txt','partial effect'); print('before spin'); while(true){}`)
	if r.Error == "" || !strings.Contains(r.Error, "deadline") || time.Since(started) > 15*time.Second || len(r.Output) != 1 {
		t.Fatal("actual script deadline not observed", r, time.Since(started))
	}
	if data, err := os.ReadFile(filepath.Join(directory, "before-timeout.txt")); err != nil || string(data) != "partial effect" {
		t.Fatal("script was not entered before deadline", err)
	}
	if j.ctx.Err() != nil {
		t.Fatal("ordinary script deadline killed parent job")
	}
	if next := evaluate(t, j, `'still alive'`); next.Error != "" || next.Result != "still alive" {
		t.Fatal("script timeout killed next instance", next)
	}
}
func TestPinnedQuickJSAssetMatchesSourceAndBuildProvenance(t *testing.T) {
	data, err := os.ReadFile("scriptwasm/provenance.json")
	if err != nil {
		t.Fatal(err)
	}
	var p struct {
		Engine          string `json:"engine"`
		SourceSHA256    string `json:"source_sha256"`
		BinaryBytes     int    `json:"binary_bytes"`
		BinarySHA256    string `json:"binary_sha256"`
		BridgeSHA256    string `json:"bridge_sha256"`
		BootstrapSHA256 string `json:"bootstrap_sha256"`
	}
	if err = json.Unmarshal(data, &p); err != nil {
		t.Fatal(err)
	}
	if p.Engine != "QuickJS-NG v0.17.0 core only" || p.SourceSHA256 != "559bc4c420475e55c7ab4510adbc562f55d7524d75e8e89d79ce4bb02f5687d9" || p.BinaryBytes != len(quickJSModule) || len(quickJSModule) > 1<<20 {
		t.Fatal("QuickJS source must remain pinned and the core must stay within 1MiB", p)
	}
	cases := []struct {
		name, want string
		data       []byte
	}{{"quickjs.wasm", p.BinarySHA256, quickJSModule}, {"bridge.c", p.BridgeSHA256, nil}, {"bootstrap.js", p.BootstrapSHA256, nil}}
	for _, tc := range cases {
		body := tc.data
		if body == nil {
			body, err = os.ReadFile("scriptwasm/" + tc.name)
			if err != nil {
				t.Fatal(err)
			}
		}
		digest := sha256.Sum256(body)
		if hex.EncodeToString(digest[:]) != tc.want {
			t.Fatal("QuickJS source/asset changed without explicit rebuild/provenance", tc.name)
		}
	}
}

func TestScriptReservationAndObserverFailureCannotEscapeActualJobOwner(t *testing.T) {
	j, dir := scriptFixture(t)
	j.request.Limits.MemoryBytes = 32 << 20
	result := evaluate(t, j, `writeFile('forbidden.txt','not enough job memory')`)
	if result.Error == "" || !strings.Contains(result.Error, "linear-memory budget") {
		t.Fatal("script acquired memory outside job ceiling", result)
	}
	if _, err := os.Stat(filepath.Join(dir, "forbidden.txt")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("reservation failure still executed", err)
	}
	j.request.Limits.MemoryBytes = 256 << 20
	private := errors.New("private script journal failure")
	j.caps.Observer = ObserverFunc(func(_ context.Context, event IOEvent) error {
		if event.Kind == "script_binding_request" {
			return private
		}
		return nil
	})
	_, err := j.script(j.ctx, abi.ScriptRequest{Version: abi.Version, Script: `writeFile('forbidden.txt','must not execute without durable intent')`})
	if !errors.Is(err, private) || j.ctx.Err() == nil || strings.Contains(err.Error(), private.Error()) {
		t.Fatal("journal failure swallowed, leaked or did not stop actual owner", err)
	}
	if _, err := os.Stat(filepath.Join(dir, "forbidden.txt")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("observer failure executed write", err)
	}
}

func TestActualSDKScriptErrorRecoversWithSharedFuel(t *testing.T) {
	request := requestFor(t, false, "")
	// The fresh Runner cold-compiles worker and QuickJS, each with and without
	// metering; keep this integration budget separate from script execution.
	request.Limits.WallClock = 3 * time.Minute
	request.Context.ScriptAvailable = true
	request.Context.Policy.AllowFileWrite = true
	request.Limits.Fuel = 1_000_000_000
	turn := 0
	model := modelFunc(func(ctx context.Context, q kernelabi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if turn == 1 {
			return emitCall(emit, "script-failure", "run_script", `{"script":"writeFile('script-effect.txt','real'); print('captured'); throw new Error('expected recoverable failure')"}`)
		}
		if turn == 2 {
			found := false
			for _, m := range q.Messages {
				for _, b := range m.Content {
					if b.ToolResult != nil && b.ToolResult.CallID == "script-failure" {
						found = true
						if !b.ToolResult.IsError || len(b.ToolResult.Content) != 1 || b.ToolResult.Content[0].Kind != wire.ContentReference {
							t.Error("script error not a recoverable reference", b.ToolResult)
						}
					}
				}
			}
			if !found {
				t.Error("failed script disappeared")
			}
			return emitCall(emit, "script-success", "run_script", `{"script":"readFile('script-effect.txt') + ':' + (typeof privateMarker)"}`)
		}
		return emitText(emit, "agent continued after script error")
	})
	outcome, err := Run(t.Context(), request, Capabilities{Model: model})
	if err != nil || outcome.ExitCode != 0 || turn != 3 || outcome.ScriptCalls != 2 || outcome.FuelRemaining == 0 || outcome.FuelRemaining >= request.Limits.Fuel || string(outcome.Stdout) != "agent continued after script error" {
		t.Fatal("actual SDK/QuickJS/fuel integration failed", outcome, turn, err)
	}
}

func TestScriptFuelExhaustionAfterLoopEntryCancelsOwner(t *testing.T) {
	j, _ := scriptFixture(t)
	// Keep a real owner's WASM globals and measure the actual engine prefix.
	// This avoids racing a fixed SDK-startup allowance against the 10s script
	// deadline, while still executing and metering the real JS loop.
	owner, _ := compiledFuel(t, wazero.NewRuntimeConfig(), wasmHeader, MaxFuel, nil)
	j.fuel = owner.ExportedGlobal(fuelRemainingExport).(api.MutableGlobal)
	j.fuelExhausted = owner.ExportedGlobal(fuelExhaustedExport).(api.MutableGlobal)
	baseline := evaluate(t, j, `print('fuel-loop-entered'); 'baseline'`)
	if baseline.Error != "" {
		t.Fatal("metered engine prefix failed", baseline)
	}
	prefix := MaxFuel - j.fuel.Get()
	if prefix == 0 || prefix > MaxFuel-100_000 {
		t.Fatal("invalid measured prefix fuel", prefix)
	}
	budget := prefix + 100_000
	j.fuel.Set(budget)
	entered := false
	j.caps.Observer = ObserverFunc(func(_ context.Context, event IOEvent) error {
		if event.Kind == "script_binding_request" && string(event.Data) == `["fuel-loop-entered"]` {
			entered = true
		}
		return nil
	})
	_, err := j.script(j.ctx, abi.ScriptRequest{Version: abi.Version, Script: `print('fuel-loop-entered'); while(true){}`})
	if !entered || !errors.Is(err, ErrFuelExhausted) || errors.Is(err, context.DeadlineExceeded) || j.ctx.Err() == nil || j.fuelExhausted.Get() != 1 || j.fuel.Get() >= budget {
		t.Fatal("script did not exhaust the actual owner's fuel after entering JS", entered, prefix, budget, j.fuel.Get(), err)
	}
}
