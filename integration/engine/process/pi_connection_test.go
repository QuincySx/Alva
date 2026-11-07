// INPUT: Slow Pi event consumers and captured native image projections.
// OUTPUT: Queue byte/count limits, released capacity and absent retained media body regressions.
// POS: [integration / native external-engine adapter] Original media stays in native storage, never in the event backlog.
package process

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	owned "github.com/QuincySx/Alva/platform/system/process"
)

func TestPiEventQueueBoundsBytesWithSlowConsumerAndReleasesCapacity(t *testing.T) {
	c := &piConnection{frames: make(chan json.RawMessage, 64), done: make(chan struct{})}
	frame := json.RawMessage(`{"type":"message_update","text":"` + strings.Repeat("x", 1<<20) + `"}`)
	count := 0
	for c.enqueue(frame) == nil {
		count++
	}
	if count == 0 || count >= 64 || c.queuedBytes > piEventQueueBytes || c.queuedBytes != len(frame)*count {
		t.Fatal("slow consumer retained more than its total byte budget", count, c.queuedBytes)
	}
	for range count {
		c.releaseFrame(<-c.frames)
	}
	if c.queuedBytes != 0 || c.enqueue(frame) != nil {
		t.Fatal("consumption did not release its exact admitted capacity", c.queuedBytes)
	}
}

func TestPiEventQueueKeepsOnlyMediaFreeProjection(t *testing.T) {
	image := strings.Repeat("QUJD", 4<<20)
	value := map[string]any{"type": "message_end", "message": map[string]any{"content": []any{map[string]any{"type": "image", "data": image}}}}
	omitPiMedia(value)
	frame, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	c := &piConnection{frames: make(chan json.RawMessage, 64), done: make(chan struct{})}
	for range 64 {
		if err := c.enqueue(frame); err != nil {
			t.Fatal(err)
		}
	}
	consumed := <-c.frames
	c.releaseFrame(consumed)
	if c.queuedBytes >= 64<<10 || strings.Contains(string(consumed), image[:128]) {
		t.Fatal("small projected events retained native media in the backlog", c.queuedBytes)
	}
	if c.enqueue(frame) == nil {
		// One slot was consumed above, so exactly one new frame is admissible.
		if c.enqueue(frame) == nil {
			t.Fatal("record count budget was lost")
		}
	} else {
		t.Fatal("the free record slot was not available")
	}
}

func TestPiEventQueueNativeFixtureProcess(t *testing.T) {
	if os.Getenv("ALVA_PI_QUEUE_FIXTURE") != "1" {
		return
	}
	data, err := json.Marshal(map[string]any{"type": "message_end", "message": map[string]any{"role": "user", "content": []any{map[string]any{"type": "image", "data": strings.Repeat("QUJD", 4<<20)}}}})
	if err != nil {
		os.Exit(81)
	}
	fmt.Println(string(data))
	os.Exit(0)
}

func TestPiEventQueueNativeReaderDoesNotRetainLargeImageWithStoppedConsumer(t *testing.T) {
	file, err := os.OpenFile(filepath.Join(t.TempDir(), "journal.jsonl"), os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	proc, err := owned.Start(owned.Config{Command: binary, Args: []string{"-test.run=^TestPiEventQueueNativeFixtureProcess$"}, Cwd: t.TempDir(), Env: map[string]string{"ALVA_PI_QUEUE_FIXTURE": "1"}})
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := proc.Close(ctx); err != nil {
			t.Error(err)
		}
	}()
	c := newPiConnection(proc, &journal{file: file, limit: 32 << 20})
	select {
	case <-c.readDone:
	case <-time.After(10 * time.Second):
		proc.BeginClose()
		t.Fatal("native reader did not reach its actual end")
	}
	frame := <-c.frames
	if len(frame) == 0 || len(frame) > 1024 || c.queuedBytes != len(frame) || strings.Contains(string(frame), strings.Repeat("QUJD", 32)) || !strings.Contains(string(frame), "omitted_encoded_bytes") {
		t.Fatal("native reader retained the original media record while its sink was stopped", len(frame), c.queuedBytes)
	}
	c.releaseFrame(frame)
}
