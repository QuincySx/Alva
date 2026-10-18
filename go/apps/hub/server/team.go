// INPUT: Authenticated Lead routes and the shared durable Team product service
// OUTPUT: Bounded Team discovery, Worker queue/lifecycle and explicit owned dispatch HTTP operations
// POS: [L7 Hub product] Transport-only collaboration facade with independent account approval and no observation replay
package server

import (
	"net/http"

	"github.com/QuincySx/Alva/go/apps/hub"
)

func (s *Server) team(w http.ResponseWriter, r *http.Request, lead string) {
	if s.service.Teams == nil {
		fail(w, 503)
		return
	}
	if _, err := query(r); err != nil {
		fail(w, 400)
		return
	}
	switch r.Method {
	case http.MethodGet:
		value, err := s.service.Teams.Store.TeamForLead(r.Context(), lead)
		reply(w, value, err)
	case http.MethodPost:
		if !emptyPost(w, r) {
			return
		}
		value, err := s.service.Teams.Store.CreateTeam(r.Context(), lead)
		if err != nil {
			operationError(w, err)
			return
		}
		respond(w, 201, value)
	default:
		fail(w, 405)
	}
}

func (s *Server) teamResource(w http.ResponseWriter, r *http.Request, lead string, parts []string) {
	if s.service.Teams == nil {
		fail(w, 503)
		return
	}
	team := parts[0]
	// Validate ownership even for read-only status and malformed subresources.
	// A worker ID must never select or start a different Lead's Team.
	detail, err := s.service.Teams.Store.Team(r.Context(), lead, team)
	if err != nil {
		operationError(w, err)
		return
	}
	if len(parts) == 1 {
		if r.Method != http.MethodGet {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		respond(w, 200, detail)
		return
	}
	if len(parts) == 2 && parts[1] == "end" {
		if !emptyPost(w, r) {
			return
		}
		reply(w, map[string]bool{"saved": true}, s.service.Teams.Store.EndTeam(r.Context(), lead, team))
		return
	}
	if parts[1] != "workers" {
		fail(w, 404)
		return
	}
	if len(parts) == 2 {
		if r.Method != http.MethodPost {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		var v struct {
			Name                      string     `json:"name"`
			Target                    hub.Target `json:"target"`
			AccountAndBillingApproved bool       `json:"account_and_billing_approved"`
		}
		if !decode(w, r, &v, 16<<10) {
			return
		}
		if !s.targetAllowed(v.Target) {
			fail(w, 400)
			return
		}
		value, err := s.service.AddWorker(r.Context(), lead, team, v.Name, v.Target, v.AccountAndBillingApproved)
		if err != nil {
			operationError(w, err)
			return
		}
		respond(w, 201, value)
		return
	}
	worker := parts[2]
	owned := false
	for _, candidate := range detail.Workers {
		owned = owned || candidate.ID == worker
	}
	if !validID(worker) || !owned {
		fail(w, 404)
		return
	}
	if len(parts) == 5 && parts[3] == "worktree" {
		s.workerWorktree(w, r, lead, team, worker, parts[4])
		return
	}
	if len(parts) == 4 {
		switch parts[3] {
		case "resume", "stop":
			if !emptyPost(w, r) {
				return
			}
			if parts[3] == "resume" {
				err = s.service.Teams.Resume(r.Context(), lead, team, worker)
			} else {
				err = s.service.Teams.Stop(r.Context(), lead, team, worker)
			}
			if err != nil {
				operationError(w, err)
				return
			}
			respond(w, 202, map[string]string{"status": "admitted; observe durable task outcome"})
			return
		case "status":
			if r.Method != http.MethodGet {
				fail(w, 405)
				return
			}
			if _, err := query(r); err != nil {
				fail(w, 400)
				return
			}
			status := s.service.Teams.Status(worker)
			// Raw runtime errors may contain host paths or private provider data.
			// Durable task/native facts remain the explicit Inspector evidence.
			respond(w, 200, map[string]bool{"active": status.Active, "has_error": status.Error != ""})
			return
		case "lifecycle":
			if r.Method != http.MethodPut {
				fail(w, 405)
				return
			}
			if _, err := query(r); err != nil {
				fail(w, 400)
				return
			}
			var v struct {
				State hub.WorkerLifecycle `json:"state"`
			}
			if !decode(w, r, &v, 1024) {
				return
			}
			reply(w, map[string]bool{"saved": true}, s.service.Teams.Store.SetWorkerLifecycle(r.Context(), lead, team, worker, v.State))
			return
		case "tasks":
			s.workerTasks(w, r, lead, team, worker, "")
			return
		}
	}
	if len(parts) == 5 && parts[3] == "tasks" && validID(parts[4]) {
		s.workerTasks(w, r, lead, team, worker, parts[4])
		return
	}
	fail(w, 404)
}

func (s *Server) workerTasks(w http.ResponseWriter, r *http.Request, lead, team, worker, task string) {
	if r.Method == http.MethodGet && task == "" {
		q, err := query(r, "after", "limit")
		if err != nil {
			fail(w, 400)
			return
		}
		after, err := integer(q, "after", 0, 0, 1<<63-1)
		if err != nil {
			fail(w, 400)
			return
		}
		limit, err := integer(q, "limit", 8, 1, 32)
		if err != nil {
			fail(w, 400)
			return
		}
		value, err := s.service.Teams.Store.WorkerTasks(r.Context(), lead, team, worker, uint64(after), int(limit))
		reply(w, value, err)
		return
	}
	if _, err := query(r); err != nil {
		fail(w, 400)
		return
	}
	if r.Method == http.MethodDelete && task != "" {
		reply(w, map[string]bool{"saved": true}, s.service.Teams.Store.CancelQueuedTask(r.Context(), lead, team, worker, task))
		return
	}
	if (r.Method != http.MethodPost || task != "") && (r.Method != http.MethodPut || task == "") {
		fail(w, 405)
		return
	}
	var v struct {
		Scope                   string              `json:"scope"`
		Claims                  []hub.ResourceClaim `json:"claims"`
		IncludeDraftAttachments bool                `json:"include_draft_attachments,omitempty"`
	}
	if !decode(w, r, &v, 32<<10) {
		return
	}
	if task != "" {
		if v.IncludeDraftAttachments {
			fail(w, 400)
			return
		}
		reply(w, map[string]bool{"saved": true}, s.service.Teams.Store.EditQueuedTask(r.Context(), lead, team, worker, task, v.Scope, v.Claims))
		return
	}
	if v.IncludeDraftAttachments {
		value, err := s.service.EnqueueWorkerDraft(r.Context(), lead, team, worker, v.Scope, v.Claims)
		if err != nil {
			operationError(w, err)
			return
		}
		respond(w, 201, value)
		return
	}
	value, err := s.service.Teams.Store.EnqueueWorker(r.Context(), lead, team, worker, v.Scope, v.Claims)
	if err != nil {
		operationError(w, err)
		return
	}
	respond(w, 201, value)
}
