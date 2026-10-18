// INPUT: Committed product history, request cancellation and durable sequence cursor
// OUTPUT: Bounded history pages and resumable lossless-order SSE observations
// POS: [L7 Hub product] Observation only; slow consumers and disconnection cannot cancel an admitted Turn
package server

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"
	"time"
)

func (s *Server) history(w http.ResponseWriter, r *http.Request, id string) {
	if r.Method != http.MethodGet {
		fail(w, 405)
		return
	}
	q, err := query(r, "after", "before", "limit")
	if err != nil || q.Has("after") && q.Has("before") {
		fail(w, 400)
		return
	}
	limit, err := integer(q, "limit", 32, 1, 64)
	if err != nil {
		fail(w, 400)
		return
	}
	if q.Has("before") {
		before, err := integer(q, "before", 0, 1, 1<<63-1)
		if err != nil {
			fail(w, 400)
			return
		}
		entries, err := s.service.Store.HistoryBefore(r.Context(), id, uint64(before), int(limit))
		reply(w, entries, err)
		return
	}
	after, err := integer(q, "after", 0, 0, 1<<63-1)
	if err != nil {
		fail(w, 400)
		return
	}
	entries, err := s.service.Store.History(r.Context(), id, uint64(after), int(limit))
	reply(w, entries, err)
}

func (s *Server) events(w http.ResponseWriter, r *http.Request, id string) {
	if r.Method != http.MethodGet {
		fail(w, 405)
		return
	}
	q, err := query(r, "after")
	if err != nil {
		fail(w, 400)
		return
	}
	after, err := integer(q, "after", 0, 0, 1<<63-1)
	if err != nil {
		fail(w, 400)
		return
	}
	last := r.Header.Values("Last-Event-ID")
	if len(last) > 1 || len(last) == 1 && q.Has("after") {
		fail(w, 400)
		return
	}
	if len(last) == 1 {
		after, err = strconv.ParseInt(last[0], 10, 64)
		if err != nil || after < 0 || strconv.FormatInt(after, 10) != last[0] {
			fail(w, 400)
			return
		}
	}
	select {
	case s.streams <- struct{}{}:
		defer func() { <-s.streams }()
	default:
		fail(w, 503)
		return
	}
	controller := http.NewResponseController(w)
	// A transport unable to bound a blocked write cannot offer this endpoint.
	if controller.SetWriteDeadline(time.Now().Add(5*time.Second)) != nil {
		fail(w, 503)
		return
	}
	w.Header().Set("Content-Type", "text/event-stream; charset=utf-8")
	w.Header().Set("X-Accel-Buffering", "no")
	if _, err = fmt.Fprintf(w, "event: ready\ndata: {\"after\":%d}\n\n", after); err != nil {
		return
	}
	if controller.Flush() != nil {
		return
	}
	ticker := time.NewTicker(s.config.PollInterval)
	defer ticker.Stop()
	heartbeat := time.NewTicker(10 * time.Second)
	defer heartbeat.Stop()
	for {
		select {
		case <-s.done:
			return
		default:
		}
		entries, readErr := s.service.Store.History(r.Context(), id, uint64(after), 32)
		if readErr != nil {
			_ = controller.SetWriteDeadline(time.Now().Add(5 * time.Second))
			_, _ = fmt.Fprint(w, "event: observation_error\ndata: {\"error\":\"history unavailable; reconnect from last committed id\"}\n\n")
			_ = controller.Flush()
			return
		}
		for _, entry := range entries {
			// Sequence is the durable authority, not an in-memory subscriber index.
			if entry.Sequence <= uint64(after) {
				return
			}
			data, encodeErr := json.Marshal(entry)
			if encodeErr != nil || len(data) > 256<<10 {
				return
			}
			if controller.SetWriteDeadline(time.Now().Add(5*time.Second)) != nil {
				return
			}
			if _, err = fmt.Fprintf(w, "id: %d\nevent: history\ndata: %s\n\n", entry.Sequence, data); err != nil {
				return
			}
			if controller.Flush() != nil {
				return
			}
			after = int64(entry.Sequence)
		}
		if len(entries) == 32 {
			select {
			case <-r.Context().Done():
				return
			default:
			}
			continue
		}
		select {
		case <-s.done:
			return
		case <-r.Context().Done():
			return
		case <-ticker.C:
		case <-heartbeat.C:
			if err = controller.SetWriteDeadline(time.Now().Add(5 * time.Second)); err != nil {
				return
			}
			if _, err = fmt.Fprint(w, ": heartbeat\n\n"); err != nil {
				return
			}
			if controller.Flush() != nil {
				return
			}
		}
	}
}
