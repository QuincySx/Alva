// INPUT: Owned stdio process, bounded JSON-RPC frames and private redacted journal
// OUTPUT: Bidirectional Codex request correlation (write owned by one context, reply wait bounded by another) and bounded notification delivery
// POS: [integration / native external-engine adapter] One connection owns pending calls; failures never replay a request
package process

import (
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

type codexFrame struct {
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
	Result json.RawMessage `json:"result,omitempty"`
	Error  json.RawMessage `json:"error,omitempty"`
}

type codexReply struct {
	result json.RawMessage
	err    error
}

type codexRPC struct {
	proc     *owned.Process
	journal  *journal
	frames   chan codexFrame
	done     chan struct{}
	readDone chan struct{}
	mu       sync.Mutex
	writeMu  sync.Mutex
	next     uint64
	pending  map[string]chan codexReply
	err      error
	closed   bool
}

func newCodexRPC(proc *owned.Process, j *journal) *codexRPC {
	r := &codexRPC{proc: proc, journal: j, frames: make(chan codexFrame, 64), done: make(chan struct{}), readDone: make(chan struct{}), pending: make(map[string]chan codexReply)}
	go r.read()
	return r
}

func codexRequestID(raw json.RawMessage) (string, error) {
	if len(raw) == 0 || len(raw) > 256 {
		return "", errors.New("invalid Codex request identity")
	}
	if raw[0] == '"' {
		var value string
		if json.Unmarshal(raw, &value) != nil || value == "" || len(value) > 128 || strings.ContainsRune(value, '\x00') {
			return "", errors.New("invalid Codex string request identity")
		}
		return "rpc-s:" + value, nil
	}
	var value uint64
	if json.Unmarshal(raw, &value) != nil {
		return "", errors.New("invalid Codex numeric request identity")
	}
	return "rpc-n:" + strconv.FormatUint(value, 10), nil
}

func (r *codexRPC) fail(err error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return
	}
	r.closed, r.err = true, err
	for id, reply := range r.pending {
		reply <- codexReply{err: err}
		delete(r.pending, id)
	}
	close(r.done)
}

func (r *codexRPC) failure() error {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.err
}

func (r *codexRPC) read() {
	defer close(r.readDone)
	defer close(r.frames)
	err := owned.ReadLines(r.proc.Stdout, 1<<20, func(line owned.StreamLine) error {
		if line.Truncated || !line.Complete || !utf8.Valid(line.Data) {
			return errors.New("Codex stdout frame is incomplete or exceeds 1 MiB")
		}
		var frame codexFrame
		data := strings.TrimSpace(string(line.Data))
		if len(data) == 0 || data[0] != '{' || json.Unmarshal([]byte(data), &frame) != nil {
			if err := r.journal.writeBody("stdout", string(line.Data), true); err != nil {
				return err
			}
			return errors.New("invalid Codex JSON-RPC frame")
		}
		if err := r.journal.writeCodexStdout(frame, string(line.Data)); err != nil {
			return err
		}
		if frame.Method == "" {
			id, err := codexRequestID(frame.ID)
			if err != nil || (len(frame.Result) == 0) == (len(frame.Error) == 0) {
				return errors.New("invalid Codex JSON-RPC response")
			}
			r.mu.Lock()
			reply := r.pending[id]
			delete(r.pending, id)
			r.mu.Unlock()
			// Late or unrecognized response IDs cannot resolve another call.
			if reply != nil {
				result := codexReply{result: frame.Result}
				if len(frame.Error) != 0 {
					result.err = errors.New("Codex app-server rejected a protocol request; inspect the private journal")
					// JSON-RPC -32601 "Method not found": the server does not know the request.
					var rpcError struct {
						Code int `json:"code"`
					}
					if json.Unmarshal(frame.Error, &rpcError) == nil && rpcError.Code == -32601 {
						result.err = errors.Join(result.err, errMethodNotFound)
					}
				}
				reply <- result
			}
			return nil
		}
		if len(frame.Result) != 0 || len(frame.Error) != 0 || len(frame.Method) > 256 {
			return errors.New("invalid Codex JSON-RPC notification/request")
		}
		select {
		case r.frames <- frame:
			return nil
		case <-r.done:
			return r.failure()
		default:
			return errors.New("Codex notification backlog exceeds 64 frames")
		}
	})
	if err == nil {
		err = io.EOF
	}
	r.fail(err)
	if !errors.Is(err, io.EOF) {
		r.proc.BeginClose()
	}
}

func (r *codexRPC) send(ctx context.Context, value any, omitBody bool) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	data, err := json.Marshal(value)
	if err != nil || len(data) > 128<<10 {
		return errors.New("invalid or oversized Codex outgoing frame")
	}
	stop := context.AfterFunc(ctx, func() {
		r.fail(ctx.Err())
		r.proc.BeginClose()
	})
	defer stop()
	r.writeMu.Lock()
	defer r.writeMu.Unlock()
	r.mu.Lock()
	closed, failure := r.closed, r.err
	r.mu.Unlock()
	if closed {
		return failure
	}
	logged := string(data)
	if omitBody {
		logged = "[human input response body omitted]"
	}
	if err = r.journal.write("stdin", logged); err != nil {
		r.fail(err)
		r.proc.BeginClose()
		return err
	}
	if err := ctx.Err(); err != nil {
		r.fail(err)
		r.proc.BeginClose()
		return err
	}
	_, err = r.proc.Stdin.Write(append(data, '\n'))
	if err != nil {
		err = errors.Join(ctx.Err(), errors.New("Codex app-server input failed"))
		r.fail(err)
		r.proc.BeginClose()
	}
	return err
}

func (r *codexRPC) call(ctx context.Context, method string, params any, result any) error {
	return r.callUntil(ctx, ctx, method, params, result)
}

// callUntil writes the request under sendCtx, whose cancellation fails the
// connection as a torn frame would, and waits for the reply only while both
// contexts live. A caller outside the run owner passes the run context as
// sendCtx so its own cancellation cannot tear down the execution.
func (r *codexRPC) callUntil(sendCtx, waitCtx context.Context, method string, params any, result any) error {
	r.mu.Lock()
	if r.closed {
		err := r.err
		r.mu.Unlock()
		return err
	}
	if len(r.pending) >= 32 {
		r.mu.Unlock()
		return errors.New("too many pending Codex calls")
	}
	r.next++
	id := r.next
	key := "rpc-n:" + strconv.FormatUint(id, 10)
	reply := make(chan codexReply, 1)
	r.pending[key] = reply
	r.mu.Unlock()
	defer func() {
		r.mu.Lock()
		delete(r.pending, key)
		r.mu.Unlock()
	}()
	if err := r.send(sendCtx, map[string]any{"id": id, "method": method, "params": params}, false); err != nil {
		return err
	}
	select {
	case response := <-reply:
		if response.err != nil {
			return response.err
		}
		if result != nil && json.Unmarshal(response.result, result) != nil {
			return fmt.Errorf("invalid Codex %s response", method)
		}
		return nil
	case <-waitCtx.Done():
		return waitCtx.Err()
	case <-sendCtx.Done():
		return sendCtx.Err()
	}
}

func (r *codexRPC) reject(ctx context.Context, id json.RawMessage, code int, message string) error {
	return r.send(ctx, map[string]any{"id": id, "error": map[string]any{"code": code, "message": message}}, false)
}
