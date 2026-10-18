// INPUT: Authenticated product session, committed branch IDs and bounded opaque job requests
// OUTPUT: Cold shell job list, owned Stop and explicit idle session-owner reset
// POS: [L7 Hub product] Existing process ownership only; no new shell execution, PID input or arbitrary native ID.
package server

import "net/http"

func (s *Server) shellJobs(w http.ResponseWriter, r *http.Request, id string) {
	if _, err := query(r); err != nil {
		fail(w, 400)
		return
	}
	if r.Method == http.MethodGet {
		jobs, err := s.service.ShellJobs(r.Context(), id)
		reply(w, jobs, err)
		return
	}
	if r.Method != http.MethodPost {
		fail(w, 405)
		return
	}
	var value struct {
		Action   string `json:"action"`
		Branch   string `json:"branch"`
		Job      string `json:"job,omitempty"`
		Approved bool   `json:"approved,omitempty"`
	}
	if !decode(w, r, &value, 2048) {
		return
	}
	if !validID(value.Branch) {
		fail(w, 400)
		return
	}
	var err error
	switch value.Action {
	case "stop":
		if !validID(value.Job) {
			fail(w, 400)
			return
		}
		err = s.service.StopShellJob(r.Context(), id, value.Branch, value.Job)
	case "reset":
		if !value.Approved || value.Job != "" {
			fail(w, 400)
			return
		}
		err = s.service.ResetShellOwner(r.Context(), id, value.Branch)
	default:
		fail(w, 400)
		return
	}
	reply(w, map[string]any{"cleanup_complete": err == nil}, err)
}
