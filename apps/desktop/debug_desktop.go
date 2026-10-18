// INPUT: Actual direct UI model/callbacks, observer lifetime and own-window dispatch/capture functions
// OUTPUT: NativeView diagnostic owner with bounded real view state and normal UI methods
// POS: [L6 application] Debug Send/Stop use the same callbacks as rendered controls, never another harness or execution path.
//go:build desktop

package desktop

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/debug"
	"github.com/QuincySx/Alva/platform/system/httpjson"
)

func (v *NativeView) traceUI(method string) {
	if v.diagnosticChanged != nil {
		v.diagnosticChanged()
	}
	if v.diagnosticLog != nil {
		v.diagnosticLog.Info("actual UI handler invoked", "method", method, "surface", "direct")
	}
}

func (v *NativeView) Diagnostic(owner context.Context, enqueue func(func()), capture func() ([]byte, error), bounds func() *debug.Bounds) (*debug.View, []debug.Method, error) {
	snapshot := func() debug.Snapshot {
		m := v.Model
		hash := sha256.Sum256([]byte(m.Draft))
		state := map[string]any{"demo": m.Demo, "route": m.RouteLabel, "status": m.Status, "active_run_id": m.ActiveRunID, "sequence": m.lastSeq, "busy": v.Busy, "draft_bytes": len(m.Draft), "draft_sha256": hex.EncodeToString(hash[:]), "message_rows": len(m.Messages), "can_send": m.CanSend() && !v.Busy, "can_stop": m.ActiveRunID != "" && m.Status != Stopping && !v.Busy, "has_error": m.Error != ""}
		if len(m.Draft) <= 8192 {
			state["draft"] = m.Draft
		} else {
			state["draft_processing"] = "over-budget draft body omitted whole"
		}
		var rectangle *debug.Bounds
		if bounds != nil {
			rectangle = bounds()
		}
		tree := debug.Node{ID: "window", Type: "AlvaDirectWindow", Bounds: rectangle, Children: []debug.Node{{ID: "transcript", Type: "BoundedMessageList", Properties: map[string]any{"rows": len(m.Messages), "authority": "temporary UI cache"}}, {ID: "composer", Type: "NativeComposer", Properties: map[string]any{"draft_bytes": len(m.Draft), "send_enabled": m.CanSend() && !v.Busy}}, {ID: "execution", Type: "RunStatus", Properties: map[string]any{"status": m.Status, "active_run_id": m.ActiveRunID}}}}
		return debug.Snapshot{Tree: tree, State: state}
	}
	apply := func(method string, raw json.RawMessage) error {
		if method == "set_draft" {
			var args struct {
				Text string `json:"text"`
			}
			if err := httpjson.ValidateObject(raw, &args); err != nil {
				return err
			}
			if err := json.Unmarshal(raw, &args); err != nil {
				return err
			}
			if !utf8.ValidString(args.Text) || len(args.Text) > MaxDraftBytes || v.Busy || v.Model.ActiveRunID != "" {
				return errors.New("composer is busy or draft exceeds its actual budget")
			}
			v.Model.Draft = args.Text
			return nil
		}
		var args struct{}
		if err := httpjson.ValidateObject(raw, &args); err != nil {
			return err
		}
		switch method {
		case "send":
			if v.Send == nil || v.Busy || !v.Model.CanSend() {
				return errors.New("actual Send control is disabled")
			}
			v.Send()
		case "stop":
			if v.Stop == nil || v.Busy || v.Model.ActiveRunID == "" || v.Model.Status == Stopping {
				return errors.New("actual Stop control is disabled")
			}
			v.Stop()
		case "latest":
			v.list.ScrollToEnd()
		default:
			return errors.New("undeclared direct UI action")
		}
		return nil
	}
	view, err := debug.NewView(owner, enqueue, snapshot, apply, capture)
	methods := []debug.Method{{Name: "set_draft", Description: "Edit the actual unsent composer; no model request.", Effect: "ui"}, {Name: "latest", Description: "Scroll the actual bounded transcript to its end.", Effect: "ui"}, {Name: "stop", Description: "Request the existing Controller Stop and observe its actual terminal status.", Effect: "stop"}, {Name: "send", Description: "Press the actual Send control, using the existing harness/controller.", Effect: "execute"}}
	return view, methods, err
}
