// INPUT: Actual persistent direct page state, UI dispatcher and registered own-window capture; shared chat presentation
// OUTPUT: PersistentPage diagnostic semantic tree and declared normal UI actions
// POS: [L6 application] Native diagnostics do not grant a new execution/approval or history scope.
//go:build desktop

package desktop

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/apps/debug"
	chat "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/platform/system/httpjson"
)

func (p *PersistentPage) Diagnostic(ctx context.Context, enqueue func(func()), capture func() ([]byte, error)) (*debug.View, []debug.Method, error) {
	snapshot := func() debug.Snapshot {
		hash := sha256.Sum256([]byte(p.model.Draft))
		ids := make([]string, 0, len(p.rows.Sessions))
		for _, row := range p.rows.Sessions {
			ids = append(ids, row.ID)
		}
		state := map[string]any{"demo": p.Service.Demo(), "session_id": p.selected, "pane": p.pane, "generation": p.generation, "status": p.model.Status, "active_run_id": p.model.ActiveRunID, "busy": p.busy, "composer_busy": p.chat.Busy, "message_rows": len(p.model.Messages), "draft_bytes": len(p.model.Draft), "draft_sha256": hex.EncodeToString(hash[:]), "sessions": ids, "pending_approvals": len(p.approvals), "has_problem": p.problem != "", "can_send": p.selected != "" && p.pane == "" && !p.busy && p.model.CanSend()}
		if len(p.model.Draft) <= 8192 {
			// Draft inclusion remains bounded independently from Inspector metadata.
			state["draft"] = p.model.Draft
		}
		queryHash := sha256.Sum256([]byte(p.inspectQuery))
		state["export_busy"] = p.inspectExportBusy
		state["inspector_busy"] = p.inspectorBusy
		state["inspector_query_sha256"] = hex.EncodeToString(queryHash[:])
		state["inspector_group"] = p.inspectGroup
		state["has_export"] = p.inspectExportPath != ""
		return debug.Snapshot{Tree: debug.Node{ID: "window", Type: "AlvaPersistentDirectWindow", Children: []debug.Node{{ID: "sessions", Type: "NativeSessionPage", Properties: map[string]any{"rows": len(ids), "selected": p.selected}}, {ID: "conversation", Type: "BoundedNativeHistory", Properties: map[string]any{"rows": len(p.model.Messages), "high_water": p.history.HighWater}}, {ID: "composer", Type: "NativeComposer"}, {ID: "approvals", Type: "ActualCallPermissions", Properties: map[string]any{"pending": len(p.approvals)}}, {ID: "inspector", Type: "NativeTypedRecordPage", Properties: map[string]any{"high_water": p.inspector.HighWater}}}}, State: state}
	}
	apply := func(method string, raw json.RawMessage) error {
		switch method {
		case "create_session":
			var args struct {
				Title string `json:"title"`
			}
			if err := httpjson.ValidateObject(raw, &args); err != nil {
				return err
			}
			if err := json.Unmarshal(raw, &args); err != nil {
				return err
			}
			if p.busy || len(args.Title) > 256 || args.Title == "" {
				return errors.New("actual session creation fields unavailable")
			}
			p.title = args.Title
			p.create()
			return nil
		case "select_session":
			var args struct {
				ID string `json:"id"`
			}
			if err := httpjson.ValidateObject(raw, &args); err != nil {
				return err
			}
			if err := json.Unmarshal(raw, &args); err != nil {
				return err
			}
			if p.busy {
				return errors.New("actual selection busy")
			}
			for _, row := range p.rows.Sessions {
				if row.ID == args.ID && row.MetadataAvailable {
					p.selectSession(row.ID)
					if p.selected != row.ID {
						return errors.New("actual selection rejected")
					}
					return nil
				}
			}
			return errors.New("select only an actual visible owned session")
		case "set_draft":
			var args struct {
				Text string `json:"text"`
			}
			if err := httpjson.ValidateObject(raw, &args); err != nil {
				return err
			}
			if err := json.Unmarshal(raw, &args); err != nil {
				return err
			}
			if p.selected == "" || p.pane != "" || p.busy || p.chat.Busy || p.model.ActiveRunID != "" || len(args.Text) > chat.MaxDraftBytes {
				return errors.New("actual composer disabled")
			}
			p.model.Draft = args.Text
			return nil
		}
		var args struct{}
		if err := httpjson.ValidateObject(raw, &args); err != nil {
			return err
		}
		switch method {
		case "send":
			if p.selected == "" || p.pane != "" || p.busy || p.chat.Busy || !p.model.CanSend() {
				return errors.New("actual Send disabled")
			}
			p.send()
		case "stop":
			if p.pane != "" || p.busy || p.model.ActiveRunID == "" || p.model.Status == chat.Stopping {
				return errors.New("actual Stop disabled")
			}
			p.stop()
		case "latest":
			if p.selected == "" || p.busy || p.pane != "" {
				return errors.New("actual Latest unavailable")
			}
			p.loadHistory(0)
		case "inspector":
			if p.selected == "" || p.inspectorBusy {
				return errors.New("actual Inspector disabled")
			}
			p.inspectAfter = nil
			p.loadInspector(0, 0)
		case "export_inspector":
			if p.pane != "inspector" || p.inspectorBusy || p.inspector.HighWater == 0 {
				return errors.New("actual Inspector export unavailable")
			}
			p.exportInspector()
		case "close_window":
			if p.window == nil {
				return errors.New("actual window owner unavailable")
			}
			p.window.Close()
		case "chat":
			p.changed()
			p.pane = ""
		default:
			return errors.New("undeclared persistent direct action")
		}
		return nil
	}
	view, err := debug.NewView(ctx, enqueue, snapshot, apply, capture)
	methods := []debug.Method{{Name: "create_session", Effect: "ui", Description: "Create native metadata only; no Agent run."}, {Name: "select_session", Effect: "ui", Description: "Select one actual visible owned native ID."}, {Name: "set_draft", Effect: "ui", Description: "Edit the actual unsent composer."}, {Name: "latest", Effect: "ui", Description: "Cold-load the latest bounded native record window."}, {Name: "inspector", Effect: "ui", Description: "Read the actual native record metadata pane."}, {Name: "chat", Effect: "ui", Description: "Return to the normal native chat controls."}, {Name: "stop", Effect: "stop", Description: "Use the existing selected Controller Stop and observe true cleanup."}, {Name: "send", Effect: "execute", Description: "Use the existing direct Service/harness and actual approvals; never another Agent route."}}
	methods = append(methods, debug.Method{Name: "export_inspector", Effect: "ui", Description: "Save all selected snapshot records as redacted JSONL in the private direct exports folder; no model work or artifact fetch."})
	methods = append(methods, debug.Method{Name: "close_window", Effect: "ui", Description: "Request this actual observer window's normal Close; active-run/export guards still apply, never Stop."})
	return view, methods, err
}
