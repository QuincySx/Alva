// INPUT: Original Core V2 binaries, deliberately invalid index attacks and the real SDK WASIp1 guest
// OUTPUT: Compiler/interpreter differential semantics, deterministic depletion, fresh counters and real guest fuel regressions
// POS: [platform / WASM host] Tests prove actual guest-instruction traps rather than deadline or host callback counts
package host

import (
	"context"
	"errors"
	"testing"
	"time"

	kernelabi "github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"github.com/QuincySx/Alva/go/sdk/sandbox/abi"
	"github.com/tetratelabs/wazero"
	"github.com/tetratelabs/wazero/api"
)

var wasmHeader = []byte{0, 97, 115, 109, 1, 0, 0, 0}

func fuelFixture(body []byte, result, parameter bool, sections map[byte][]byte) []byte {
	typeBody := []byte{1, 0x60, 0}
	if parameter {
		typeBody = []byte{1, 0x60, 1, 0x7f}
	}
	if result {
		typeBody = append(typeBody, 1, 0x7f)
	} else {
		typeBody = append(typeBody, 0)
	}
	parts := map[byte][]byte{1: typeBody, 3: {1, 0}, 10: append(append([]byte{1}, uleb(nil, uint32(len(body)+1))...), append([]byte{0}, body...)...)}
	name := "_start"
	if result {
		name = "value"
	}
	exports := []byte{1}
	exports = uleb(exports, uint32(len(name)))
	exports = append(exports, name...)
	exports = append(exports, 0, 0)
	parts[7] = exports
	for id, payload := range sections {
		parts[id] = payload
	}
	out := append([]byte(nil), wasmHeader...)
	// DataCount precedes Code under Core V2 despite its numeric ID.
	for _, id := range []byte{1, 2, 3, 4, 5, 6, 7, 8, 9, 12, 10, 11} {
		if p, ok := parts[id]; ok {
			out = appendSection(out, id, p)
		}
	}
	return out
}

func compiledFuel(t *testing.T, config wazero.RuntimeConfig, raw []byte, fuel uint64, env []byte) (api.Module, wazero.Runtime) {
	t.Helper()
	ctx := t.Context()
	runtime := wazero.NewRuntimeWithConfig(ctx, config)
	t.Cleanup(func() {
		if err := runtime.Close(context.Background()); err != nil {
			t.Error(err)
		}
	})
	if env != nil {
		if _, err := runtime.InstantiateWithConfig(ctx, env, wazero.NewModuleConfig().WithName("env")); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := runtime.CompileModule(ctx, raw); err != nil {
		t.Fatal("original fixture invalid", err)
	}
	encoded, err := instrumentFuel(ctx, raw, fuel)
	if err != nil {
		t.Fatal(err)
	}
	compiled, err := runtime.CompileModule(ctx, encoded)
	if err != nil {
		t.Fatal("instrumented fixture invalid", err)
	}
	instance, err := runtime.InstantiateModule(ctx, compiled, wazero.NewModuleConfig().WithStartFunctions())
	if err != nil {
		t.Fatal(err)
	}
	return instance, runtime
}

func TestFuelDifferentialControlImmediateAndCoreV2Semantics(t *testing.T) {
	cases := []struct {
		name       string
		body       []byte
		parameter  bool
		args       []uint64
		want, cost uint64
		extra      map[byte][]byte
	}{
		{"arithmetic", []byte{0x41, 20, 0x41, 22, 0x6a, 0x0b}, false, nil, 42, 3, nil},
		{"if-then", []byte{0x20, 0, 0x04, 0x7f, 0x41, 11, 0x05, 0x41, 22, 0x0b, 0x0b}, true, []uint64{1}, 11, 3, nil},
		{"if-else", []byte{0x20, 0, 0x04, 0x7f, 0x41, 11, 0x05, 0x41, 22, 0x0b, 0x0b}, true, []uint64{0}, 22, 3, nil},
		{"branch-table", []byte{0x02, 0x7f, 0x41, 42, 0x20, 0, 0x0e, 1, 0, 0, 0x0b, 0x0b}, true, []uint64{7}, 42, 3, nil},
		{"simd-immediates", append(append([]byte{0xfd, 12}, []byte{0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 0xfc, 0xfd, 0xff}...), 0x1a, 0x41, 42, 0x0b), false, nil, 42, 2, nil},
		{"signed-leb", []byte{0x42, 0xfc, 0, 0x1a, 0x41, 42, 0x0b}, false, nil, 42, 2, nil},
		{"float-immediate", []byte{0x44, 0x0b, 0x0c, 0xfd, 0xfc, 0, 0, 0, 0, 0x1a, 0x41, 42, 0x0b}, false, nil, 42, 2, nil},
		{"bulk-memory", []byte{0x41, 0, 0x41, 0, 0x41, 16, 0xfc, 10, 0, 0, 0x41, 42, 0x0b}, false, nil, 42, 5, map[byte][]byte{5: {1, 0, 1}}},
		{"defined-global", []byte{0x23, 0, 0x0b}, false, nil, 11, 1, map[byte][]byte{6: {1, 0x7f, 1, 0x41, 11, 0x0b}}},
	}
	for _, backend := range []struct {
		name   string
		config wazero.RuntimeConfig
	}{{"compiler", wazero.NewRuntimeConfigCompiler()}, {"interpreter", wazero.NewRuntimeConfigInterpreter()}} {
		for _, tc := range cases {
			t.Run(backend.name+"/"+tc.name, func(t *testing.T) {
				raw := fuelFixture(tc.body, true, tc.parameter, tc.extra)
				instance, runtime := compiledFuel(t, backend.config, raw, tc.cost, nil)
				original, err := runtime.InstantiateWithConfig(t.Context(), raw, wazero.NewModuleConfig().WithName("original").WithStartFunctions())
				if err != nil {
					t.Fatal(err)
				}
				before, err := original.ExportedFunction("value").Call(t.Context(), tc.args...)
				if err != nil || len(before) != 1 || before[0] != tc.want {
					t.Fatal("bad original", before, err)
				}
				after, err := instance.ExportedFunction("value").Call(t.Context(), tc.args...)
				if err != nil || len(after) != 1 || after[0] != before[0] {
					t.Fatal("meter changed semantics", after, err)
				}
				if instance.ExportedGlobal(fuelRemainingExport).Get() != 0 || instance.ExportedGlobal(fuelExhaustedExport).Get() != 0 {
					t.Fatal("incorrect exact budget")
				}
				if _, err = instance.ExportedFunction("value").Call(t.Context(), tc.args...); err == nil || instance.ExportedGlobal(fuelExhaustedExport).Get() != 1 {
					t.Fatal("spent instance ran again", err)
				}
			})
		}
	}
}

func TestFuelImportedGlobalsDoNotShiftOriginalAuthority(t *testing.T) {
	env := appendSection(append([]byte(nil), wasmHeader...), 6, []byte{1, 0x7f, 1, 0x41, 9, 0x0b})
	env = appendSection(env, 7, []byte{1, 1, 'g', 3, 0})
	raw := fuelFixture([]byte{0x23, 0, 0x0b}, true, false, map[byte][]byte{2: {1, 3, 'e', 'n', 'v', 1, 'g', 3, 0x7f, 1}})
	instance, _ := compiledFuel(t, wazero.NewRuntimeConfig(), raw, 1, env)
	got, err := instance.ExportedFunction("value").Call(t.Context())
	if err != nil || len(got) != 1 || got[0] != 9 || instance.ExportedGlobal(fuelRemainingExport).Get() != 0 {
		t.Fatal("import/global indices changed", got, err)
	}
}

func tinyFuelRequest(t *testing.T, module []byte) Request {
	t.Helper()
	return Request{JobID: "fuel-test", Module: module, Grants: []Grant{{Host: t.TempDir(), Guest: "/work", Access: ReadOnly}}, Context: abi.Context{Version: abi.Version, SessionID: "fuel-session", Workspace: "/work", Task: "bounded CPU fixture", MaxIterations: 1}, Limits: Limits{Fuel: 17, WallClock: 5 * time.Second}}
}
func TestFuelActuallyStopsLoopAndBinaryStartAndResetsPerJob(t *testing.T) {
	for _, start := range []bool{false, true} {
		t.Run(map[bool]string{false: "_start", true: "binary-start"}[start], func(t *testing.T) {
			parts := map[byte][]byte{}
			if start {
				parts[8] = []byte{0}
			}
			request := tinyFuelRequest(t, fuelFixture([]byte{0x03, 0x40, 0x0c, 0, 0x0b, 0x0b}, false, false, parts))
			runner := NewRunner()
			defer runner.Close(context.Background())
			for i := 0; i < 2; i++ {
				outcome, err := runner.Run(t.Context(), request, Capabilities{})
				if !errors.Is(err, ErrFuelExhausted) || errors.Is(err, context.DeadlineExceeded) || !outcome.FuelExhausted || outcome.FuelLimit != 17 || outcome.FuelRemaining != 0 {
					t.Fatal("instruction loop escaped fuel or reused old state", outcome, err)
				}
			}
		})
	}
}

func TestFuelNeverLegalizesOriginalOutOfRangeGlobals(t *testing.T) {
	// Appending private globals WOULD make global.get 0 structurally legal.
	// The mandatory first compile must reject it before rewriting/execution.
	request := tinyFuelRequest(t, fuelFixture([]byte{0x23, 0, 0x1a, 0x0b}, false, false, nil))
	if _, err := Run(t.Context(), request, Capabilities{}); err == nil {
		t.Fatal("meter legalized hostile access to its private counter")
	}
	request = tinyFuelRequest(t, fuelFixture([]byte{0x0b}, false, false, map[byte][]byte{7: append(append([]byte{1}, uleb(nil, uint32(len(fuelRemainingExport)))...), append([]byte(fuelRemainingExport), 3, 0)...)}))
	if _, err := Run(t.Context(), request, Capabilities{}); err == nil {
		t.Fatal("original invalid export legalized")
	}
}

func TestActualSDKGuestRunsWithMeasuredInstructionFuel(t *testing.T) {
	request := requestFor(t, false, "")
	// This fresh Runner compiles both full SDK variants under race; startup
	// belongs to the job budget, independently of the asserted instruction fuel.
	request.Limits.WallClock = 3 * time.Minute
	request.Limits.Fuel = 1_000_000_000
	outcome, err := Run(t.Context(), request, Capabilities{Model: modelFunc(func(ctx context.Context, _ kernelabi.ModelRequest, emit func(wire.StreamChunk) error) error {
		return emitText(emit, "fuel-metered SDK")
	})})
	if err != nil || outcome.ExitCode != 0 || string(outcome.Stdout) != "fuel-metered SDK" || outcome.FuelLimit != request.Limits.Fuel || outcome.FuelRemaining >= request.Limits.Fuel || outcome.FuelRemaining == 0 || outcome.FuelExhausted {
		t.Fatal("real metered SDK guest failed", outcome, err)
	}
}
