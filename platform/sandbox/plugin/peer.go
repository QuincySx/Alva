// INPUT: Explicit WASIp1 bytes, contexts, wazero and bounded in-memory stdio.
// OUTPUT: Config, Start and Peer execution ownership for a single isolated module.
// POS: [platform / WASM extension] Provides an execution transport without protocol, storage or native filesystem authority.
package plugin

import (
	"bytes"
	"context"
	"errors"
	"io"
	"sync"
	"time"

	"github.com/tetratelabs/wazero"
	"github.com/tetratelabs/wazero/api"
	"github.com/tetratelabs/wazero/imports/wasi_snapshot_preview1"
	"github.com/tetratelabs/wazero/sys"
)

const MaxModuleBytes = 8 << 20

type Config struct {
	Module      []byte
	MemoryBytes uint64
	MaxIOBytes  uint64
	WallClock   time.Duration
}

// Peer is per-instance, not a compilation/authority singleton. No environment,
// preopens, network, model or host import is supplied. AEP is an outer consumer.
type Peer struct {
	inputR, outputR, errorR *io.PipeReader
	inputW, outputW, errorW *io.PipeWriter
	runtime                 wazero.Runtime
	cancel                  context.CancelFunc
	stop                    func() bool
	done                    chan struct{}
	once                    sync.Once
	mu                      sync.Mutex
	remaining               uint64
	err                     error
	code                    int
	closing                 bool
}

func Start(ctx context.Context, config Config) (*Peer, error) {
	if len(config.Module) < 8 || len(config.Module) > MaxModuleBytes {
		return nil, errors.New("WASM plugin module must be 8 bytes..8 MiB")
	}
	if config.MemoryBytes == 0 {
		config.MemoryBytes = 64 << 20
	}
	if config.MaxIOBytes == 0 {
		config.MaxIOBytes = 64 << 20
	}
	if config.WallClock == 0 {
		config.WallClock = 10 * time.Minute
	}
	if config.MemoryBytes < 16<<20 || config.MemoryBytes > 256<<20 || config.MemoryBytes%65536 != 0 || config.MaxIOBytes < 1024 || config.MaxIOBytes > 64<<20 || config.WallClock < time.Millisecond || config.WallClock > 30*time.Minute {
		return nil, errors.New("WASM plugin memory/IO/lifetime outside bounds")
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	// Start's context covers compilation and admission; the owner controls the
	// returned instance through Close, plus an independent hard total lifetime.
	deadline := time.Now().Add(config.WallClock)
	compile, stopCompile := context.WithDeadline(ctx, deadline)
	defer stopCompile()
	runtime := wazero.NewRuntimeWithConfig(compile, wazero.NewRuntimeConfig().WithCoreFeatures(api.CoreFeaturesV2).WithMemoryLimitPages(uint32(config.MemoryBytes/65536)).WithCloseOnContextDone(true))
	module, err := runtime.CompileModule(compile, bytes.Clone(config.Module))
	if err != nil {
		_ = runtime.Close(context.Background())
		return nil, err
	}
	for _, imported := range module.ImportedFunctions() {
		name, _, _ := imported.Import()
		if name != wasi_snapshot_preview1.ModuleName {
			_ = runtime.Close(context.Background())
			return nil, errors.New("WASM plugin imports undelegated host module")
		}
	}
	if len(module.ImportedMemories()) != 0 {
		_ = runtime.Close(context.Background())
		return nil, errors.New("WASM plugin imported memory is unsupported")
	}
	if _, err = wasi_snapshot_preview1.Instantiate(compile, runtime); err != nil {
		_ = runtime.Close(context.Background())
		return nil, err
	}
	if err = compile.Err(); err != nil {
		_ = runtime.Close(context.Background())
		return nil, err
	}
	worker, cancel := context.WithDeadline(context.Background(), deadline)
	p := &Peer{runtime: runtime, cancel: cancel, done: make(chan struct{}), remaining: config.MaxIOBytes, code: -1}
	p.inputR, p.inputW = io.Pipe()
	p.outputR, p.outputW = io.Pipe()
	p.errorR, p.errorW = io.Pipe()
	p.stop = context.AfterFunc(worker, p.closePipes)
	go func() {
		_, runErr := runtime.InstantiateModule(worker, module, wazero.NewModuleConfig().WithName("").WithArgs("plugin.wasm").WithStdin(p.inputR).WithStdout(budgetOutput{p, p.outputW}).WithStderr(budgetOutput{p, p.errorW}))
		code := 0
		if runErr != nil {
			var exit *sys.ExitError
			if errors.As(runErr, &exit) {
				code = int(exit.ExitCode())
				if code == 0 {
					runErr = nil
				}
				p.mu.Lock()
				if p.closing && exit.ExitCode() == sys.ExitCodeContextCanceled {
					runErr = nil
				}
				p.mu.Unlock()
			} else {
				code = -1
			}
		}
		// Runtime Close and blocked stdio complete before done is published.
		runErr = errors.Join(runErr, runtime.Close(context.Background()))
		if !p.stop() {
			p.closePipes()
		}
		// Natural exit gives consumers EOF so they can drain final diagnostics.
		p.inputR.Close()
		p.outputW.Close()
		p.errorW.Close()
		cancel()
		p.mu.Lock()
		p.err = errors.Join(p.err, runErr)
		p.code = code
		p.mu.Unlock()
		close(p.done)
	}()
	return p, nil
}

func (p *Peer) closePipes() {
	p.once.Do(func() {
		p.inputR.Close()
		p.inputW.Close()
		p.outputW.Close()
		p.outputR.Close()
		p.errorW.Close()
		p.errorR.Close()
	})
}
func (p *Peer) reserve(n int) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if uint64(n) > p.remaining {
		err := errors.New("WASM plugin total IO budget exceeded")
		p.err = errors.Join(p.err, err)
		p.cancel()
		return err
	}
	p.remaining -= uint64(n)
	return nil
}

type budgetOutput struct {
	p      *Peer
	writer io.Writer
}

func (w budgetOutput) Write(data []byte) (int, error) {
	if err := w.p.reserve(len(data)); err != nil {
		return 0, err
	}
	return w.writer.Write(data)
}

type budgetInput struct{ p *Peer }

func (w budgetInput) Write(data []byte) (int, error) {
	if err := w.p.reserve(len(data)); err != nil {
		return 0, err
	}
	return w.p.inputW.Write(data)
}
func (w budgetInput) Close() error    { return w.p.inputW.Close() }
func (p *Peer) Input() io.WriteCloser { return budgetInput{p} }
func (p *Peer) Output() io.ReadCloser { return p.outputR }
func (p *Peer) Errors() io.ReadCloser { return p.errorR }
func (p *Peer) BeginClose() {
	p.mu.Lock()
	p.closing = true
	p.mu.Unlock()
	p.cancel()
	p.closePipes()
}
func (p *Peer) Wait(ctx context.Context) error {
	select {
	case <-p.done:
		p.mu.Lock()
		defer p.mu.Unlock()
		return p.err
	case <-ctx.Done():
		return ctx.Err()
	}
}
func (p *Peer) Close(ctx context.Context) error { p.BeginClose(); return p.Wait(ctx) }
func (p *Peer) ExitCode() (int, bool) {
	select {
	case <-p.done:
		p.mu.Lock()
		defer p.mu.Unlock()
		return p.code, true
	default:
		return 0, false
	}
}
func (p *Peer) CleanupComplete() bool {
	select {
	case <-p.done:
		p.mu.Lock()
		defer p.mu.Unlock()
		return p.closing
	default:
		return false
	}
}
