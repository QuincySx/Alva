// INPUT: Authenticated session routes and the shared Hub product service
// OUTPUT: Session turns, routing, approvals, files, settings, Skills and Inspector HTTP operations
// POS: [L7 Hub product] Bounded transport decoding and ownership resolution without reimplementing product policy
package server

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"strconv"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
)

func (s *Server) sessionRoute(w http.ResponseWriter, r *http.Request, id, route string) {
	if route == "automations" {
		s.automations(w, r, id, nil)
		return
	}
	switch route {
	case "waits", "waits-enabled", "wait-resolve", "wait-resume", "wait-abandon":
		s.waits(w, r, id, route)
	case "inspector-bundle":
		s.inspectorBundle(w, r, id)
	case "shell-jobs":
		s.shellJobs(w, r, id)
	case "turns":
		if r.Method != http.MethodPost {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		var v struct {
			Prompt     string                            `json:"prompt"`
			References []product.DraftReferenceSelection `json:"references"`
		}
		if !decode(w, r, &v, 64<<10) {
			return
		}
		if strings.TrimSpace(v.Prompt) == "" || len(v.Prompt) > 32<<10 {
			fail(w, 400)
			return
		}
		start, err := s.service.Turns.Send(r.Context(), id, v.Prompt, v.References)
		if err != nil {
			operationError(w, err)
			return
		}
		respond(w, 202, map[string]string{"turn_id": start.TurnID, "execution_id": start.ExecutionID, "status": "admitted"})
	case "continue":
		if r.Method != http.MethodPost {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		start, err := s.service.Turns.Continue(r.Context(), id)
		if err != nil {
			operationError(w, err)
			return
		}
		respond(w, 202, map[string]string{"turn_id": start.TurnID, "execution_id": start.ExecutionID, "status": "admitted"})
	case "execution":
		if r.Method != http.MethodGet {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		executionID, active := s.service.Turns.Active(id)
		respond(w, 200, map[string]any{"execution_id": executionID, "active": active})
	case "stop":
		// Stop names the execution the client observed (turns or execution
		// response), so a late request never cancels a replacement owner.
		if r.Method != http.MethodPost {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		var v struct {
			ExecutionID string `json:"execution_id"`
		}
		if !decode(w, r, &v, 4<<10) {
			return
		}
		if !validID(v.ExecutionID) {
			fail(w, 400)
			return
		}
		err := s.service.Turns.Stop(r.Context(), id, v.ExecutionID)
		if err != nil {
			operationError(w, err)
			return
		}
		// This acknowledges a cancel request, not a fabricated terminal outcome.
		respond(w, 202, map[string]string{"status": "cancel_requested"})
	case "history":
		s.history(w, r, id)
	case "events":
		s.events(w, r, id)
	case "branch":
		if r.Method != http.MethodPost {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		var v struct {
			Target hub.Target `json:"target"`
			Reason string     `json:"reason"`
		}
		if !decode(w, r, &v, 64<<10) {
			return
		}
		if !s.targetAllowed(v.Target) {
			fail(w, 400)
			return
		}
		branch, err := s.service.SwitchTarget(r.Context(), id, v.Target, v.Reason)
		reply(w, branch, err)
	case "fallback":
		if r.Method != http.MethodPut {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		var v struct {
			Targets []hub.Target `json:"targets"`
		}
		if !decode(w, r, &v, 64<<10) {
			return
		}
		for _, target := range v.Targets {
			if !s.targetAllowed(target) {
				fail(w, 400)
				return
			}
		}
		reply(w, map[string]bool{"saved": true}, s.service.SetFallback(r.Context(), id, v.Targets))
	case "approvals":
		if r.Method != http.MethodGet {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		value, err := s.service.PendingApprovals(r.Context(), id)
		reply(w, value, err)
	case "attachments":
		s.attachments(w, r, id)
	case "questions":
		if r.Method != http.MethodGet {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		value, err := s.service.PendingQuestions(r.Context(), id)
		reply(w, value, err)
	case "inspector-events":
		s.inspectionEvents(w, r, id)
	case "inspector":
		if r.Method != http.MethodGet {
			fail(w, 405)
			return
		}
		q, err := query(r, "branch", "q", "after", "before", "high_water", "child_path")
		if err != nil || !validID(q.Get("branch")) || len(q.Get("q")) > 256 || q.Has("before") && (q.Has("after") || q.Get("q") != "") {
			fail(w, 400)
			return
		}
		after, err := integer(q, "after", 0, 0, 1<<63-1)
		if err != nil {
			fail(w, 400)
			return
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
		if q.Has("before") {
			// before=0 reads backward from the snapshot's end.
			before, err := integer(q, "before", 0, 0, 1<<63-1)
			if err != nil {
				fail(w, 400)
				return
			}
			value, err := s.service.InspectorBeforeAt(r.Context(), id, q.Get("branch"), path, uint64(before), uint64(high))
			reply(w, value, err)
			return
		}
		value, err := s.service.InspectorAt(r.Context(), id, q.Get("branch"), path, q.Get("q"), uint64(after), uint64(high))
		reply(w, value, err)
	case "artifact":
		if r.Method != http.MethodGet {
			fail(w, 405)
			return
		}
		q, err := query(r, "branch", "uri", "offset", "child_path")
		if err != nil || !validID(q.Get("branch")) || len(q.Get("uri")) > 2048 {
			fail(w, 400)
			return
		}
		offset, err := integer(q, "offset", 0, 0, 1<<63-1)
		if err != nil {
			fail(w, 400)
			return
		}
		path, err := childPath(q)
		if err != nil {
			fail(w, 400)
			return
		}
		value, err := s.service.InspectorReadAt(r.Context(), id, q.Get("branch"), path, q.Get("uri"), offset)
		reply(w, value, err)
	case "artifact-bytes":
		s.artifactBytes(w, r, id)
	case "settings":
		if r.Method != http.MethodGet {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		value, err := s.service.SessionSettings(r.Context(), id)
		reply(w, value, err)
	case "skills":
		if r.Method != http.MethodGet {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		value, err := s.service.Skills(r.Context(), id)
		reply(w, value, err)
	case "team":
		s.team(w, r, id)
	default:
		fail(w, 404)
	}
}

func (s *Server) nested(w http.ResponseWriter, r *http.Request, id string, parts []string) {
	if parts[0] == "automations" {
		s.automations(w, r, id, parts[1:])
		return
	}
	if len(parts) == 2 && parts[0] == "handoffs" && validID(parts[1]) {
		if r.Method != http.MethodGet {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		value, err := s.service.Conversations.Handoff(r.Context(), id, parts[1])
		reply(w, value, err)
		return
	}
	if len(parts) == 2 && parts[0] == "media" {
		s.media(w, r, id, parts[1])
		return
	}
	if len(parts) >= 2 && parts[0] == "teams" && validID(parts[1]) {
		s.teamResource(w, r, id, parts[1:])
		return
	}
	if len(parts) == 3 && parts[0] == "questions" && validID(parts[1]) && parts[2] == "answer" {
		s.answerQuestion(w, r, id, parts[1])
		return
	}
	if len(parts) == 3 && parts[0] == "settings" && parts[1] == "tools" {
		s.toolSetting(w, r, id, parts[2])
		return
	}
	if len(parts) == 3 && parts[0] == "approvals" && validID(parts[1]) && parts[2] == "decision" {
		if r.Method != http.MethodPost {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		var v struct {
			Allow *bool `json:"allow"`
		}
		if !decode(w, r, &v, 1024) {
			return
		}
		if v.Allow == nil {
			fail(w, 400)
			return
		}
		reply(w, map[string]bool{"accepted": true}, s.service.RespondApproval(r.Context(), id, parts[1], *v.Allow))
		return
	}
	if len(parts) == 2 && parts[0] == "attachments" && validID(parts[1]) {
		if r.Method != http.MethodDelete {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		s.service.RemoveDraftAttachment(id, parts[1])
		respond(w, 200, map[string]bool{"removed": true})
		return
	}
	if len(parts) == 2 && parts[0] == "settings" {
		switch parts[1] {
		case "compatibility":
			if r.Method != http.MethodPut {
				fail(w, 405)
				return
			}
			if _, err := query(r); err != nil {
				fail(w, 400)
				return
			}
			var v struct {
				File *string `json:"file"`
			}
			if !decode(w, r, &v, 2048) {
				return
			}
			if v.File == nil {
				fail(w, 400)
				return
			}
			reply(w, map[string]bool{"saved": true}, s.service.SetCompatibilityFile(r.Context(), id, *v.File))
			return
		case "mcp":
			if r.Method != http.MethodPut {
				fail(w, 405)
				return
			}
			if _, err := query(r); err != nil {
				fail(w, 400)
				return
			}
			var v struct {
				Servers []hub.MCPServer `json:"servers"`
			}
			if !decode(w, r, &v, 64<<10) {
				return
			}
			data, err := json.Marshal(v.Servers)
			if err != nil {
				fail(w, 400)
				return
			}
			reply(w, map[string]bool{"saved_disabled": true}, s.service.SaveMCPDraft(r.Context(), id, string(data)))
			return
		case "skills":
			// Skills are session-scoped and never inherited from application settings.
			s.settingSwitch(w, r, id, "skills", "")
			return
		case "subagents":
			s.settingSwitch(w, r, id, hub.CapabilitySubagents, "")
			return
		case "memory":
			s.settingSwitch(w, r, id, hub.CapabilityMemory, "")
			return
		case "media":
			s.settingSwitch(w, r, id, hub.CapabilityMedia, "")
			return
		case "media-decoder":
			s.settingSwitch(w, r, id, hub.CapabilityDecoder, "")
			return
		case "browser":
			s.settingSwitch(w, r, id, hub.CapabilityBrowser, "")
			return
		}
	}
	if len(parts) == 4 && parts[0] == "settings" && parts[1] == "commands" && (parts[2] == "hook" || parts[2] == "diagnostic") && validID(parts[3]) {
		s.settingSwitch(w, r, id, parts[2], parts[3])
		return
	}
	if len(parts) == 2 && parts[0] == "settings" && parts[1] == "app" {
		// Read-only application-level settings as the redacted form model: saved
		// secrets are placeholders, each item carries the fingerprint an approval
		// must quote.
		if r.Method != http.MethodGet {
			fail(w, 405)
			return
		}
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		if s.service.Settings == nil {
			fail(w, 404)
			return
		}
		view, err := s.service.Settings.Load(r.Context(), product.SettingsScope{App: true, SessionID: id})
		reply(w, view, err)
		return
	}
	if len(parts) == 3 && parts[0] == "settings" && (parts[1] == "mcp" || parts[1] == "aep") && validID(parts[2]) {
		// AEP toggles only an operator/native-inspected immutable pin. HTTP has no
		// source-directory import or arbitrary host file staging endpoint.
		s.settingSwitch(w, r, id, parts[1], parts[2])
		return
	}
	if len(parts) == 2 && parts[0] == "skills" {
		switch parts[1] {
		case "initialize":
			if !emptyPost(w, r) {
				return
			}
			reply(w, map[string]bool{"initialized": true}, s.service.InitializeSkills(r.Context(), id))
			return
		case "document":
			if r.Method != http.MethodGet {
				fail(w, 405)
				return
			}
			q, err := query(r, "name")
			if err != nil || !validID(q.Get("name")) {
				fail(w, 400)
				return
			}
			value, err := s.service.SkillDocument(r.Context(), id, q.Get("name"))
			reply(w, value, err)
			return
		case "install":
			if r.Method != http.MethodPost {
				fail(w, 405)
				return
			}
			if _, err := query(r); err != nil {
				fail(w, 400)
				return
			}
			var v struct {
				Document string `json:"document"`
			}
			if !decode(w, r, &v, 2<<20) {
				return
			}
			reply(w, map[string]bool{"installed": true}, s.service.InstallSkill(r.Context(), id, v.Document))
			return
		case "archive":
			if r.Method != http.MethodPost {
				fail(w, 405)
				return
			}
			if _, err := query(r); err != nil {
				fail(w, 400)
				return
			}
			var v struct {
				Name string `json:"name"`
			}
			if !decode(w, r, &v, 1024) {
				return
			}
			receipt, err := s.service.ArchiveSkill(r.Context(), id, v.Name)
			// Preserve the recovery receipt even when recording it failed. A caller
			// must never retry the filesystem mutation blindly after partial failure.
			if err != nil {
				respond(w, 409, map[string]string{"error": "archive not fully committed; do not retry automatically", "receipt": receipt})
				return
			}
			respond(w, 200, map[string]string{"receipt": receipt})
			return
		case "restore":
			if r.Method != http.MethodPost {
				fail(w, 405)
				return
			}
			if _, err := query(r); err != nil {
				fail(w, 400)
				return
			}
			var v struct {
				Receipt string `json:"receipt"`
			}
			if !decode(w, r, &v, 1024) {
				return
			}
			reply(w, map[string]bool{"restored": true}, s.service.RestoreSkill(r.Context(), id, v.Receipt))
			return
		}
	}
	fail(w, 404)
}

func (s *Server) attachments(w http.ResponseWriter, r *http.Request, id string) {
	if _, err := query(r); err != nil {
		fail(w, 400)
		return
	}
	switch r.Method {
	case http.MethodGet:
		respond(w, 200, s.service.DraftAttachments(id))
	case http.MethodPost:
		select {
		case s.uploads <- struct{}{}:
			defer func() { <-s.uploads }()
		default:
			fail(w, 503)
			return
		}
		if len(r.Header.Values("X-File-Name")) != 1 {
			fail(w, 400)
			return
		}
		if r.ContentLength > 20<<20 {
			fail(w, 413)
			return
		}
		// Body is original bytes, not base64 JSON, not an arbitrary host path.
		reader := http.MaxBytesReader(w, r.Body, 20<<20)
		value, err := s.service.UploadReader(r.Context(), id, r.Header.Get("X-File-Name"), reader)
		if err != nil {
			var large *http.MaxBytesError
			if errors.As(err, &large) {
				fail(w, 413)
			} else {
				operationError(w, err)
			}
			return
		}
		respond(w, 201, value)
	default:
		fail(w, 405)
	}
}

func (s *Server) artifactBytes(w http.ResponseWriter, r *http.Request, id string) {
	if r.Method != http.MethodGet {
		fail(w, 405)
		return
	}
	q, err := query(r, "branch", "uri", "offset")
	if err != nil || !validID(q.Get("branch")) || len(q.Get("uri")) > 2048 {
		fail(w, 400)
		return
	}
	offset, err := integer(q, "offset", 0, 0, 1<<63-1)
	if err != nil {
		fail(w, 400)
		return
	}
	value, err := s.service.Conversations.ReadArtifact(r.Context(), id, q.Get("branch"), q.Get("uri"), offset)
	if errors.Is(err, hub.ErrNotFound) {
		fail(w, 404)
		return
	}
	reply(w, value, err)
}

func emptyPost(w http.ResponseWriter, r *http.Request) bool {
	if r.Method != http.MethodPost {
		fail(w, 405)
		return false
	}
	if _, err := query(r); err != nil {
		fail(w, 400)
		return false
	}
	// Accept only the explicit empty object, rejecting accidental extra payloads.
	var v struct{}
	return decode(w, r, &v, 1024)
}

func childPath(q url.Values) ([]product.ChildHop, error) {
	if !q.Has("child_path") {
		return nil, nil
	}
	text := q.Get("child_path")
	if text == "" || len(text) > 167 {
		return nil, errors.New("invalid child path")
	}
	parts := strings.Split(text, ",")
	if len(parts) > 8 {
		return nil, errors.New("child depth exceeds 8")
	}
	var path []product.ChildHop
	for _, part := range parts {
		sequence, err := strconv.ParseUint(part, 10, 63)
		if err != nil || sequence == 0 || strconv.FormatUint(sequence, 10) != part {
			return nil, errors.New("canonical child link sequence required")
		}
		path = append(path, product.ChildHop{LinkSequence: sequence})
	}
	return path, nil
}

// settingSwitch changes one capability switch through the product
// SettingsService, the same owner and scope rules as the desktop: application
// scope by default, or {"scope":"session"} for a task that keeps its own
// settings. Enabling a host item also needs host_access_approved in the same
// request. Skills are always session scoped.
func (s *Server) settingSwitch(w http.ResponseWriter, r *http.Request, id, kind, item string) {
	if r.Method != http.MethodPut {
		fail(w, 405)
		return
	}
	if _, err := query(r); err != nil {
		fail(w, 400)
		return
	}
	var v struct {
		Enabled            *bool  `json:"enabled"`
		HostAccessApproved bool   `json:"host_access_approved"`
		Fingerprint        string `json:"fingerprint"`
		Scope              string `json:"scope"`
	}
	if !decode(w, r, &v, 1024) {
		return
	}
	if v.Enabled == nil || v.Scope != "" && v.Scope != "app" && v.Scope != "session" {
		fail(w, 400)
		return
	}
	if kind == "skills" {
		// Session-scoped and never inherited: needs no SettingsService scope.
		reply(w, map[string]bool{"saved": true}, s.service.SetSkillsSwitch(r.Context(), id, *v.Enabled))
		return
	}
	if s.service.Settings == nil {
		fail(w, 404)
		return
	}
	settingsReply(w, map[string]bool{"saved": true}, s.service.Settings.SetCapability(r.Context(), product.SettingsScope{App: v.Scope != "session", SessionID: id}, kind, item, *v.Enabled, v.HostAccessApproved, v.Fingerprint))
}

// settingsReply states why a settings write was refused, in Chinese, for the
// refusals the product itself owns; other failures stay generic.
func settingsReply(w http.ResponseWriter, value any, err error) {
	var fields product.FieldErrors
	switch {
	case err == nil:
		respond(w, 200, value)
	case errors.Is(err, product.ErrUsesApplicationSettings), errors.Is(err, product.ErrHostApprovalRequired), errors.Is(err, product.ErrNoWorkspaceToApprove),
		errors.Is(err, product.ErrUnknownCapability), errors.Is(err, product.ErrFingerprintRequired), errors.Is(err, product.ErrSettingsConflict), errors.Is(err, product.ErrSettingsChanged), errors.As(err, &fields):
		failWith(w, 409, err.Error())
	default:
		operationError(w, err)
	}
}
