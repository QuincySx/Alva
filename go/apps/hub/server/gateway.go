// INPUT: Authenticated application request and operator-frozen local Gateway route
// OUTPUT: Explicit cold status/start/stop with one-start-only bearer response
// POS: [L7 Hub product] Transport exposes no provider config/key import, background start or Agent execution.
package server

import "net/http"

func (s *Server) localGateway(w http.ResponseWriter, r *http.Request) {
	if s.service.Gateway == nil {
		fail(w, 503)
		return
	}
	if _, err := query(r); err != nil {
		fail(w, 400)
		return
	}
	switch r.Method {
	case http.MethodGet:
		respond(w, 200, s.service.Gateway.Snapshot())
	case http.MethodPost:
		var value struct {
			ModelUsageApproved bool `json:"model_usage_approved"`
		}
		if !decode(w, r, &value, 1024) {
			return
		}
		credentials, err := s.service.Gateway.Start(r.Context(), value.ModelUsageApproved)
		reply(w, credentials, err)
	case http.MethodDelete:
		reply(w, map[string]bool{"stopped": true}, s.service.Gateway.Stop(r.Context()))
	default:
		fail(w, 405)
	}
}
