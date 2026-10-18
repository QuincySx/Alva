// INPUT: Authenticated product identity, pre-saved tool configuration and actual pending question ID
// OUTPUT: Explicit next-turn enablement and real human answer HTTP transport
// POS: [L7 Hub product] No arbitrary executable/path import and no question response grants permission.
package server

import (
	"github.com/QuincySx/Alva/go/extension/interaction"
	"net/http"
)

func (s *Server) toolSetting(w http.ResponseWriter, r *http.Request, id, kind string) {
	if r.Method != http.MethodPut {
		fail(w, 405)
		return
	}
	if _, err := query(r); err != nil {
		fail(w, 400)
		return
	}
	var value struct {
		Enabled        *bool `json:"enabled"`
		AccessApproved bool  `json:"access_approved"`
	}
	if !decode(w, r, &value, 1024) {
		return
	}
	if value.Enabled == nil {
		fail(w, 400)
		return
	}
	reply(w, map[string]bool{"saved": true}, s.service.SetToolEnabled(r.Context(), id, kind, *value.Enabled, value.AccessApproved))
}
func (s *Server) answerQuestion(w http.ResponseWriter, r *http.Request, id, question string) {
	if r.Method != http.MethodPost {
		fail(w, 405)
		return
	}
	if _, err := query(r); err != nil {
		fail(w, 400)
		return
	}
	var answer interaction.Answer
	if !decode(w, r, &answer, 128<<10) {
		return
	}
	reply(w, map[string]bool{"accepted": true}, s.service.RespondQuestion(r.Context(), id, question, answer))
}
