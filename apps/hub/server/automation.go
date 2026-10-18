// INPUT: Authenticated session-owned strict schedule requests and the shared product coordinator
// OUTPUT: Paused-first optimistic CRUD and explicit account/billing enable actions
// POS: [L7 Hub product] HTTP transport exposes no due-time override, fabricated route, replay or separate scheduler
package server

import (
	"net/http"

	"github.com/QuincySx/Alva/apps/hub"
)

func (s *Server) automations(w http.ResponseWriter, r *http.Request, owner string, parts []string) {
	a := s.service.Automations
	if a == nil {
		fail(w, 503)
		return
	}
	if _, err := query(r); err != nil {
		fail(w, 400)
		return
	}
	if len(parts) == 0 {
		switch r.Method {
		case http.MethodGet:
			value, err := a.Store.ListAutomations(r.Context(), owner)
			reply(w, value, err)
		case http.MethodPost:
			var draft hub.AutomationDraft
			if !decode(w, r, &draft, 24<<10) {
				return
			}
			value, err := a.Store.CreateAutomation(r.Context(), owner, draft)
			if err != nil {
				operationError(w, err)
				return
			}
			respond(w, 201, value)
		default:
			fail(w, 405)
		}
		return
	}
	if len(parts) > 2 || !validID(parts[0]) || (len(parts) == 2 && parts[1] != "enabled") {
		fail(w, 404)
		return
	}
	id := parts[0]
	if len(parts) == 2 {
		if r.Method != http.MethodPut {
			fail(w, 405)
			return
		}
		var value struct {
			Revision                  *uint64 `json:"revision"`
			Enabled                   *bool   `json:"enabled"`
			AccountAndBillingApproved bool    `json:"account_and_billing_approved"`
		}
		if !decode(w, r, &value, 1024) {
			return
		}
		if value.Revision == nil || value.Enabled == nil {
			fail(w, 400)
			return
		}
		result, err := s.service.SetAutomationEnabled(r.Context(), owner, id, *value.Revision, *value.Enabled, value.AccountAndBillingApproved)
		reply(w, result, err)
		return
	}
	switch r.Method {
	case http.MethodGet:
		result, err := a.Store.Automation(r.Context(), owner, id)
		reply(w, result, err)
	case http.MethodPut:
		var value struct {
			Revision *uint64              `json:"revision"`
			Draft    *hub.AutomationDraft `json:"draft"`
		}
		if !decode(w, r, &value, 24<<10) {
			return
		}
		if value.Revision == nil || value.Draft == nil {
			fail(w, 400)
			return
		}
		result, err := a.Store.UpdateAutomation(r.Context(), owner, id, *value.Revision, *value.Draft)
		reply(w, result, err)
	case http.MethodDelete:
		var value struct {
			Revision *uint64 `json:"revision"`
		}
		if !decode(w, r, &value, 1024) {
			return
		}
		if value.Revision == nil {
			fail(w, 400)
			return
		}
		reply(w, map[string]bool{"configuration_deleted_only": true}, a.Store.DeleteAutomation(r.Context(), owner, id, *value.Revision))
	default:
		fail(w, 405)
	}
}
