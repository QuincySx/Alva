// INPUT: Validated per-request Exchange, explicit upstream RawTransport and downstream HTTP writer
// OUTPUT: Backpressured native SSE without premature DONE or successful completion on truncated upstream
// POS: [L6 application] Gateway transport framing owns write limits while codec owns protocol terminal and loss validation
package gateway

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/QuincySx/Alva/go/integration/llm/codec"
)

func (s *Server) stream(w http.ResponseWriter, r *http.Request, inbound codec.Protocol, alias string, route modelRoute, exchange *codec.Exchange, body json.RawMessage) {
	stream, err := exchange.NewResponseStream()
	if err != nil {
		writeError(w, inbound, 400)
		return
	}
	started := false
	written := 0
	emit := func(event codec.Event) error {
		if strings.ContainsAny(event.Name, "\r\n\x00") || len(event.Name) > 128 {
			return errors.New("invalid bounded SSE event name")
		}
		// SSE treats CR, CRLF and LF as line endings. Prefix every data line
		// and account for the actual encoded bytes, not an estimated overhead.
		data := strings.ReplaceAll(string(event.Data), "\r\n", "\n")
		data = strings.ReplaceAll(data, "\r", "\n")
		frameBytes := len(data) + 6*(strings.Count(data, "\n")+1) + 2
		if event.Name != "" {
			frameBytes += len("event: \n") + len(event.Name)
		}
		if written+frameBytes > 8<<20 {
			return errors.New("gateway output stream exceeds 8 MiB")
		}
		if err := r.Context().Err(); err != nil {
			return err
		}
		if err := http.NewResponseController(w).SetWriteDeadline(time.Now().Add(30 * time.Second)); err != nil && !errors.Is(err, http.ErrNotSupported) {
			return err
		}
		if !started {
			w.Header().Set("Content-Type", "text/event-stream")
			w.Header().Set("X-Accel-Buffering", "no")
			w.WriteHeader(200)
			started = true
		}
		if event.Name != "" {
			if _, err := io.WriteString(w, "event: "+event.Name+"\n"); err != nil {
				return err
			}
		}
		for _, line := range strings.Split(data, "\n") {
			if _, err := io.WriteString(w, "data: "+line+"\n"); err != nil {
				return err
			}
		}
		if _, err := io.WriteString(w, "\n"); err != nil {
			return err
		}
		written += frameBytes
		return http.NewResponseController(w).Flush()
	}
	err = route.transport.StreamRaw(r.Context(), body, func(event codec.Event) (bool, error) {
		if err := stream.Push(r.Context(), event, emit); err != nil {
			return false, err
		}
		// Gemini's STOP candidate may precede legal usage-only frames. Unlike
		// Chat [DONE], Responses completed or Anthropic message_stop, STOP is
		// not a transport terminator; retain the tail and any read failure.
		return route.protocol != codec.Gemini && stream.Terminal(), nil
	})
	if err == nil {
		err = stream.Finish(r.Context(), emit)
	}
	if err == nil {
		return
	}
	if !started {
		writeError(w, inbound, 502)
		return
	}
	// No DONE/stop/completed follows this error. Previously emitted partial
	// data is not relabeled a successful generation or automatically replayed.
	data, _ := json.Marshal(errorValue(inbound, 502))
	name := "error"
	if inbound == codec.OpenAIChat || inbound == codec.Gemini {
		name = ""
	}
	_ = emit(codec.Event{Name: name, Data: data})
}

func errorValue(protocol codec.Protocol, status int) any {
	message := http.StatusText(status)
	switch protocol {
	case codec.Anthropic:
		return map[string]any{"type": "error", "error": map[string]string{"type": "api_error", "message": message}}
	case codec.Gemini:
		name := "INTERNAL"
		switch status {
		case 400, 405, 415:
			name = "INVALID_ARGUMENT"
		case 401:
			name = "UNAUTHENTICATED"
		case 403:
			name = "PERMISSION_DENIED"
		case 404:
			name = "NOT_FOUND"
		case 413, 429:
			name = "RESOURCE_EXHAUSTED"
		case 502, 503:
			name = "UNAVAILABLE"
		}
		return map[string]any{"error": map[string]any{"code": status, "message": message, "status": name}}
	case codec.OpenAIResponses:
		return map[string]any{"type": "error", "code": "gateway_error", "message": message, "error": map[string]string{"message": message, "type": "gateway_error"}}
	default:
		return map[string]any{"error": map[string]string{"message": message, "type": "gateway_error"}}
	}
}
func writeJSON(w http.ResponseWriter, status int, value any) {
	data, err := json.Marshal(value)
	if err != nil {
		data = []byte(`{"error":{"message":"Internal Server Error"}}`)
		status = 500
	}
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_, _ = w.Write(append(data, '\n'))
}
func writeError(w http.ResponseWriter, protocol codec.Protocol, status int) {
	writeJSON(w, status, errorValue(protocol, status))
}
