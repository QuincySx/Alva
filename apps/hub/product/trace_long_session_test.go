// INPUT: Actual native SQLite facts behind a call-counting Inspector, committed product Turns and optional synthetic GiB native history
// OUTPUT: Long-session trace gate: opening from the tail costs a fixed number of bounded reads and window bytes independent of history size; ALVA_LONG_SESSION_GIB reports first-screen time and memory
// POS: [L7 Hub product] CI gate for the trace data path; the full-size run stays opt-in outside CI
package product

import (
	"context"
	"encoding/json"
	"os"
	"runtime"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type countingTraceInspector struct {
	engine.Inspector
	pages, reads atomic.Int64
}

func (i *countingTraceInspector) Page(ctx context.Context, scope engine.InspectionScope, query engine.InspectionQuery) (engine.InspectionPage, error) {
	i.pages.Add(1)
	return i.Inspector.Page(ctx, scope, query)
}

func (i *countingTraceInspector) Read(ctx context.Context, scope engine.InspectionScope, uri string, offset int64) (engine.InspectionText, error) {
	i.reads.Add(1)
	return i.Inspector.Read(ctx, scope, uri, offset)
}

// countingTargets hands the trace reader the routed Inspector behind a counter.
type countingTargets struct {
	inner    traceTargets
	counting *countingTraceInspector
}

func (c countingTargets) traceTarget(ctx context.Context, id, branchID string, path []ChildHop) (engine.Inspector, engine.InspectionScope, error) {
	inspector, scope, err := c.inner.traceTarget(ctx, id, branchID, path)
	if err != nil {
		return nil, scope, err
	}
	c.counting.Inspector = inspector
	return c.counting, scope, nil
}

// countTrace routes the fixture's trace reads through a counting Inspector.
func countTrace(t *testing.T, f *traceFixture) *countingTraceInspector {
	t.Helper()
	counting := &countingTraceInspector{}
	f.s.Traces.targets = countingTargets{inner: f.s, counting: counting}
	return counting
}

// seedNativeBulk appends roughly bytes of extension records inside one open
// tool call of the first Turn, as a long-running Agent session would.
func seedNativeBulk(t *testing.T, f *traceFixture, bytes int64, payload int) int64 {
	t.Helper()
	claim, named := f.turn(t, traceTurn{tools: 1, open: true})
	body, _ := json.Marshal(map[string]string{"progress": strings.Repeat("长会话进度 progress ", payload/24)})
	written := int64(0)
	for written < bytes {
		batch := make([]abi.SessionRecord, 256)
		for i := range batch {
			batch[i] = abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "fixture", Type: "progress", SchemaVersion: 1, Payload: body}
		}
		if _, err := f.log.AppendRecords(t.Context(), batch); err != nil {
			t.Fatal(err)
		}
		written += int64(len(batch) * len(body))
	}
	result := f.append(t, abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: "run-1", CallID: "call-0", Status: abi.OutcomeSucceeded, Result: wire.ToolResult{CallID: "call-0"}}})
	f.record(t, claim.Attempt.ID, engine.Event{Kind: engine.ToolEnd, ID: "call-0", Name: "read_file", Reference: ref(f.native, result[0])})
	f.finish(t, claim, named, 1, "")
	return written
}

type traceOpen struct {
	elapsed       time.Duration
	pages, reads  int64
	windowBytes   int
	heapDelta     int64
	records, last int
}

func openTraceTail(t *testing.T, f *traceFixture, counting *countingTraceInspector) traceOpen {
	t.Helper()
	runtime.GC()
	var before runtime.MemStats
	runtime.ReadMemStats(&before)
	pages, reads := counting.pages.Load(), counting.reads.Load()
	start := time.Now()
	chunk, err := f.s.Traces.Tail(t.Context(), f.target())
	if err != nil {
		t.Fatal(err)
	}
	w := NewTraceWindow(0)
	w.Reset(chunk)
	steps := w.Steps()
	elapsed := time.Since(start)
	runtime.GC()
	var after runtime.MemStats
	runtime.ReadMemStats(&after)
	result := traceOpen{elapsed: elapsed, pages: counting.pages.Load() - pages, reads: counting.reads.Load() - reads, windowBytes: w.Bytes(), heapDelta: int64(after.HeapAlloc) - int64(before.HeapAlloc), records: w.Records(), last: len(steps)}
	runtime.KeepAlive(w)
	if len(steps) == 0 || steps[len(steps)-1].Turn == 0 {
		t.Fatal("tail window has no labelled steps")
	}
	return result
}

func TestLongSessionTraceGate(t *testing.T) {
	gib := 0.0
	if value := os.Getenv("ALVA_LONG_SESSION_GIB"); value != "" {
		parsed, err := strconv.ParseFloat(value, 64)
		if err != nil || parsed <= 0 {
			t.Fatal("ALVA_LONG_SESSION_GIB must be a positive number of GiB", value)
		}
		gib = parsed
	}
	measure := func(t *testing.T, bytes int64, payload int) traceOpen {
		f := newTraceFixture(t)
		start := time.Now()
		written := seedNativeBulk(t, f, bytes, payload)
		for range 20 {
			f.turn(t, traceTurn{tools: 3, timed: true})
		}
		counting := countTrace(t, f)
		open := openTraceTail(t, f, counting)
		t.Logf("native bulk %.3f GiB seeded in %s: tail %s, %d Inspector pages, %d body reads, %d records, %d steps, window %d B, heap delta %d B",
			float64(written)/(1<<30), time.Since(start).Round(time.Millisecond), open.elapsed, open.pages, open.reads, open.records, open.last, open.windowBytes, open.heapDelta)
		return open
	}
	small := measure(t, 256<<10, 1<<10)
	large := measure(t, 8<<20, 2<<10)
	// Opening reads a fixed number of metadata pages and no bodies, whatever
	// the history size; the window retains the same bounded metadata.
	if small.reads != 0 || large.reads != 0 || large.pages != small.pages || large.pages > int64(traceTailRecords/traceInspectorPage+2) {
		t.Fatal("tail open depends on history size or reads bodies", small, large)
	}
	if large.windowBytes > traceDefaultBytes || large.windowBytes > 2*small.windowBytes {
		t.Fatal("tail window memory grew with history", small.windowBytes, large.windowBytes)
	}
	if gib > 0 {
		full := measure(t, int64(gib*(1<<30)), 64<<10)
		if full.elapsed > 100*time.Millisecond {
			t.Fatal("first trace screen exceeds 100 ms on the long session", full.elapsed)
		}
		if full.pages != large.pages || full.windowBytes > 2*large.windowBytes {
			t.Fatal("full-size tail open depends on history size", full, large)
		}
	}
}
