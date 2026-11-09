// INPUT: Authenticated session routes, TurnService queue/interject/edit/regenerate/select-version and ConversationService versions/fork
// OUTPUT: HTTP operations for the durable input queue, interjection, edit-and-resend, regenerate, version switching and fork
// POS: [L7 Hub product] Bounded transport decoding only: every rule (limits, idle-only versions, attachment refusal, downgrade to the queue) stays in the product services
package server

import (
	"errors"
	"net/http"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
)

// conversationOps serves the running-time and version routes; it reports false
// for a route it does not own.
func (s *Server) conversationOps(w http.ResponseWriter, r *http.Request, id, route string) bool {
	switch route {
	case "queue", "queue-edit", "queue-cancel", "queue-send", "interject", "edit", "regenerate", "versions", "version-select", "fork":
	default:
		return false
	}
	want := http.MethodPost
	if route == "queue" || route == "versions" {
		want = ""
	}
	if want != "" && r.Method != want || want == "" && r.Method != http.MethodGet && r.Method != http.MethodPost {
		fail(w, 405)
		return true
	}
	if r.Method == http.MethodGet && route != "queue" && route != "versions" {
		fail(w, 405)
		return true
	}
	if _, err := query(r); err != nil {
		fail(w, 400)
		return true
	}
	switch route {
	case "queue":
		if r.Method == http.MethodGet {
			items, err := s.service.Turns.Queued(r.Context(), id)
			reply(w, map[string]any{"items": items}, err)
			return true
		}
		var v struct {
			Text string `json:"text"`
		}
		if !decode(w, r, &v, 32<<10) {
			return true
		}
		if hub.ValidPendingText(v.Text) != nil {
			fail(w, 400)
			return true
		}
		item, err := s.service.Turns.Queue(r.Context(), id, v.Text)
		if errors.Is(err, hub.ErrPendingFull) {
			failWith(w, 409, "队列已满（最多 8 条）")
			return true
		}
		if err != nil {
			operationError(w, err)
			return true
		}
		respond(w, 201, item)
	case "queue-edit":
		var v struct {
			InputID string `json:"input_id"`
			Text    string `json:"text"`
		}
		if !decode(w, r, &v, 32<<10) {
			return true
		}
		if !validID(v.InputID) || hub.ValidPendingText(v.Text) != nil {
			fail(w, 400)
			return true
		}
		item, err := s.service.Turns.EditQueued(r.Context(), id, v.InputID, v.Text)
		reply(w, item, err)
	case "queue-cancel", "queue-send":
		var v struct {
			InputID string `json:"input_id"`
		}
		if !decode(w, r, &v, 4<<10) {
			return true
		}
		if v.InputID != "" && !validID(v.InputID) {
			fail(w, 400)
			return true
		}
		if route == "queue-cancel" {
			if v.InputID == "" {
				fail(w, 400)
				return true
			}
			if err := s.service.Turns.CancelQueued(r.Context(), id, v.InputID); err != nil {
				operationError(w, err)
				return true
			}
			respond(w, 200, map[string]string{"status": "cancelled"})
			return true
		}
		start, err := s.service.Turns.SendQueued(r.Context(), id, v.InputID)
		if err != nil {
			operationError(w, err)
			return true
		}
		respond(w, 202, map[string]string{"turn_id": start.TurnID, "execution_id": start.ExecutionID, "status": "admitted"})
	case "interject":
		var v struct {
			ExecutionID string `json:"execution_id"`
			Text        string `json:"text"`
		}
		if !decode(w, r, &v, 32<<10) {
			return true
		}
		if !validID(v.ExecutionID) || hub.ValidPendingText(v.Text) != nil {
			fail(w, 400)
			return true
		}
		result, err := s.service.Turns.Interject(r.Context(), id, v.ExecutionID, v.Text)
		if err != nil {
			operationError(w, err)
			return true
		}
		respond(w, 202, map[string]any{"mode": result.Mode, "notice": result.Notice, "input": result.Input})
	case "edit", "regenerate":
		var v struct {
			TurnID string `json:"turn_id"`
			Prompt string `json:"prompt"`
		}
		if !decode(w, r, &v, 80<<10) {
			return true
		}
		if !validID(v.TurnID) || route == "edit" && hub.ValidTurnText(v.Prompt) != nil {
			fail(w, 400)
			return true
		}
		var start product.TurnStart
		var err error
		if route == "edit" {
			start, err = s.service.Turns.Edit(r.Context(), id, v.TurnID, v.Prompt)
		} else {
			start, err = s.service.Turns.Regenerate(r.Context(), id, v.TurnID)
		}
		if err != nil {
			s.versionError(w, err)
			return true
		}
		respond(w, 202, map[string]string{"turn_id": start.TurnID, "execution_id": start.ExecutionID, "status": "admitted"})
	case "versions":
		if r.Method == http.MethodGet {
			switches, err := s.service.Conversations.Versions(r.Context(), id)
			reply(w, map[string]any{"versions": switches}, err)
			return true
		}
		fail(w, 405)
	case "version-select":
		var v struct {
			LineID string `json:"line_id"`
		}
		if !decode(w, r, &v, 4<<10) {
			return true
		}
		if !validID(v.LineID) {
			fail(w, 400)
			return true
		}
		if err := s.service.Turns.SelectVersion(r.Context(), id, v.LineID); err != nil {
			s.versionError(w, err)
			return true
		}
		respond(w, 200, map[string]string{"status": "selected"})
	case "fork":
		var v struct {
			TurnID string `json:"turn_id"`
		}
		if !decode(w, r, &v, 4<<10) {
			return true
		}
		if !validID(v.TurnID) {
			fail(w, 400)
			return true
		}
		result, err := s.service.Conversations.Fork(r.Context(), id, v.TurnID)
		if err != nil {
			s.versionError(w, err)
			return true
		}
		respond(w, 201, map[string]any{"session": result.Session, "degraded": result.Degraded})
	}
	return true
}

// versionError reports a refused version operation with its product-written
// reason (only the typed refusals carry text; others are plain conflicts).
func (s *Server) versionError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, hub.ErrVersionUnsupported):
		failWith(w, 409, "该轮次不支持编辑、重新生成或分叉（带附件或技能的轮次，以及团队、伙伴、自动化会话除外）")
	case errors.Is(err, hub.ErrVersionBusy), errors.Is(err, hub.ErrSessionBusy), errors.Is(err, hub.ErrSessionRunning):
		failWith(w, 409, "任务进行中，结束后才能编辑、重新生成、切换版本或分叉")
	default:
		operationError(w, err)
	}
}
