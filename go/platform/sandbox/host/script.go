// INPUT: Explicit advertised script capability, original job root handles and pinned embedded QuickJS reactor
// OUTPUT: Fresh isolated script execution with bounded result/output, cancellation and recoverable engine errors
// POS: [platform / WASM host] A script instance has no mounted filesystem, environment, provider or native FFI; authority stays in the enclosing job
package host

import (
	"context"
	"crypto/rand"
	_ "embed"
	"encoding/json"
	"errors"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"github.com/QuincySx/Alva/go/sdk/sandbox/abi"
	"github.com/tetratelabs/wazero"
	"github.com/tetratelabs/wazero/api"
	"github.com/tetratelabs/wazero/imports/wasi_snapshot_preview1"
)

//go:embed scriptwasm/quickjs.wasm
var quickJSModule []byte

type scriptState struct {
	job                *job
	ctx                context.Context
	pending            []byte
	output             []string
	outputBytes, calls int
	outputFailure      bool
	failure            error
}

func (s *scriptState) call(ctx context.Context, module api.Module, op, ptr, size uint32) uint32 {
	s.calls++
	if s.pending != nil || size > 8<<20 {
		panic("invalid script binding exchange")
	}
	input, ok := module.Memory().Read(ptr, size)
	if !ok {
		panic("invalid script binding input")
	}
	data := append([]byte(nil), input...)
	var value any
	var err error
	if s.calls > 1024 {
		err = errors.New("script binding call limit exceeded")
	} else if !wire.ValidJSON(data) {
		err = errors.New("strict UTF-8 JSON binding arguments required")
	} else {
		if err = s.job.observe("script_binding_request", abi.OpScript, data); err != nil {
			s.failure = errors.Join(s.failure, err)
			panic("script binding journal failed")
		}
		value, err = s.binding(ctx, op, data)
	}
	response := map[string]any{"value": value}
	if err != nil {
		response = map[string]any{"error": err.Error()}
	}
	encoded, e := json.Marshal(response)
	if e != nil || len(encoded) > 8<<20 {
		encoded = []byte(`{"error":"script binding result exceeds 8MiB or is invalid"}`)
	}
	if err = s.job.observe("script_binding_response", abi.OpScript, encoded); err != nil {
		s.failure = errors.Join(s.failure, err)
		panic("script binding journal failed")
	}
	s.pending = encoded
	return uint32(len(encoded))
}
func (s *scriptState) read(_ context.Context, module api.Module, ptr, size uint32) uint32 {
	if s.pending == nil || size != uint32(len(s.pending)) || !module.Memory().Write(ptr, s.pending) {
		panic("invalid script binding response buffer")
	}
	s.pending = nil
	return size
}

func (j *job) script(ctx context.Context, request abi.ScriptRequest) (result abi.ScriptResult, runErr error) {
	result.Version = abi.Version
	if !j.request.Context.ScriptAvailable {
		result.Error = "isolated script capability was not delegated by the host"
		return result, nil
	}
	j.outcome.ScriptCalls++
	if j.outcome.ScriptCalls > 64 {
		result.Error = "job script call limit exceeded (64)"
		return result, nil
	}
	used := uint64(0)
	if j.guestMemory != nil {
		used = uint64(j.guestMemory.Size())
	}
	if used > j.request.Limits.MemoryBytes || j.request.Limits.MemoryBytes-used < 64<<20 {
		result.Error = "isolated script needs 64MiB unused within the job's total linear-memory budget"
		return result, nil
	}
	state := &scriptState{job: j, ctx: ctx}
	defer func() {
		result.Output = append([]string{}, state.output...)
		runErr = errors.Join(runErr, state.failure)
		if ctx.Err() != nil {
			runErr = errors.Join(runErr, ctx.Err())
		}
	}()
	// Only code is cached by the enclosing Runner. Every script owns a fresh
	// runtime/module/linear memory/QuickJS heap and no filesystem or ambient env.
	runtime := wazero.NewRuntimeWithConfig(ctx, wazero.NewRuntimeConfig().WithCompilationCache(j.cache).WithMemoryLimitPages(1024).WithCloseOnContextDone(true))
	defer func() { runErr = errors.Join(runErr, runtime.Close(context.Background())) }()
	if _, err := wasi_snapshot_preview1.Instantiate(ctx, runtime); err != nil {
		return result, err
	}
	if _, err := runtime.NewHostModuleBuilder("alva_script").NewFunctionBuilder().WithFunc(state.call).Export("call").NewFunctionBuilder().WithFunc(state.read).Export("read").NewFunctionBuilder().WithFunc(func() uint32 {
		if state.ctx.Err() != nil {
			return 1
		}
		return 0
	}).Export("interrupted").Instantiate(ctx); err != nil {
		return result, err
	}
	// Cold native compilation is charged to the enclosing job deadline. Start
	// the script's execution deadline only after both validated code variants
	// are ready; race-enabled cold compilation can itself exceed ten seconds.
	compiled, err := runtime.CompileModule(ctx, quickJSModule)
	if err == nil && j.fuel != nil {
		// Cache one fixed metered engine, then seed its private counter from
		// the suspended SDK worker. Inner instructions consume that SAME
		// remaining job budget; invoking run_script cannot evade --wasm-fuel.
		var encoded []byte
		encoded, err = instrumentFuel(ctx, quickJSModule, MaxFuel)
		if err == nil {
			compiled, err = runtime.CompileModule(ctx, encoded)
		}
	}
	if err != nil {
		if ctx.Err() != nil {
			return result, ctx.Err()
		}
		result.Error = "isolated script compilation did not complete"
		return result, nil
	}
	if err := ctx.Err(); err != nil {
		return result, err
	}
	owned, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	state.ctx = owned
	instance, err := runtime.InstantiateModule(owned, compiled, wazero.NewModuleConfig().WithName("isolated-script").WithRandSource(rand.Reader).WithSysWalltime().WithSysNanotime().WithStartFunctions())
	if err == nil && j.fuel != nil {
		remaining := instance.ExportedGlobal(fuelRemainingExport).(api.MutableGlobal)
		remaining.Set(j.fuel.Get())
		defer func() {
			j.fuel.Set(remaining.Get())
			if instance.ExportedGlobal(fuelExhaustedExport).Get() != 0 {
				j.fuelExhausted.Set(1)
				runErr = errors.Join(runErr, ErrFuelExhausted)
				j.abort(ErrFuelExhausted)
			}
		}()
	}
	if err == nil {
		for _, name := range []string{fuelInitializeExport, "_initialize"} {
			if initialize := instance.ExportedFunction(name); initialize != nil {
				_, err = initialize.Call(owned)
				if err != nil {
					break
				}
			}
		}
	}
	if err == nil {
		allocation, e := instance.ExportedFunction("alva_alloc").Call(owned, uint64(len(request.Script)))
		err = e
		if err == nil && (len(allocation) != 1 || allocation[0] == 0 || !instance.Memory().Write(uint32(allocation[0]), []byte(request.Script))) {
			err = errors.New("script source allocation failed")
		}
		if err == nil {
			var returned, length []uint64
			returned, err = instance.ExportedFunction("alva_eval").Call(owned, allocation[0], uint64(len(request.Script)))
			if err == nil {
				length, err = instance.ExportedFunction("alva_result_length").Call(owned)
			}
			if err == nil && len(returned) == 1 && len(length) == 1 && length[0] <= 64<<10 {
				data, ok := instance.Memory().Read(uint32(returned[0]), uint32(length[0]))
				var output struct {
					Result string `json:"result"`
					Error  string `json:"error"`
				}
				if !ok || !utf8.Valid(data) || !wire.ValidJSON(data) || json.Unmarshal(data, &output) != nil {
					err = errors.New("invalid script result")
				} else {
					result.Result, result.Error = output.Result, output.Error
				}
			} else if err == nil {
				err = errors.New("invalid script result bounds")
			}
		}
	}
	if err != nil {
		result.Result = ""
		result.Error = "isolated script evaluation failed; prior file changes may be partial"
		if owned.Err() != nil && ctx.Err() == nil {
			result.Error = "script exceeded its 10-second deadline; prior file changes may be partial"
		}
	}
	if state.outputFailure {
		result.Result = ""
		result.Error = "script output exceeded 256 lines/64KiB; retained output is a bounded prefix"
	}
	result.Output = state.output
	if err := result.Validate(); err != nil {
		result.Result = ""
		result.Error = "script result failed bounded UTF-8 validation"
	}
	return result, state.failure
}
