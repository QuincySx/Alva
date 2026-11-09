// INPUT: Owned Pi RPC stdio and bounded private native journals
// OUTPUT: Correlated command replies (write owned by one context, reply wait bounded by another), continuous JSONL events and serialized native input
// POS: [integration / native external-engine adapter] Failed or interrupted commands never replay through another transport
package process

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strconv"
	"strings"
	"sync"
	"unicode/utf8"

	owned "github.com/QuincySx/Alva/platform/system/process"
)

const piEventQueueBytes = 8 << 20

type piReply struct {
	command string
	data    json.RawMessage
	err     error
}
type piConnection struct {
	proc           *owned.Process
	journal        *journal
	frames         chan json.RawMessage
	done, readDone chan struct{}
	mu, writeMu    sync.Mutex
	next           uint64
	pending        map[string]chan piReply
	failure        error
	queuedBytes    int
}

func newPiConnection(proc *owned.Process, j *journal) *piConnection {
	c := &piConnection{proc: proc, journal: j, frames: make(chan json.RawMessage, 64), done: make(chan struct{}), readDone: make(chan struct{}), pending: make(map[string]chan piReply)}
	go c.read()
	return c
}
func (c *piConnection) fail(err error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.failure != nil {
		return
	}
	c.failure = err
	for id, reply := range c.pending {
		reply <- piReply{err: err}
		delete(c.pending, id)
	}
	close(c.done)
}
func (c *piConnection) read() {
	defer close(c.readDone)
	defer close(c.frames)
	err := owned.ReadLines(c.proc.Stdout, 64<<20, func(line owned.StreamLine) error {
		if line.Truncated || !line.Complete || !utf8.Valid(line.Data) {
			return errors.New("Pi RPC stdout record is incomplete or exceeds 64 MiB")
		}
		body := bytes.TrimSpace(line.Data)
		var logged map[string]any
		if len(body) == 0 || body[0] != '{' || json.Unmarshal(body, &logged) != nil || logged == nil {
			return errors.New("Pi RPC emitted malformed JSONL")
		}
		// Native session files retain media bodies. This bounded journal records
		// their shape without duplicating base64 images into every lifecycle frame.
		omitPiMedia(logged)
		safe, err := json.Marshal(logged)
		if err != nil {
			return err
		}
		if len(safe) > piEventQueueBytes {
			return errors.New("Pi RPC projected record exceeds 8 MiB")
		}
		frame := decode(safe)
		c.journal.mu.Lock()
		protected := len(c.journal.secrets) != 0
		c.journal.mu.Unlock()
		complete := false
		switch str(frame, "type") {
		case "response", "extension_ui_request", "extension_error", "message_end", "tool_execution_start", "tool_execution_end", "agent_start", "agent_end", "agent_settled", "turn_start", "turn_end":
			complete = true
		}
		if err := c.journal.writeBody("stdout", string(safe), protected && !complete); err != nil {
			return err
		}
		if str(frame, "type") == "response" {
			id := str(frame, "id")
			c.mu.Lock()
			reply := c.pending[id]
			delete(c.pending, id)
			c.mu.Unlock()
			if reply != nil {
				result := piReply{command: str(frame, "command"), data: frame["data"]}
				if !boolean(frame, "success") {
					result.err = errors.New("Pi RPC command failed; inspect its native journal")
					// pi rpc-mode answers an unrecognised command with "Unknown command: <type>".
					if strings.HasPrefix(str(frame, "error"), "Unknown command:") {
						result.err = errors.Join(result.err, errMethodNotFound)
					}
				}
				reply <- result
			}
			return nil
		}
		return c.enqueue(safe)
	})
	if err == nil {
		err = io.EOF
	}
	c.fail(err)
	if !errors.Is(err, io.EOF) {
		c.proc.BeginClose()
	}
}

// The queue owns only the media-free projection. Count bytes as well as
// records: a slow sink must never retain 64 full native image messages.
func (c *piConnection) enqueue(frame json.RawMessage) error {
	c.mu.Lock()
	if len(frame) > piEventQueueBytes-c.queuedBytes {
		c.mu.Unlock()
		return errors.New("Pi RPC event backlog exceeds 8 MiB")
	}
	c.queuedBytes += len(frame)
	c.mu.Unlock()
	select {
	case c.frames <- frame:
		return nil
	case <-c.done:
		c.releaseFrame(frame)
		return errors.New("Pi RPC reader is closed")
	default:
		c.releaseFrame(frame)
		return errors.New("Pi RPC event backlog exceeds 64 records")
	}
}

func (c *piConnection) releaseFrame(frame json.RawMessage) {
	c.mu.Lock()
	c.queuedBytes -= len(frame)
	c.mu.Unlock()
}
func omitPiMedia(value any) {
	switch v := value.(type) {
	case map[string]any:
		if v["type"] == "image" {
			if body, ok := v["data"].(string); ok {
				v["data"] = "[native image body retained in the owned Pi session file]"
				v["omitted_encoded_bytes"] = len(body)
			}
		}
		for _, child := range v {
			omitPiMedia(child)
		}
	case []any:
		for _, child := range v {
			omitPiMedia(child)
		}
	}
}
func (c *piConnection) send(ctx context.Context, value any, omit bool) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	data, err := json.Marshal(value)
	if err != nil || len(data) > 64<<20 {
		return errors.New("Pi RPC outgoing record exceeds 64 MiB")
	}
	stop := context.AfterFunc(ctx, func() { c.fail(ctx.Err()); c.proc.BeginClose() })
	defer stop()
	c.writeMu.Lock()
	defer c.writeMu.Unlock()
	c.mu.Lock()
	failure := c.failure
	c.mu.Unlock()
	if failure != nil {
		return failure
	}
	logged := string(data)
	if omit {
		logged = "[Pi human response or captured image body omitted]"
	}
	if err := c.journal.write("stdin", logged); err != nil {
		c.fail(err)
		c.proc.BeginClose()
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if _, err := c.proc.Stdin.Write(append(data, '\n')); err != nil {
		failure := errors.Join(ctx.Err(), errors.New("Pi RPC native input failed"))
		c.fail(failure)
		c.proc.BeginClose()
		return failure
	}
	return nil
}
func (c *piConnection) call(ctx context.Context, command string, value map[string]any, result any, omit bool) error {
	return c.callUntil(ctx, ctx, command, value, result, omit)
}

// callUntil writes the command under sendCtx, whose cancellation fails the
// connection as a torn record would, and waits for the reply only while both
// contexts live. A caller outside the run owner passes the run context as
// sendCtx so its own cancellation cannot tear down the execution.
func (c *piConnection) callUntil(sendCtx, waitCtx context.Context, command string, value map[string]any, result any, omit bool) error {
	c.mu.Lock()
	if c.failure != nil {
		err := c.failure
		c.mu.Unlock()
		return err
	}
	if len(c.pending) >= 16 {
		c.mu.Unlock()
		return errors.New("Pi RPC has too many pending commands")
	}
	c.next++
	id := "alva-" + strconv.FormatUint(c.next, 10)
	reply := make(chan piReply, 1)
	c.pending[id] = reply
	c.mu.Unlock()
	defer func() { c.mu.Lock(); delete(c.pending, id); c.mu.Unlock() }()
	if value == nil {
		value = map[string]any{}
	}
	value["type"], value["id"] = command, id
	if err := c.send(sendCtx, value, omit); err != nil {
		return err
	}
	select {
	case response := <-reply:
		if response.err != nil {
			return response.err
		}
		if response.command != command {
			return errors.New("Pi RPC response changed command identity")
		}
		if result != nil && json.Unmarshal(response.data, result) != nil {
			return fmt.Errorf("Pi RPC %s returned invalid data", command)
		}
		return nil
	case <-waitCtx.Done():
		return waitCtx.Err()
	case <-sendCtx.Done():
		return sendCtx.Err()
	}
}
