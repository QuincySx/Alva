// INPUT: Authenticated claimed branch/typed child path and committed native inspection cursor
// OUTPUT: Bounded lossless Inspector SSE through the independent engine Inspector contract
// POS: [L7 Hub product] Observer disconnect/transport Close detaches this stream without cancelling an engine or exposing client-selected native identities.
package server

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"time"

	"github.com/QuincySx/Alva/apps/hub/product"
)

func (s *Server) inspectionEvents(w http.ResponseWriter, r *http.Request, id string) {
	if r.Method != http.MethodGet {
		fail(w, 405)
		return
	}
	q, err := query(r, "branch", "child_path", "after")
	if err != nil || !validID(q.Get("branch")) {
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
	path, err := childPath(q)
	if err != nil {
		fail(w, 400)
		return
	}
	// Validate the real branch, typed ancestry, cursor and capability before
	// sending successful SSE headers. This page is metadata only.
	probe, err := s.service.InspectorAt(r.Context(), id, q.Get("branch"), path, "", uint64(after), 0)
	if err != nil {
		reply(w, nil, err)
		return
	}
	if !probe.Capabilities.Live {
		fail(w, 409)
		return
	}
	select {
	case s.streams <- struct{}{}:
		defer func() { <-s.streams }()
	default:
		fail(w, 503)
		return
	}
	controller := http.NewResponseController(w)
	if controller.SetWriteDeadline(time.Now().Add(5*time.Second)) != nil {
		fail(w, 503)
		return
	}
	w.Header().Set("Content-Type", "text/event-stream; charset=utf-8")
	w.Header().Set("X-Accel-Buffering", "no")
	if _, err = fmt.Fprintf(w, "event: ready\ndata: {\"after\":%d}\n\n", after); err != nil || controller.Flush() != nil {
		return
	}
	ctx, detach := context.WithCancel(r.Context())
	defer detach()
	go func() {
		select {
		case <-s.done:
			detach()
		case <-ctx.Done():
		}
	}()
	err = s.service.ObserveInspectorAt(ctx, id, q.Get("branch"), path, uint64(after), func(page product.InspectorPage) error {
		data, err := json.Marshal(page)
		if err != nil {
			return err
		}
		if len(data) > 128<<10 || page.Next <= uint64(after) {
			return errors.New("invalid bounded inspection observation")
		}
		if err = controller.SetWriteDeadline(time.Now().Add(5 * time.Second)); err != nil {
			return err
		}
		if _, err = fmt.Fprintf(w, "id: %d\nevent: inspection\ndata: %s\n\n", page.Next, data); err != nil {
			return err
		}
		if err = controller.Flush(); err != nil {
			return err
		}
		after = int64(page.Next)
		return nil
	})
	if err != nil && ctx.Err() == nil {
		_ = controller.SetWriteDeadline(time.Now().Add(5 * time.Second))
		_, _ = fmt.Fprint(w, "event: observation_error\ndata: {\"error\":\"inspection unavailable; reconnect from last committed id\"}\n\n")
		_ = controller.Flush()
	}
}
