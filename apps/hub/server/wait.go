// INPUT: Authenticated bounded JSON and the single shared product wait service.
// OUTPUT: Cold wait list/opt-in, exact resolution, explicit continuation and reviewed abandonment HTTP routes.
// POS: [L7 transport] Accepts product branch/Turn/revision, never a host path, native ID or new resume prompt.
package server

import (
	"errors"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/harness/durable"
	"net/http"
)

func (s *Server) waits(w http.ResponseWriter, r *http.Request, id, route string) {
	if _, err := query(r); err != nil {
		fail(w, 400)
		return
	}
	if route == "waits" {
		if r.Method != http.MethodGet {
			fail(w, 405)
			return
		}
		v, err := s.service.Waits(r.Context(), id)
		reply(w, v, err)
		return
	}
	if route == "waits-enabled" {
		if r.Method != http.MethodPut {
			fail(w, 405)
			return
		}
		var v struct {
			Enabled *bool `json:"enabled"`
		}
		if !decode(w, r, &v, 1024) {
			return
		}
		if v.Enabled == nil {
			fail(w, 400)
			return
		}
		reply(w, map[string]bool{"saved": true}, s.service.EnableDurableWaits(r.Context(), id, *v.Enabled))
		return
	}
	if r.Method != http.MethodPost {
		fail(w, 405)
		return
	}
	var err error
	if route == "wait-resolve" {
		var v product.WaitResolution
		if !decode(w, r, &v, 20<<10) {
			return
		}
		err = s.service.ResolveWait(r.Context(), id, v)
	} else {
		var v product.WaitTarget
		if !decode(w, r, &v, 2048) {
			return
		}
		if route == "wait-resume" {
			var turn string
			turn, err = s.service.ResumeWait(r.Context(), id, v)
			if err == nil {
				respond(w, 202, map[string]string{"turn_id": turn, "status": "continuation_admitted"})
				return
			}
		} else {
			err = s.service.AbandonWait(r.Context(), id, v)
		}
	}
	if errors.Is(err, durable.ErrConflict) {
		fail(w, http.StatusConflict)
		return
	}
	reply(w, map[string]bool{"saved": err == nil}, err)
}
