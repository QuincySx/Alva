// INPUT: Minimal real WASM programs, explicit resource budgets and owned byte streams.
// OUTPUT: Memory/CPU/IO limits, actual exit, blocked IO cleanup and retained error regressions.
// POS: [platform / WASM extension] Tests isolated execution independently of AEP, SDK or product policy.
package plugin

import (
	"context"
	"encoding/binary"
	"io"
	"strings"
	"testing"
	"time"
)

func uleb(b []byte, value uint32) []byte {
	for {
		x := byte(value & 127)
		value >>= 7
		if value != 0 {
			x |= 128
		}
		b = append(b, x)
		if value == 0 {
			return b
		}
	}
}
func section(b []byte, id byte, payload []byte) []byte {
	b = append(b, id)
	b = uleb(b, uint32(len(payload)))
	return append(b, payload...)
}
func program(body []byte, imports, memory []byte) []byte {
	b := []byte{0, 97, 115, 109, 1, 0, 0, 0}
	b = section(b, 1, []byte{2, 0x60, 0, 0, 0x60, 4, 0x7f, 0x7f, 0x7f, 0x7f, 1, 0x7f})
	index := byte(0)
	if imports != nil {
		b = section(b, 2, imports)
		index = 1
	}
	b = section(b, 3, []byte{1, 0})
	if memory != nil {
		b = section(b, 5, memory)
	}
	b = section(b, 7, []byte{1, 6, '_', 's', 't', 'a', 'r', 't', 0, index})
	code := uleb([]byte{1}, uint32(len(body)+1))
	code = append(code, 0) // no locals
	return section(b, 10, append(code, body...))
}
func wasiImport(name string) []byte {
	b := uleb([]byte{1}, uint32(len("wasi_snapshot_preview1")))
	b = append(b, "wasi_snapshot_preview1"...)
	b = uleb(b, uint32(len(name)))
	b = append(b, name...)
	return append(b, 0, 1) // function, type 1
}
func constant(value int32) []byte {
	b := []byte{0x41}
	for {
		x := byte(value & 127)
		value >>= 7
		more := !(value == 0 && x&64 == 0 || value == -1 && x&64 != 0)
		if more {
			x |= 128
		}
		b = append(b, x)
		if !more {
			return b
		}
	}
}
func ioProgram(name string, fd, count int32) []byte {
	var body []byte
	// iovec { pointer: 1024, length: count } at address zero.
	for offset, value := range []int32{1024, count} {
		body = append(body, constant(int32(offset*4))...)
		body = append(body, constant(value)...)
		body = append(body, 0x36, 2, 0)
	}
	for _, value := range []int32{fd, 0, 1, 8} {
		body = append(body, constant(value)...)
	}
	body = append(body, 0x10, 0, 0x1a, 0x0b)
	return program(body, wasiImport(name), []byte{1, 0, 1})
}
func closePeer(t *testing.T, p *Peer) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	_ = p.Close(ctx)
	if !p.CleanupComplete() {
		t.Error("runtime/stdio cleanup incomplete")
	}
}

func TestRealMemoryAndUndelegatedImportLimits(t *testing.T) {
	tooLarge := program([]byte{0x0b}, nil, append([]byte{1, 0}, uleb(nil, 257)...))
	foreign := program([]byte{0x0b}, []byte{1, 3, 'e', 'n', 'v', 1, 'f', 0, 1}, nil)
	for name, module := range map[string][]byte{"initial-memory": tooLarge, "undelegated-import": foreign} {
		t.Run(name, func(t *testing.T) {
			p, err := Start(t.Context(), Config{Module: module, MemoryBytes: 16 << 20})
			if p != nil {
				closePeer(t, p)
			}
			if err == nil {
				t.Fatal("undelegated resource admitted")
			}
		})
	}
	// memory.grow beyond the configured 256 pages must return -1.
	body := append(constant(256), 0x40, 0)
	body = append(body, constant(-1)...)
	body = append(body, 0x47, 0x04, 0x40, 0, 0x0b, 0x0b) // if != -1, trap
	p, err := Start(t.Context(), Config{Module: program(body, nil, []byte{1, 0, 1}), MemoryBytes: 16 << 20})
	if err != nil {
		t.Fatal(err)
	}
	defer closePeer(t, p)
	if err = p.Wait(t.Context()); err != nil {
		t.Fatal("guest escaped memory ceiling", err)
	}
	if code, ok := p.ExitCode(); !ok || code != 0 {
		t.Fatal("guest did not really exit", code, ok)
	}
}

func TestTotalIOBudgetCoversInputOutputAndDiagnostics(t *testing.T) {
	for _, stream := range []string{"stdin", "stdout", "stderr"} {
		t.Run(stream, func(t *testing.T) {
			module := ioProgram("fd_write", 1, 1025)
			if stream == "stdin" {
				module = ioProgram("fd_read", 0, 1)
			}
			if stream == "stderr" {
				module = ioProgram("fd_write", 2, 1025)
			}
			p, err := Start(t.Context(), Config{Module: module, MaxIOBytes: 1024})
			if err != nil {
				t.Fatal(err)
			}
			defer closePeer(t, p)
			if stream == "stdin" {
				if _, err = p.Input().Write(make([]byte, 1025)); err == nil {
					t.Fatal("oversize input succeeded")
				}
			}
			ctx, stop := context.WithTimeout(t.Context(), 3*time.Second)
			defer stop()
			err = p.Wait(ctx)
			if err == nil || !strings.Contains(err.Error(), "IO budget exceeded") {
				t.Fatal("IO error was lost or execution survived", err)
			}
			closePeer(t, p)
			if again := p.Close(ctx); again == nil || !strings.Contains(again.Error(), "IO budget exceeded") {
				t.Fatal("Close erased the actual failure", again)
			}
		})
	}
}

func TestHardLifetimeAndOwnerCloseInterruptRealExecution(t *testing.T) {
	loop := program([]byte{0x03, 0x40, 0x0c, 0, 0x0b, 0x0b}, nil, nil)
	p, err := Start(t.Context(), Config{Module: loop, WallClock: 200 * time.Millisecond})
	if err != nil {
		t.Fatal(err)
	}
	defer closePeer(t, p)
	ctx, stop := context.WithTimeout(t.Context(), 3*time.Second)
	defer stop()
	if err = p.Wait(ctx); err == nil || ctx.Err() != nil {
		t.Fatal("hard deadline did not stop CPU loop", err)
	}
	if _, ok := p.ExitCode(); !ok {
		t.Fatal("deadline returned without actual module exit")
	}
	for _, module := range [][]byte{loop, ioProgram("fd_read", 0, 1), ioProgram("fd_write", 1, 16)} {
		p, err := Start(t.Context(), Config{Module: module})
		if err != nil {
			t.Fatal(err)
		}
		// Either CPU work or a WASI pipe is active; Close must join both paths.
		if err = p.Close(ctx); err != nil || !p.CleanupComplete() {
			t.Fatal("owner close did not settle actual work", err)
		}
		if err = p.Close(ctx); err != nil {
			t.Fatal("idempotent close failed", err)
		}
	}
}

func TestNaturalExitDrainsOutputBeforeOwnerCleanup(t *testing.T) {
	p, err := Start(t.Context(), Config{Module: ioProgram("fd_write", 1, 16)})
	if err != nil {
		t.Fatal(err)
	}
	defer closePeer(t, p)
	data, err := io.ReadAll(p.Output())
	if err != nil || len(data) != 16 || binary.LittleEndian.Uint32(data[:4]) != 0 {
		t.Fatal("final stdout lost or no EOF", len(data), err)
	}
	if err = p.Wait(t.Context()); err != nil {
		t.Fatal(err)
	}
	if p.CleanupComplete() {
		t.Fatal("unclosed consumer endpoints claimed owner cleanup")
	}
	closePeer(t, p)
}

func TestStartContextDoesNotReplaceReturnedExecutionOwner(t *testing.T) {
	admission, cancel := context.WithCancel(t.Context())
	p, err := Start(admission, Config{Module: ioProgram("fd_read", 0, 1), WallClock: 3 * time.Second})
	if err != nil {
		cancel()
		t.Fatal(err)
	}
	defer closePeer(t, p)
	cancel()
	// The admitted module still belongs to its returned owner, independently
	// of the now-cancelled Start context. Actual fd_read must consume input.
	if _, err = p.Input().Write([]byte{42}); err != nil {
		t.Fatal("admission cancellation killed transferred execution", err)
	}
	if err = p.Wait(t.Context()); err != nil {
		t.Fatal(err)
	}
	if code, ok := p.ExitCode(); !ok || code != 0 {
		t.Fatal("actual read did not finish", code, ok)
	}
}
