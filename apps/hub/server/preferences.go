// INPUT: Authenticated application-scoped HTTP and the shared preference service.
// OUTPUT: GET/PATCH preferences with strict JSON and optimistic conflict responses.
// POS: [L7 transport] Reuses product policy and never opens SQLite or edits session configuration.
package server

import (
	"errors"
	"net/http"

	"github.com/QuincySx/Alva/apps/hub"
)

func (s *Server) preferences(w http.ResponseWriter, r *http.Request) {
	if _, err := query(r); err != nil {
		fail(w, 400)
		return
	}
	owner := s.service.Preferences
	if owner == nil {
		fail(w, 503)
		return
	}
	var value hub.Preferences
	var err error
	switch r.Method {
	case http.MethodGet:
		value, err = owner.Read(r.Context())
	case http.MethodPatch:
		var update hub.PreferenceUpdate
		if !decode(w, r, &update, 4<<10) {
			return
		}
		value, err = owner.Update(r.Context(), update)
	default:
		w.Header().Set("Allow", "GET, PATCH")
		fail(w, 405)
		return
	}
	if err != nil {
		switch {
		case errors.Is(err, hub.ErrPreferenceConflict):
			fail(w, 409)
		case errors.Is(err, hub.ErrInvalidPreferences):
			fail(w, 400)
		default:
			fail(w, 500)
		}
		return
	}
	respond(w, 200, value)
}
