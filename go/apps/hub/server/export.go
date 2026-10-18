// INPUT: Authenticated selected product branch/typed path and explicit fixed snapshot/child scope
// OUTPUT: Bounded inspector-bundle JSONL download without host file writes or runtime restoration
// POS: [L7 Hub product] Native and HTTP export use the same exact product authorization and record encoder.
package server

import (
	"net/http"
	"strconv"
)

func (s *Server) inspectorBundle(w http.ResponseWriter, r *http.Request, id string) {
	if r.Method != http.MethodGet {
		fail(w, 405)
		return
	}
	q, err := query(r, "branch", "high_water", "child_path", "include_children")
	if err != nil || !validID(q.Get("branch")) {
		fail(w, 400)
		return
	}
	children := false
	if q.Has("include_children") {
		switch q.Get("include_children") {
		case "true":
			children = true
		case "false":
		default:
			fail(w, 400)
			return
		}
	}
	high, err := integer(q, "high_water", 0, 0, 1<<63-1)
	if err != nil {
		fail(w, 400)
		return
	}
	path, err := childPath(q)
	if err != nil {
		fail(w, 400)
		return
	}
	select {
	case s.exports <- struct{}{}:
		defer func() { <-s.exports }()
	default:
		fail(w, 503)
		return
	}
	data, summary, err := s.service.InspectorBundleAt(r.Context(), id, q.Get("branch"), path, uint64(high), children)
	if err != nil {
		operationError(w, err)
		return
	}
	w.Header().Set("Content-Type", "application/x-ndjson")
	w.Header().Set("Content-Disposition", `attachment; filename="alva-inspector.jsonl"`)
	w.Header().Set("Content-Length", strconv.Itoa(len(data)))
	w.Header().Set("X-Content-SHA256", summary.SHA256)
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(data)
}
