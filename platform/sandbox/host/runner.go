// INPUT: context, crypto/rand, errors, fmt, os, path/filepath, sort, strings, sync, time, wazero, kernel/abi, llm/wire, sandbox/abi
// OUTPUT: NewRunner, Runner, Run, CallbackError
// POS: [platform / WASM host or guest] Fresh per-job WASIp1 execution with actual cancellation, bounded memory and explicit host capability bridges.
package host

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/binary"
	"errors"
	"fmt"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/tetratelabs/wazero"
	"github.com/tetratelabs/wazero/api"
	"github.com/tetratelabs/wazero/experimental/sysfs"
	"github.com/tetratelabs/wazero/imports/wasi_snapshot_preview1"
	"github.com/tetratelabs/wazero/sys"

	kernelabi "github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"github.com/QuincySx/Alva/sdk/sandbox/abi"
)

const MaxModuleBytes = 64 << 20

// Runner shares only compiled code. Runtime, module instances, handles and all
// authority remain per job. The explicit cache owner accepts <=16 distinct
// modules; Close rejects new jobs and waits real active jobs before releasing it.
type Runner struct {
	mu        sync.Mutex
	cache     wazero.CompilationCache
	modules   map[[32]byte]bool
	active    int
	idle      chan struct{}
	closing   bool
	closeOnce sync.Once
	closeErr  error
}

func NewRunner() *Runner {
	idle := make(chan struct{})
	close(idle)
	return &Runner{cache: wazero.NewCompilationCache(), modules: make(map[[32]byte]bool), idle: idle}
}
func (runner *Runner) Run(ctx context.Context, request Request, caps Capabilities) (Outcome, error) {
	if len(request.Module) > MaxModuleBytes {
		return Outcome{}, errors.New("sandbox module input byte limit exceeded")
	}
	hash := sha256.New()
	hash.Write(request.Module)
	var fuelKey [8]byte
	binary.LittleEndian.PutUint64(fuelKey[:], request.Limits.Fuel)
	hash.Write(fuelKey[:])
	var digest [32]byte
	copy(digest[:], hash.Sum(nil))
	runner.mu.Lock()
	if runner.closing {
		runner.mu.Unlock()
		return Outcome{}, errors.New("sandbox runner closed")
	}
	if !runner.modules[digest] && len(runner.modules) >= 16 {
		runner.mu.Unlock()
		return Outcome{}, errors.New("runner distinct module cache budget exceeded")
	}
	runner.modules[digest] = true
	if runner.active == 0 {
		runner.idle = make(chan struct{})
	}
	runner.active++
	runner.mu.Unlock()
	defer func() {
		runner.mu.Lock()
		runner.active--
		if runner.active == 0 {
			close(runner.idle)
		}
		runner.mu.Unlock()
	}()
	return run(ctx, request, caps, runner.cache)
}
func (runner *Runner) Close(ctx context.Context) error {
	runner.mu.Lock()
	runner.closing = true
	idle := runner.idle
	runner.mu.Unlock()
	select {
	case <-idle:
	case <-ctx.Done():
		return ctx.Err()
	}
	runner.closeOnce.Do(func() { runner.closeErr = runner.cache.Close(ctx) })
	return runner.closeErr
}

// CallbackError hides private provider/journal/native details from public text,
// retaining the actual cause for trusted host code through Unwrap.
type CallbackError struct {
	Operation string
	Err       error
}

func (err *CallbackError) Error() string { return "sandbox host " + err.Operation + " failed" }
func (err *CallbackError) Unwrap() error { return err.Err }

type mount struct {
	grant Grant
	root  *os.Root
}
type job struct {
	request             Request
	caps                Capabilities
	ctx                 context.Context
	stop                context.CancelFunc
	mu                  sync.Mutex
	failure             error
	sequence            uint64
	pending             []byte
	mounts              []mount
	outcome             Outcome
	cache               wazero.CompilationCache
	files               *fsBudget
	fuel, fuelExhausted api.MutableGlobal
	guestMemory         api.Memory
}
type output struct {
	job   *job
	kind  string
	limit int
	data  []byte
}

func (out *output) Write(data []byte) (int, error) {
	if len(data) > out.limit-len(out.data) {
		prefix := data[:min(len(data), out.limit-len(out.data))]
		observed := out.job.observe(out.kind+"_prefix_limit", 0, prefix)
		err := errors.New("sandbox output byte budget exceeded")
		out.job.abort(err)
		return 0, errors.Join(err, observed)
	}
	if err := out.job.observe(out.kind, 0, data); err != nil {
		return 0, err
	}
	out.data = append(out.data, data...)
	return len(data), nil
}

// Run returns only after guest execution AND all synchronous callbacks actually
// leave their stacks. Host callbacks must honor ctx; no goroutine fabricates a
// completed cancellation if a trusted callback refuses to exit.
func Run(ctx context.Context, request Request, caps Capabilities) (outcome Outcome, runErr error) {
	runner := NewRunner()
	defer func() { runErr = errors.Join(runErr, runner.Close(context.Background())) }()
	return runner.Run(ctx, request, caps)
}
func run(ctx context.Context, request Request, caps Capabilities, cache wazero.CompilationCache) (outcome Outcome, runErr error) {
	if err := ctx.Err(); err != nil {
		return outcome, err
	}
	if err := normalize(&request); err != nil {
		return outcome, err
	}
	if _, err := nativeOpenFlags(os.O_RDONLY, false); err != nil {
		return outcome, err
	}
	request.Module = append([]byte(nil), request.Module...)
	request.Args = append([]string(nil), request.Args...)
	request.Grants = append([]Grant(nil), request.Grants...)
	request.AllowedDomains = append([]string(nil), request.AllowedDomains...)
	request.Context.Attachments = append([]wire.Reference(nil), request.Context.Attachments...)
	jobCtx, cancel := context.WithTimeout(ctx, request.Limits.WallClock)
	defer cancel()
	j := &job{request: request, caps: caps, ctx: jobCtx, stop: cancel, cache: cache}
	started := time.Now()
	defer func() {
		outcome = j.outcome
		outcome.Elapsed = time.Since(started)
		for _, m := range j.mounts {
			runErr = errors.Join(runErr, m.root.Close())
		}
		j.mu.Lock()
		runErr = errors.Join(runErr, j.failure)
		j.mu.Unlock()
	}()
	fsConfig := wazero.NewFSConfig()
	budget := &fsBudget{}
	j.files = budget
	for _, grant := range request.Grants {
		before, err := os.Lstat(grant.Host)
		if err != nil || !before.IsDir() || before.Mode()&os.ModeSymlink != 0 {
			return outcome, errors.New("sandbox grant must be an existing real directory")
		}
		root, err := os.OpenRoot(grant.Host)
		if err != nil {
			return outcome, err
		}
		opened, err := root.Open(".")
		if err != nil {
			root.Close()
			return outcome, err
		}
		actual, err := opened.Stat()
		closeErr := opened.Close()
		if err != nil || closeErr != nil || !os.SameFile(before, actual) {
			root.Close()
			return outcome, errors.New("sandbox grant changed during open")
		}
		j.mounts = append(j.mounts, mount{grant: grant, root: root})
		filesystem := &rootedFS{root: root, readOnly: grant.Access == ReadOnly || !request.Context.Policy.AllowFileWrite, budget: budget}
		fsConfig = fsConfig.(sysfs.FSConfig).WithSysFSMount(filesystem, grant.Guest)
	}
	runtime := wazero.NewRuntimeWithConfig(jobCtx, wazero.NewRuntimeConfig().WithCompilationCache(cache).WithMemoryLimitPages(uint32(request.Limits.MemoryBytes/65536)).WithCloseOnContextDone(true))
	defer func() { runErr = errors.Join(runErr, runtime.Close(context.Background())) }()
	if _, err := wasi_snapshot_preview1.Instantiate(jobCtx, runtime); err != nil {
		return outcome, err
	}
	if _, err := runtime.NewHostModuleBuilder("alva_host").NewFunctionBuilder().WithFunc(j.exchange).Export("exchange").NewFunctionBuilder().WithFunc(j.readResponse).Export("read_response").Instantiate(jobCtx); err != nil {
		return outcome, err
	}
	stdout := &output{job: j, kind: "stdout", limit: request.Limits.OutputBytes}
	stderr := &output{job: j, kind: "stderr", limit: request.Limits.OutputBytes}
	defer func() {
		j.outcome.Stdout = append([]byte(nil), stdout.data...)
		j.outcome.Stderr = append([]byte(nil), stderr.data...)
	}()
	compiled, err := runtime.CompileModule(jobCtx, request.Module)
	if err != nil {
		return outcome, err
	}
	if request.Limits.Fuel != 0 {
		// First validation is a security boundary: instrumentation appends
		// private globals; it must never legalize hostile original indices.
		metered, err := instrumentFuel(jobCtx, request.Module, request.Limits.Fuel)
		if err != nil {
			return outcome, err
		}
		compiled, err = runtime.CompileModule(jobCtx, metered)
		if err != nil {
			return outcome, err
		}
		j.outcome.FuelLimit = request.Limits.Fuel
	}
	config := wazero.NewModuleConfig().WithName(request.JobID).WithArgs(request.Args...).WithFSConfig(fsConfig).WithStdout(stdout).WithStderr(stderr).WithRandSource(rand.Reader).WithSysWalltime().WithSysNanotime().WithSysNanosleep().WithEnv("PWD", request.Context.Workspace)
	if request.Limits.Fuel != 0 {
		config = config.WithStartFunctions()
	}
	instance, err := runtime.InstantiateModule(jobCtx, compiled, config)
	if request.Limits.Fuel != 0 && instance != nil {
		j.fuel = instance.ExportedGlobal(fuelRemainingExport).(api.MutableGlobal)
		j.fuelExhausted = instance.ExportedGlobal(fuelExhaustedExport).(api.MutableGlobal)
		if err == nil {
			if initialize := instance.ExportedFunction(fuelInitializeExport); initialize != nil {
				_, err = initialize.Call(jobCtx)
			}
			if start := instance.ExportedFunction("_start"); err == nil && start != nil {
				_, err = start.Call(jobCtx)
			}
		}
		j.outcome.FuelRemaining = instance.ExportedGlobal(fuelRemainingExport).Get()
		j.outcome.FuelExhausted = instance.ExportedGlobal(fuelExhaustedExport).Get() != 0
		if j.outcome.FuelExhausted {
			err = errors.Join(ErrFuelExhausted, err)
		}
	}
	if err != nil {
		var exit *sys.ExitError
		if errors.As(err, &exit) && jobCtx.Err() != nil && (exit.ExitCode() == sys.ExitCodeContextCanceled || exit.ExitCode() == sys.ExitCodeDeadlineExceeded) {
			// These are the runtime's typed context-termination exits, not an
			// independent guest trap. Real callback/observer failures remain
			// joined from j.failure by the outer defer.
			return outcome, jobCtx.Err()
		}
		if errors.As(err, &exit) && jobCtx.Err() == nil {
			j.outcome.ExitCode = exit.ExitCode()
			return outcome, nil
		}
		return outcome, errors.Join(err, jobCtx.Err())
	}
	return outcome, jobCtx.Err()
}
func normalize(request *Request) error {
	if !abi.Identity(request.JobID) || len(request.Module) < 8 || len(request.Module) > MaxModuleBytes || len(request.Args) > 64 || len(request.Grants) > 16 || len(request.AllowedDomains) > 256 {
		return errors.New("invalid sandbox job inputs")
	}
	if err := request.Context.Validate(); err != nil {
		return err
	}
	if _, err := abi.Encode(request.Context, abi.MaxContextBytes); err != nil {
		return err
	}
	if len(request.Args) == 0 {
		request.Args = []string{"alva-worker-wasm"}
	}
	argBytes := 0
	for _, arg := range request.Args {
		argBytes += len(arg)
		if !abi.Text(arg) || strings.IndexByte(arg, 0) >= 0 {
			return errors.New("invalid guest argument")
		}
	}
	if argBytes > 64<<10 {
		return errors.New("guest argument budget exceeded")
	}
	for i, grant := range request.Grants {
		if !filepath.IsAbs(grant.Host) || !abi.GuestPath(grant.Guest) || path.Clean(grant.Guest) != grant.Guest || (grant.Access != ReadOnly && grant.Access != ReadWrite) {
			return errors.New("invalid explicit sandbox grant")
		}
		for _, prior := range request.Grants[:i] {
			if overlaps(prior.Guest, grant.Guest) {
				return errors.New("overlapping guest mounts are not accepted")
			}
		}
	}
	workspaceGranted := false
	for _, grant := range request.Grants {
		if beneath(grant.Guest, path.Clean(request.Context.Workspace)) {
			workspaceGranted = true
		}
	}
	if !workspaceGranted {
		return errors.New("guest workspace must be inside an explicit grant")
	}
	for _, domain := range request.AllowedDomains {
		if _, err := domainRule(domain); err != nil {
			return err
		}
	}
	limits := &request.Limits
	if limits.WallClock == 0 {
		limits.WallClock = time.Hour
	}
	if limits.MemoryBytes == 0 {
		limits.MemoryBytes = 256 << 20
	}
	if limits.OutputBytes == 0 {
		limits.OutputBytes = 1 << 20
	}
	if limits.MaxModelCalls == 0 {
		limits.MaxModelCalls = 128
	}
	if limits.MaxFetchCalls == 0 {
		limits.MaxFetchCalls = 256
	}
	if limits.MaxEscalations == 0 {
		limits.MaxEscalations = 16
	}
	if limits.MaxExchanges == 0 {
		limits.MaxExchanges = 8192
	}
	if limits.MaxIOBytes == 0 {
		limits.MaxIOBytes = 64 << 20
	}
	if limits.MaxOutputTokens == 0 {
		limits.MaxOutputTokens = 16384
	}
	if limits.Fuel > MaxFuel || limits.WallClock < time.Millisecond || limits.WallClock > time.Hour || limits.MemoryBytes < 16<<20 || limits.MemoryBytes > 512<<20 || limits.MemoryBytes%65536 != 0 || limits.OutputBytes < 1024 || limits.OutputBytes > 4<<20 || limits.MaxModelCalls < 1 || limits.MaxModelCalls > 1000 || limits.MaxFetchCalls < 1 || limits.MaxFetchCalls > 256 || limits.MaxEscalations < 1 || limits.MaxEscalations > 256 || limits.MaxExchanges < 1 || limits.MaxExchanges > 65536 || limits.MaxIOBytes < 1<<20 || limits.MaxIOBytes > 256<<20 || limits.MaxOutputTokens > 1<<20 {
		return errors.New("sandbox limits out of range")
	}
	return nil
}
func beneath(root, name string) bool {
	return root == "/" || name == root || strings.HasPrefix(name, root+"/")
}
func overlaps(a, b string) bool { return beneath(a, b) || beneath(b, a) }
func (j *job) abort(err error) {
	j.mu.Lock()
	j.failure = errors.Join(j.failure, err)
	j.mu.Unlock()
	j.stop()
}
func (j *job) observe(kind string, operation uint32, data []byte) error {
	if uint64(len(data)) > j.request.Limits.MaxIOBytes-j.outcome.IOBytes {
		err := errors.New("sandbox total IO byte budget exceeded; interaction stopped")
		j.abort(err)
		return err
	}
	j.outcome.IOBytes += uint64(len(data))
	if j.caps.Observer == nil {
		return nil
	}
	j.sequence++
	ctx, cancel := context.WithTimeout(context.WithoutCancel(j.ctx), 5*time.Second)
	defer cancel()
	err := j.caps.Observer.Observe(ctx, IOEvent{JobID: j.request.JobID, Sequence: j.sequence, Time: time.Now().UTC(), Kind: kind, Operation: operation, Data: append([]byte(nil), data...)})
	if err != nil {
		failure := &CallbackError{Operation: "observer", Err: err}
		j.abort(failure)
		return failure
	}
	return nil
}
func (j *job) exchange(ctx context.Context, module api.Module, operation, ptr, length uint32) uint32 {
	// The WASIp1 caller is suspended for this synchronous exchange. A script
	// instance may reserve only unused space in the same total linear-memory
	// budget; there is no parallel guest thread that can grow this memory.
	j.guestMemory = module.Memory()
	j.outcome.Exchanges++
	if j.outcome.Exchanges > j.request.Limits.MaxExchanges {
		err := errors.New("sandbox exchange count budget exceeded")
		j.abort(err)
		panic(err)
	}
	if j.pending != nil {
		panic("sandbox response not consumed")
	}
	limit := abi.MaxModelRequestBytes
	switch operation {
	case abi.OpContext:
		limit = 2
	case abi.OpModel:
	case abi.OpFetch:
		limit = abi.MaxFetchRequestBytes
	case abi.OpEscalation:
		limit = abi.MaxEscalationBytes
	case abi.OpAudit:
		limit = abi.MaxAuditBytes
	case abi.OpReferenceRead, abi.OpReferenceSearch, abi.OpHistory:
		limit = abi.MaxReferenceBytes
	case abi.OpScript:
		limit = abi.MaxScriptBytes*6 + 1024
	default:
		panic("unknown sandbox operation")
	}
	if length > uint32(limit) {
		panic("sandbox request byte limit exceeded")
	}
	data, ok := module.Memory().Read(ptr, length)
	if !ok {
		panic("sandbox request memory range invalid")
	}
	owned := append([]byte(nil), data...)
	if err := j.observe("request", operation, owned); err != nil {
		panic(err)
	}
	response, err := j.dispatch(ctx, operation, owned)
	if err != nil {
		failure := &CallbackError{Operation: "capability", Err: err}
		j.abort(failure)
		panic(failure)
	}
	if err := j.observe("response", operation, response); err != nil {
		panic(err)
	}
	if len(response) > abi.MaxResponseBytes {
		panic("sandbox response byte limit exceeded")
	}
	j.pending = response
	return uint32(len(response))
}
func (j *job) readResponse(_ context.Context, module api.Module, ptr, length uint32) uint32 {
	if j.pending == nil || length != uint32(len(j.pending)) {
		panic("sandbox response buffer mismatch")
	}
	if !module.Memory().Write(ptr, j.pending) {
		panic("sandbox response memory range invalid")
	}
	n := uint32(len(j.pending))
	j.pending = nil
	return n
}
func (j *job) dispatch(ctx context.Context, operation uint32, data []byte) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	switch operation {
	case abi.OpContext:
		if string(data) != "{}" {
			return nil, errors.New("context request must be empty object")
		}
		return abi.Encode(j.request.Context, abi.MaxContextBytes)
	case abi.OpModel:
		var request abi.ModelRequest
		if err := abi.Decode(data, abi.MaxModelRequestBytes, &request); err != nil {
			return nil, err
		}
		if err := request.Validate(); err != nil {
			return nil, err
		}
		if len(request.Config.ExtraBody) > 0 && !j.caps.AllowExtraBody {
			return nil, errors.New("guest extra provider body not delegated")
		}
		maxTokens := j.request.Limits.MaxOutputTokens
		if request.Config.MaxTokens != nil && *request.Config.MaxTokens > maxTokens {
			return nil, errors.New("guest requested output tokens above host budget")
		}
		j.outcome.ModelCalls++
		if j.outcome.ModelCalls > j.request.Limits.MaxModelCalls {
			return nil, errors.New("model proxy call budget exceeded")
		}
		if j.caps.Model == nil {
			return nil, errors.New("model proxy unavailable")
		}
		actual := kernelabi.ModelRequest{Messages: wire.CloneMessages(request.Messages), Config: wire.CloneModelConfig(request.Config)}
		if actual.Config.MaxTokens == nil {
			actual.Config.MaxTokens = &maxTokens
		}
		for _, tool := range request.Tools {
			actual.Tools = append(actual.Tools, wire.ToolDefinition{Name: tool.Name, Description: tool.Description, Parameters: append([]byte(nil), tool.Parameters...)})
		}
		response := abi.ModelResponse{Version: abi.Version}
		size := 0
		var callbackMu sync.Mutex
		var callbackErr error
		callbackOpen := true
		err := j.caps.Model.Stream(ctx, actual, func(chunk wire.StreamChunk) error {
			callbackMu.Lock()
			defer callbackMu.Unlock()
			if !callbackOpen {
				return errors.New("model proxy stream callback closed")
			}
			if callbackErr != nil {
				return callbackErr
			}
			if err := ctx.Err(); err != nil {
				callbackErr = err
				return err
			}
			encoded, err := abi.Encode(chunk, abi.MaxResponseBytes)
			if err != nil {
				callbackErr = err
				return err
			}
			size += len(encoded)
			if size > abi.MaxResponseBytes-128 || len(response.Chunks) >= 65536 {
				callbackErr = errors.New("model response budget exceeded")
				return callbackErr
			}
			response.Chunks = append(response.Chunks, wire.CloneStreamChunk(chunk))
			return nil
		})
		callbackMu.Lock()
		callbackOpen = false
		err = errors.Join(err, callbackErr)
		callbackMu.Unlock()
		if err != nil {
			return nil, err
		}
		if err := response.Validate(); err != nil {
			return nil, err
		}
		return abi.Encode(response, abi.MaxResponseBytes)
	case abi.OpFetch:
		var request abi.FetchRequest
		if err := abi.Decode(data, abi.MaxFetchRequestBytes, &request); err != nil {
			return nil, err
		}
		if err := request.Validate(); err != nil {
			return nil, err
		}
		j.outcome.FetchCalls++
		result := abi.FetchResult{Version: abi.Version}
		if j.outcome.FetchCalls > j.request.Limits.MaxFetchCalls {
			result.Error = "fetch call budget exceeded"
		} else {
			response, err := executeFetch(ctx, j.request.AllowedDomains, request)
			if err != nil {
				result.Error = err.Error()
			} else {
				result.Response = &response
			}
		}
		if err := result.Validate(); err != nil {
			return nil, err
		}
		return abi.Encode(result, abi.MaxResponseBytes)
	case abi.OpEscalation:
		var request abi.EscalationRequest
		if err := abi.Decode(data, abi.MaxEscalationBytes, &request); err != nil {
			return nil, err
		}
		if err := request.Validate(); err != nil {
			return nil, err
		}
		j.outcome.Escalations++
		result, err := j.escalate(ctx, request)
		if err != nil {
			return nil, err
		}
		return abi.Encode(result, abi.MaxResponseBytes)
	case abi.OpAudit:
		var event abi.AuditEvent
		if err := abi.Decode(data, abi.MaxAuditBytes, &event); err != nil {
			return nil, err
		}
		if err := event.Validate(); err != nil {
			return nil, err
		}
		return nil, nil
	case abi.OpScript:
		var request abi.ScriptRequest
		if err := abi.Decode(data, abi.MaxScriptBytes*6+1024, &request); err != nil {
			return nil, err
		}
		if err := request.Validate(); err != nil {
			return nil, err
		}
		result, err := j.script(ctx, request)
		if err != nil {
			return nil, err
		}
		if err = result.Validate(); err != nil {
			return nil, err
		}
		return abi.Encode(result, abi.MaxScriptResultBytes)
	case abi.OpReferenceRead:
		var request abi.ReferenceRead
		if err := abi.Decode(data, abi.MaxReferenceBytes, &request); err != nil {
			return nil, err
		}
		if err := request.Validate(); err != nil {
			return nil, err
		}
		if j.caps.References == nil {
			return nil, errors.New("native reference reader unavailable")
		}
		page, err := j.caps.References.Read(ctx, request)
		if err != nil {
			return nil, err
		}
		result := abi.ReferenceResult{Version: abi.Version, Page: &page}
		if err := result.ValidateRead(request); err != nil {
			return nil, err
		}
		return abi.Encode(result, abi.MaxReferenceBytes)
	case abi.OpReferenceSearch:
		var request abi.ReferenceSearch
		if err := abi.Decode(data, abi.MaxReferenceBytes, &request); err != nil {
			return nil, err
		}
		if err := request.Validate(); err != nil {
			return nil, err
		}
		if j.caps.References == nil {
			return nil, errors.New("native reference reader unavailable")
		}
		matches, err := j.caps.References.Search(ctx, request)
		if err != nil {
			return nil, err
		}
		result := abi.ReferenceResult{Version: abi.Version, Matches: matches}
		if err := result.ValidateSearch(request); err != nil {
			return nil, err
		}
		return abi.Encode(result, abi.MaxReferenceBytes)
	case abi.OpHistory:
		var request abi.HistoryRequest
		if err := abi.Decode(data, abi.MaxReferenceBytes, &request); err != nil {
			return nil, err
		}
		if err := request.Validate(); err != nil {
			return nil, err
		}
		if j.caps.History == nil {
			return nil, errors.New("native history reader unavailable")
		}
		page, err := j.caps.History.Page(ctx, request)
		if err != nil {
			return nil, err
		}
		if err := page.Validate(request); err != nil {
			return nil, err
		}
		return abi.Encode(page, abi.MaxReferenceBytes*4)
	}
	return nil, errors.New("unknown sandbox operation")
}
func (j *job) escalate(ctx context.Context, request abi.EscalationRequest) (abi.EscalationResult, error) {
	result := abi.EscalationResult{Version: abi.Version}
	if !j.caps.AllowUnsandboxed || !j.request.Context.Policy.AllowEscalationRequest || j.caps.Approval == nil || j.caps.Executor == nil {
		result.Error = "unsandboxed host execution is not enabled"
		return result, nil
	}
	if j.outcome.Escalations > j.request.Limits.MaxEscalations {
		result.Error = "escalation call budget exceeded"
		return result, nil
	}
	target, err := j.target(request)
	if err != nil {
		result.Error = "escalation cwd is outside authorized directories"
		return result, nil
	}
	defer target.Directory.Close()
	ctx, cancel := context.WithTimeout(ctx, time.Duration(request.TimeoutMS)*time.Millisecond)
	defer cancel()
	approved, err := j.caps.Approval.Approve(ctx, target)
	if err != nil {
		return result, err
	}
	if !approved {
		result.Error = "host approval rejected unsandboxed execution"
		return result, nil
	}
	if err := ctx.Err(); err != nil {
		return result, err
	}
	response, err := j.caps.Executor.Execute(ctx, target)
	if err != nil {
		return result, err
	}
	result.Response = &response
	if err := result.Validate(); err != nil {
		return result, err
	}
	return result, nil
}
func (j *job) target(request abi.EscalationRequest) (EscalationTarget, error) {
	name := path.Clean(request.Cwd)
	mounts := append([]mount(nil), j.mounts...)
	sort.Slice(mounts, func(a, b int) bool { return len(mounts[a].grant.Guest) > len(mounts[b].grant.Guest) })
	for _, m := range mounts {
		if !beneath(m.grant.Guest, name) {
			continue
		}
		relative := strings.TrimPrefix(strings.TrimPrefix(name, m.grant.Guest), "/")
		if relative == "" {
			relative = "."
		}
		directory, err := m.root.OpenRoot(filepath.FromSlash(relative))
		if err != nil {
			return EscalationTarget{}, err
		}
		return EscalationTarget{JobID: j.request.JobID, Request: request, Directory: directory, HostPath: filepath.Join(m.grant.Host, filepath.FromSlash(relative))}, nil
	}
	return EscalationTarget{}, fmt.Errorf("ungranted guest cwd")
}
