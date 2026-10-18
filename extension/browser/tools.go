// INPUT: bytes, context, crypto/sha256, encoding/hex/json, errors, io, time, utf8, host/browser, ABI/wire, protocol/browser, session, platform/system/strictjson.
// OUTPUT: Seven actual browser Tool implementations with immutable launch policy, HITL and bounded source references.
// POS: [L4 extension] Exact dispatched identity gates all native browser authority and large results never become inline model bodies.
package browser

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"time"
	"unicode/utf8"

	protocol "github.com/QuincySx/Alva/integration/protocol/browser"
	host "github.com/QuincySx/Alva/platform/host/browser"
	"github.com/QuincySx/Alva/platform/storage/session"
	strictjson "github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type browserTool struct {
	plugin *Plugin
	name   string
}

func (t *browserTool) Name() string { return t.name }
func (t *browserTool) Description() string {
	return map[string]string{"browser_start": "Start an approved fresh private run-owned browser instance; no existing profile or arbitrary argv. Must stop at run end.", "browser_stop": "Stop and actually reap this run-owned browser and remove only its private temporary profile.", "browser_navigate": "Open a new owned tab, navigate explicit HTTP/HTTPS and wait for page load, then return real URL/title source metadata.", "browser_action": "Perform actual selector/coordinate click, text insertion, named key press or bounded scroll on the latest owned tab; external side effects require approval.", "browser_snapshot": "Retain complete bounded text/HTML or explicit heuristic readability extraction as same-session reference, not raw model context.", "browser_screenshot": "Retain actual PNG/JPEG viewport/full-page/element screenshot bytes as same-session artifact; no arbitrary output path or automatic model image delivery.", "browser_status": "Inspect actual owned browser state and tracked tabs; no existing-browser discovery or automatic startup."}[t.name]
}
func (*browserTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.SerialGlobal, ManagesOwnTimeout: true}
}
func (t *browserTool) IsReadOnly(wire.ToolCall) bool { return t.name == "browser_status" }
func (t *browserTool) DescribeEffect(wire.ToolCall) abi.Effect {
	kind := abi.EffectUnknown
	if t.name == "browser_status" {
		kind = abi.EffectRead
	}
	return abi.Effect{Kind: kind, Description: "Inspect or interact with explicitly owned browser; page scripts/network and user actions may have external effects; no OS filesystem/network sandbox claim"}
}
func (t *browserTool) ParametersSchema() json.RawMessage {
	properties := map[string]any{"id": map[string]any{"type": "string", "maxLength": 64}}
	add := func(name, kind string) { properties[name] = map[string]any{"type": kind} }
	var required []string
	switch t.name {
	case "browser_navigate":
		add("url", "string")
		required = []string{"url"}
	case "browser_action":
		for _, name := range []string{"action", "selector", "text", "key", "direction"} {
			add(name, "string")
		}
		for _, name := range []string{"x", "y"} {
			add(name, "number")
		}
		add("amount", "integer")
		required = []string{"action"}
	case "browser_snapshot":
		add("mode", "string")
		add("selector", "string")
	case "browser_screenshot":
		add("format", "string")
		add("selector", "string")
		add("quality", "integer")
		add("full_page", "boolean")
	}
	value := map[string]any{"type": "object", "additionalProperties": false, "properties": properties}
	if len(required) > 0 {
		value["required"] = required
	}
	data, _ := json.Marshal(value)
	return data
}

type arguments struct {
	ID, URL, Action, Selector, Key, Direction, Mode, Format string
	Text                                                    *string
	X, Y                                                    *float64
	Amount                                                  int64
	FullPage                                                bool `json:"full_page"`
	Quality                                                 *int64
}

func (t *browserTool) decode(raw json.RawMessage) (arguments, error) {
	var a arguments
	if len(raw) > 128<<10 || !utf8.Valid(raw) {
		return a, errors.New("browser input exceeds valid UTF-8 128 KiB budget")
	}
	if err := strictjson.Validate(raw); err != nil {
		return a, err
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || fields == nil {
		return a, errors.New("browser input must be an explicit object")
	}
	var schema struct {
		Properties map[string]any `json:"properties"`
	}
	json.Unmarshal(t.ParametersSchema(), &schema)
	for key, value := range fields {
		if _, ok := schema.Properties[key]; !ok || bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return a, errors.New("unknown/null browser argument")
		}
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&a); err != nil {
		return a, err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return a, errors.New("browser requires one input object")
	}
	if a.ID == "" {
		a.ID = "default"
	}
	if len(a.ID) > 64 || !utf8.ValidString(a.ID) {
		return a, errors.New("invalid browser instance id")
	}
	if t.name == "browser_action" {
		switch a.Action {
		case "click":
			if a.Text != nil || a.Key != "" || a.Direction != "" || a.Amount != 0 {
				return a, errors.New("click does not consume text/key/scroll options")
			}
			if a.Selector == "" && (a.X == nil || a.Y == nil) {
				return a, errors.New("click requires selector or x/y")
			}
		case "type":
			if a.Key != "" || a.Direction != "" || a.Amount != 0 || a.X != nil || a.Y != nil {
				return a, errors.New("type does not consume key/coordinates/scroll options")
			}
			if a.Text == nil {
				return a, errors.New("type requires explicit text")
			}
		case "press":
			if a.Text != nil || a.Selector != "" || a.Direction != "" || a.Amount != 0 || a.X != nil || a.Y != nil {
				return a, errors.New("press consumes only an explicit key name")
			}
			if a.Key == "" {
				return a, errors.New("press requires key")
			}
		case "scroll":
			if a.Text != nil || a.Selector != "" || a.Key != "" || a.X != nil || a.Y != nil {
				return a, errors.New("scroll consumes only direction/amount")
			}
		default:
			return a, errors.New("unsupported browser action")
		}
	}
	if t.name == "browser_screenshot" && a.Quality != nil && a.Format != "jpeg" {
		return a, errors.New("quality is only defined for explicit jpeg format")
	}
	return a, nil
}
func (t *browserTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (result wire.ToolResult, runErr error) {
	actual, ok := e.ActualTool.(*browserTool)
	if !ok || actual != t || call.Name != t.name || call.ID == "" || call.ID != e.CallID {
		return result, errors.New("browser requires its actual dispatched tool/call authority")
	}
	a, err := t.decode(call.Arguments)
	if err != nil {
		return result, err
	}
	p := t.plugin
	select {
	case p.gate <- struct{}{}:
	case <-ctx.Done():
		return result, ctx.Err()
	}
	defer func() { <-p.gate }()
	m, err := p.check(e)
	if err != nil {
		return result, err
	}
	p.mu.Lock()
	runCtx := p.runCtx
	p.mu.Unlock()
	operation, stopOperation := context.WithCancel(ctx)
	defer stopOperation()
	stopRun := context.AfterFunc(runCtx, stopOperation)
	defer stopRun()
	if runCtx.Err() != nil {
		stopOperation()
	}
	ctx = operation
	if err = ctx.Err(); err != nil {
		return result, err
	}
	if err = p.config.Guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return result, err
	}
	p.mu.Lock()
	if p.operations >= 64 {
		p.mu.Unlock()
		return result, errors.New("browser operation budget exceeds 64 per run")
	}
	p.operations++
	p.mu.Unlock()
	admit := func(ctx context.Context) error {
		if _, err := p.check(e); err != nil {
			return err
		}
		fingerprint := sha256.Sum256([]byte(e.RunID + "\x00" + call.ID + "\x00" + t.name))
		activityID := "browser-" + hex.EncodeToString(fingerprint[:16])
		if err := p.config.Report(ctx, Activity{ID: activityID, Name: t.name, SessionID: e.SessionID, RunID: e.RunID, CallID: e.CallID, Effect: t.DescribeEffect(call).Kind}); err != nil {
			return err
		}
		return p.append(ctx, "browser_intent", map[string]any{"run_id": e.RunID, "call_id": call.ID, "tool": t.name, "instance_id": a.ID, "arguments": json.RawMessage(call.Arguments), "state": "operation_attempted", "notice": "Admission is not proof of external effects; cancellation never automatically replays."})
	}
	deadline, cancel := context.WithTimeout(ctx, p.config.OperationTimeout)
	defer cancel()
	if t.name != "browser_start" {
		if err = admit(deadline); err != nil {
			return result, err
		}
	}
	var body any
	switch t.name {
	case "browser_start":
		body, runErr = m.Start(deadline, a.ID, func(ctx context.Context, _ host.Launch) error { return admit(ctx) })
	case "browser_stop":
		body, runErr = m.Stop(deadline, a.ID)
	case "browser_status":
		var statuses []any
		running := false
		for _, status := range m.Status() {
			if a.ID != "*" && status.ID != a.ID {
				continue
			}
			entry := map[string]any{"state": status}
			if status.Running {
				running = true
				client, err := m.Client(status.ID)
				if err != nil {
					runErr = err
					break
				}
				tabs, err := client.Tabs(deadline)
				if err != nil {
					runErr = err
					break
				}
				entry["tabs"] = tabs
			}
			statuses = append(statuses, entry)
		}
		body = map[string]any{"instances": statuses, "selected_id": a.ID, "running": running, "scope": "only this run's explicitly owned instances; no discovery"}
	default:
		client, err := m.Client(a.ID)
		if err != nil {
			runErr = err
			break
		}
		switch t.name {
		case "browser_navigate":
			body, runErr = client.Navigate(deadline, a.URL)
		case "browser_action":
			text := ""
			if a.Text != nil {
				text = *a.Text
			}
			runErr = client.Action(deadline, protocol.Action{Kind: a.Action, Selector: a.Selector, Text: text, Key: a.Key, Direction: a.Direction, X: a.X, Y: a.Y, Amount: a.Amount})
			body = map[string]any{"status": "action_completed", "action": a.Action, "notice": "CDP input dispatch completed, not proof of remote server-side completion"}
		case "browser_snapshot":
			var snapshot protocol.Snapshot
			snapshot, runErr = client.Snapshot(deadline, a.Mode, a.Selector)
			if runErr == nil {
				ref, err := p.retain(deadline, session.ArtifactInput{Kind: "browser-snapshot", Name: snapshot.Mode, MediaType: "text/plain; charset=utf-8", Processing: snapshot.Processing, Data: []byte(snapshot.Content), Text: true})
				runErr = err
				body = map[string]any{"status": "source_retained", "source": ref, "mode": snapshot.Mode, "tab": snapshot.Tab, "processing": snapshot.Processing}
			}
		case "browser_screenshot":
			quality := int64(0)
			if a.Format == "jpeg" {
				quality = 80
			}
			if a.Quality != nil {
				quality = *a.Quality
			}
			var shot protocol.Screenshot
			shot, runErr = client.Screenshot(deadline, protocol.ScreenshotOptions{Format: a.Format, Quality: quality, FullPage: a.FullPage, Selector: a.Selector})
			if runErr == nil {
				ref, err := p.retain(deadline, session.ArtifactInput{Kind: "browser-screenshot", Name: "screenshot." + shot.Format, MediaType: "image/" + shot.Format, Processing: shot.Processing, Data: shot.Data})
				runErr = err
				imageBody := map[string]any{"status": "image_retained_not_delivered", "source": ref, "format": shot.Format, "width": shot.Width, "height": shot.Height, "processing": shot.Processing}
				if shot.Format == "jpeg" {
					imageBody["jpeg_quality"] = quality
				}
				body = imageBody
			}
		}
	}
	if runErr != nil && ctx.Err() == nil && abi.OnlyCancellation(runErr) {
		runErr = errors.Join(errors.New("browser operation's own deadline failed"), runErr)
	}
	cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer end()
	if runErr != nil {
		runErr = errors.Join(runErr, m.Close(cleanup))
	}
	data, err := json.Marshal(body)
	runErr = errors.Join(runErr, err)
	if runErr == nil && len(data) > 4096 {
		ref, err := p.retain(cleanup, session.ArtifactInput{Kind: "browser-result", Name: t.name, MediaType: "application/json", Processing: "Complete browser operation metadata JSON; source evidence only, not automatically delivered in full", Data: data, Text: true})
		runErr = err
		data, _ = json.Marshal(map[string]any{"source": ref, "notice": "Complete operation metadata retained; retrieve only relevant bounded ranges"})
	}
	outcome := map[string]any{"run_id": e.RunID, "call_id": call.ID, "tool": t.name, "instance_id": a.ID, "state": "completed", "result": json.RawMessage(data)}
	if runErr != nil {
		outcome["state"] = "failed_or_cancelled_unknown_effects"
		message := runErr.Error()
		if len(message) > 1024 {
			message = "Browser operation failed; inspect retained intent/process evidence."
		}
		outcome["error"] = message
	}
	runErr = errors.Join(runErr, p.append(cleanup, "browser_outcome", outcome))
	if runErr != nil {
		return wire.ToolResult{}, runErr
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, nil
}
