// INPUT: Authenticated owned Lead/Team/Worker, operator-approved Git and exact saved evidence/revision
// OUTPUT: Shared product provision/preview/patch/adopt HTTP operations without client host paths
// POS: [L7 Hub product] Explicit filesystem changes use the same execution owner and review evidence as native UI.
package server

import "net/http"

func (s *Server) workerWorktree(w http.ResponseWriter, r *http.Request, lead, team, worker, action string) {
	if action == "patch" {
		if r.Method != http.MethodGet {
			fail(w, 405)
			return
		}
		q, err := query(r, "evidence", "offset")
		if err != nil || !validID(q.Get("evidence")) {
			fail(w, 400)
			return
		}
		offset, err := integer(q, "offset", 0, 0, 16<<20)
		if err != nil {
			fail(w, 400)
			return
		}
		value, err := s.service.WorkerPatch(r.Context(), lead, team, worker, q.Get("evidence"), int(offset))
		reply(w, value, err)
		return
	}
	if action == "preview" || action == "review-removal" {
		if !emptyPost(w, r) {
			return
		}
		if action == "review-removal" {
			value, err := s.service.ReviewWorkerRemoval(r.Context(), lead, team, worker)
			reply(w, value, err)
			return
		}
		value, err := s.service.PreviewWorkerChanges(r.Context(), lead, team, worker)
		reply(w, value, err)
		return
	}
	if action != "provision" && action != "adopt" && action != "remove" {
		fail(w, 404)
		return
	}
	if r.Method != http.MethodPost {
		fail(w, 405)
		return
	}
	if _, err := query(r); err != nil {
		fail(w, 400)
		return
	}
	if action == "provision" {
		var value struct {
			Approved bool `json:"approved"`
		}
		if !decode(w, r, &value, 1024) {
			return
		}
		result, err := s.service.ProvisionWorkerWorktree(r.Context(), lead, team, worker, value.Approved)
		reply(w, result, err)
		return
	}
	if action == "remove" {
		var value struct {
			Approved bool   `json:"approved"`
			Revision uint64 `json:"revision"`
			Snapshot string `json:"snapshot"`
		}
		if !decode(w, r, &value, 2048) {
			return
		}
		if len(value.Snapshot) != 64 || value.Revision == 0 {
			fail(w, 400)
			return
		}
		result, err := s.service.RemoveWorkerWorktree(r.Context(), lead, team, worker, value.Revision, value.Snapshot, value.Approved)
		reply(w, result, err)
		return
	}
	var value struct {
		Approved bool   `json:"approved"`
		Evidence string `json:"evidence"`
		Revision uint64 `json:"revision"`
	}
	if !decode(w, r, &value, 2048) {
		return
	}
	if !validID(value.Evidence) || value.Revision == 0 {
		fail(w, 400)
		return
	}
	result, err := s.service.AdoptWorkerChanges(r.Context(), lead, team, worker, value.Evidence, value.Revision, value.Approved)
	reply(w, result, err)
}
