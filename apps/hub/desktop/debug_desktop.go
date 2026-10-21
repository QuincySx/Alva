// INPUT: Actual Hub page state, shared product UI methods and independent native diagnostic owner callbacks
// OUTPUT: Registered Hub semantic view tree/state and normal declared UI actions without exposing settings credentials
// POS: [L7 Hub product] Debugging observes the same page and delegates Send/Stop to the existing product owner.
//go:build desktop

package hubdesktop

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/debug"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/apps/ui/chat"
	strictjson "github.com/QuincySx/Alva/platform/system/strictjson"
)

func (p *NativePage) traceUI(method string) {
	if p.diagnosticChanged != nil {
		p.diagnosticChanged()
	}
	if p.diagnosticLog != nil {
		p.diagnosticLog.Info("actual UI handler invoked", "method", method, "surface", "hub")
	}
}

func (p *NativePage) Diagnostic(owner context.Context, enqueue func(func()), capture func() ([]byte, error), bounds func() *debug.Bounds) (*debug.View, []debug.Method, error) {
	snapshot := func() debug.Snapshot {
		hash := sha256.Sum256([]byte(p.model.Draft))
		ids := make([]string, 0, len(p.sessions))
		for _, session := range p.sessions {
			ids = append(ids, session.ID)
		}
		state := map[string]any{"demo": p.Service.Demo, "session_id": p.selected, "pane": p.pane, "generation": p.generation, "status": p.model.Status, "active_run_id": p.model.ActiveRunID, "busy": p.busy, "pane_busy": p.paneBusy, "draft_bytes": len(p.model.Draft), "draft_sha256": hex.EncodeToString(hash[:]), "message_rows": len(p.model.Messages), "history_sequence": p.transcript.Last, "sessions": ids, "pending_approvals": len(p.approvals), "pending_questions": len(p.questions), "player_busy": p.playerBusy, "gateway_busy": p.gatewayBusy, "can_send": p.selected != "" && p.pane == "" && !p.busy && !p.chat.Busy && p.model.CanSend(), "can_stop": p.selected != "" && p.pane == "" && !p.busy && !p.chat.Busy && p.model.ActiveRunID != "" && p.model.Status != chat.Stopping, "has_problem": p.problem != ""}
		state["composer_busy"] = p.chat.Busy
		state["export_busy"] = p.inspectExportBusy
		state["inspector_branch"] = p.inspectBranch
		state["inspector_native_id"] = p.inspectPage.NativeID
		state["inspector_child_path"] = append([]product.ChildHop(nil), p.inspectPath...)
		state["inspector_high_water"] = p.inspectPage.HighWater
		state["inspector_cursor"] = p.inspectCursor
		queryHash := sha256.Sum256([]byte(p.inspectQuery))
		state["inspector_query_sha256"] = hex.EncodeToString(queryHash[:])
		state["inspector_group"] = p.inspectGroup
		state["export_children"] = p.inspectExportChildren
		state["has_export"] = p.inspectExportPath != ""
		state["history_busy"] = p.polling
		state["can_latest"] = p.selected != "" && p.pane == "" && !p.busy && !p.polling
		if len(p.model.Draft) <= 8192 {
			state["draft"] = p.model.Draft
		} else {
			state["draft_processing"] = "over-budget draft body omitted whole"
		}
		var rectangle *debug.Bounds
		if bounds != nil {
			rectangle = bounds()
		}
		tree := debug.Node{ID: "window", Type: "AlvaHubWindow", Bounds: rectangle, Children: []debug.Node{{ID: "sessions", Type: "SessionPage", Properties: map[string]any{"rows": len(p.sessions), "selected": p.selected}}, {ID: "navigation", Type: "ProductPane", Properties: map[string]any{"selected": p.pane}}, {ID: "transcript", Type: "BoundedProductHistory", Properties: map[string]any{"rows": len(p.model.Messages), "sequence": p.transcript.Last}}, {ID: "composer", Type: "NativeComposer", Properties: map[string]any{"draft_bytes": len(p.model.Draft)}}, {ID: "approvals", Type: "ActualOwnerRequests", Properties: map[string]any{"permissions": len(p.approvals), "questions": len(p.questions)}}}}
		return debug.Snapshot{Tree: tree, State: state}
	}
	apply := func(method string, raw json.RawMessage) error {
		switch method {
		case "set_draft":
			var args struct {
				Text string `json:"text"`
			}
			if err := strictjson.ValidateObject(raw, &args); err != nil {
				return err
			}
			if err := json.Unmarshal(raw, &args); err != nil {
				return err
			}
			if !utf8.ValidString(args.Text) || len(args.Text) > chat.MaxDraftBytes || p.busy || p.chat.Busy || p.pane != "" || p.selected == "" || p.inputBlocked() {
				return errors.New("actual composer is busy or over budget")
			}
			p.model.Draft = args.Text
			return nil
		case "select_session":
			var args struct {
				ID string `json:"id"`
			}
			if err := strictjson.ValidateObject(raw, &args); err != nil {
				return err
			}
			if err := json.Unmarshal(raw, &args); err != nil {
				return err
			}
			if p.busy {
				return errors.New("actual session selection is busy")
			}
			for _, session := range p.sessions {
				if session.ID == args.ID {
					p.selectSession(args.ID)
					return nil
				}
			}
			return errors.New("select only an actual visible session ID")
		case "create_session":
			var args struct {
				Title     string `json:"title"`
				Workspace string `json:"workspace"`
			}
			if err := strictjson.ValidateObject(raw, &args); err != nil {
				return err
			}
			if err := json.Unmarshal(raw, &args); err != nil {
				return err
			}
			if p.busy || args.Title == "" || len(args.Title) > 256 || len(args.Workspace) > 4096 {
				return errors.New("actual session creation fields are unavailable")
			}
			p.title, p.workspace = args.Title, args.Workspace
			p.create()
			return nil
		case "open_pane":
			var args struct {
				Pane string `json:"pane"`
			}
			if err := strictjson.ValidateObject(raw, &args); err != nil {
				return err
			}
			if err := json.Unmarshal(raw, &args); err != nil {
				return err
			}
			if p.paneBusy {
				return errors.New("actual navigation is busy")
			}
			if args.Pane != "chat" && p.selected == "" {
				return errors.New("actual product pane requires a selected session")
			}
			switch args.Pane {
			case "chat":
				p.setPane("")
			case "settings":
				p.openSettings()
			case "inspector":
				p.openInspector()
			case "media":
				p.openMedia()
			case "team":
				if p.Service.Teams == nil {
					return errors.New("actual collaboration control is disabled")
				}
				p.openTeam()
			case "automations":
				if p.Service.Automations == nil {
					return errors.New("actual automations control is disabled")
				}
				p.openAutomations()
			case "handoffs":
				p.setPane("handoffs")
			default:
				return errors.New("undeclared product pane")
			}
			return nil
		}
		var args struct{}
		if err := strictjson.ValidateObject(raw, &args); err != nil {
			return err
		}
		switch method {
		case "send":
			if p.selected == "" || p.busy || p.chat.Busy || p.pane != "" || !p.model.CanSend() {
				return errors.New("actual Send control is disabled")
			}
			p.send()
		case "stop":
			if p.selected == "" || p.busy || p.chat.Busy || p.pane != "" || p.model.ActiveRunID == "" || p.model.Status == chat.Stopping {
				return errors.New("actual Stop control is disabled")
			}
			p.stop()
		case "latest":
			if p.selected == "" || p.busy || p.polling || p.pane != "" {
				return errors.New("actual Latest control is unavailable")
			}
			p.loadTail()
		case "new_window":
			if p.openWindow == nil {
				return errors.New("window owner is unavailable")
			}
			return p.openWindow(p.selected)
		case "export_inspector":
			if p.pane != "inspector" || p.paneBusy || p.inspectPage.NativeID == "" {
				return errors.New("actual Inspector export unavailable")
			}
			p.exportInspector()
		case "close_window":
			if p.window == nil {
				return errors.New("actual window owner unavailable")
			}
			p.window.Close()
		default:
			return errors.New("undeclared Hub UI action")
		}
		return nil
	}
	protectedCapture := func() ([]byte, error) {
		if p.gatewayCredentials != "" || p.pane == "settings" || p.pane == "plugins" || p.pane == "commands" || p.pane == "tools" || p.pane == "browser-settings" {
			return nil, errors.New("credential-capable editor is visible; window capture is unavailable")
		}
		if capture == nil {
			return nil, errors.New("actual window capture unavailable")
		}
		return capture()
	}
	view, err := debug.NewView(owner, enqueue, snapshot, apply, protectedCapture)
	methods := []debug.Method{{Name: "set_draft", Effect: "ui", Description: "Edit the actual unsent composer only."}, {Name: "select_session", Effect: "ui", Description: "Select an actual ID in the currently displayed session page."}, {Name: "create_session", Effect: "ui", Description: "Use normal product creation without submitting a model request."}, {Name: "open_pane", Effect: "ui", Description: "Open a declared normal product pane; no settings mutation."}, {Name: "latest", Effect: "ui", Description: "Load the normal newest bounded product history page."}, {Name: "new_window", Effect: "ui", Description: "Open another bounded observer using the same product owner."}, {Name: "stop", Effect: "stop", Description: "Request actual product Stop; observe completion separately."}, {Name: "send", Effect: "execute", Description: "Use normal Send/Worker admission and existing permissions/billing."}}
	methods = append(methods, debug.Method{Name: "export_inspector", Effect: "ui", Description: "Save all typed records at the actual selected Inspector snapshot in the private application exports folder; no runtime or referenced artifact fetch."})
	methods = append(methods, debug.Method{Name: "close_window", Effect: "ui", Description: "Request the normal Close of this exact observer window. Existing active-run/local-work close guards still apply; this is not Stop."})
	return view, methods, err
}
