// INPUT: platform/system/httpjson, Claimed Lead/native identity, shared product Team service and actual permission Guard
// OUTPUT: Bounded hub_team Tool/Plugin for visible independent Worker queues and outcomes
// POS: [L7 Hub product] Product collaboration capability, never inherited implicitly by native SDK children
package product

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/system/httpjson"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type TeamTool struct {
	Service             *HubService
	ProductID, NativeID string
	Guard               *security.Guard
}

func (*TeamTool) Name() string { return "hub_team" }
func (*TeamTool) Description() string {
	return "Manage visible independent product Workers (not native subagents). list shows this Lead's Team and actual configured routes; create/add/enqueue/edit/cancel/resume/stop/lifecycle require approval. Enqueue persists only; Resume explicitly starts FIFO. Read bounded task metadata/outcomes then selected 2KiB task ranges. No startup replay, automatic bypass, hidden credentials, implicit changed-file/test evidence or parent native-log grants."
}
func (*TeamTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","properties":{"action":{"enum":["list","create","add","tasks","read","enqueue","edit","cancel","resume","stop","active","hibernated","archived","end"]},"team_id":{"type":"string","maxLength":128},"worker_id":{"type":"string","maxLength":128},"name":{"type":"string","maxLength":80},"target_index":{"type":"integer","minimum":0,"maximum":3},"task_id":{"type":"string","maxLength":128},"scope":{"type":"string","maxLength":16384},"claims":{"type":"array","maxItems":16,"items":{"type":"object","properties":{"path":{"type":"string","maxLength":1024},"write":{"type":"boolean"}},"required":["path","write"],"additionalProperties":false}},"after":{"type":"integer","minimum":0},"sequence":{"type":"integer","minimum":1},"offset":{"type":"integer","minimum":0}},"required":["action"],"additionalProperties":false}`)
}

type teamArgs struct {
	Action      string              `json:"action"`
	TeamID      string              `json:"team_id"`
	WorkerID    string              `json:"worker_id"`
	Name        string              `json:"name"`
	TargetIndex int                 `json:"target_index"`
	TaskID      string              `json:"task_id"`
	Scope       string              `json:"scope"`
	Claims      []hub.ResourceClaim `json:"claims"`
	After       uint64              `json:"after"`
	Sequence    uint64              `json:"sequence"`
	Offset      int                 `json:"offset"`
}

func teamReadOnly(call wire.ToolCall) bool {
	var a teamArgs
	if json.Unmarshal(call.Arguments, &a) != nil {
		return false
	}
	return a.Action == "list" || a.Action == "tasks" || a.Action == "read"
}
func (t *TeamTool) DescribeEffect(call wire.ToolCall) abi.Effect {
	if teamReadOnly(call) {
		return abi.Effect{Kind: abi.EffectRead, Description: "Read bounded Team/Worker task metadata for the claimed Lead only"}
	}
	description := "Mutate this Lead's durable Worker queues or actual execution; account/billing and full task arguments require consent"
	var a teamArgs
	if json.Unmarshal(call.Arguments, &a) == nil && a.Action == "add" && t.Service != nil && a.TargetIndex >= 0 && a.TargetIndex < len(t.Service.Targets) {
		r := t.Service.Targets[a.TargetIndex]
		description = fmt.Sprintf("Create a durable Worker: engine=%s model=%s provider=%s account=%s billing=%s; separate permissions, no implicit autonomy", r.EngineID, r.ModelID, r.ProviderID, r.AccountSource, r.BillingRoute)
	}
	return abi.Effect{Kind: abi.EffectUnknown, Description: description}
}
func (*TeamTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: false, ExecutionMode: abi.Coordinator}
}
func (*TeamTool) IsReadOnly(call wire.ToolCall) bool { return teamReadOnly(call) }
func (t *TeamTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if t == nil || t.Service == nil || t.Service.Teams == nil || t.Guard == nil || e.SessionID != t.NativeID || t.NativeID == "" || t.ProductID == "" || call.Name != t.Name() || e.ActualTool != t || call.ID == "" || e.CallID != call.ID {
		return wire.ToolResult{}, errors.New("Team execution binding denied")
	}
	detail, err := t.Service.Store.Detail(ctx, t.ProductID)
	if err != nil {
		return wire.ToolResult{}, err
	}
	owned := false
	for _, b := range detail.Branches {
		owned = owned || b.NativeSessionID == t.NativeID
	}
	if !owned {
		return wire.ToolResult{}, errors.New("native session is not this product's claimed branch")
	}
	if len(call.Arguments) > 24<<10 {
		return wire.ToolResult{}, errors.New("Team arguments exceed 24 KiB")
	}
	var a teamArgs
	if err = httpjson.ValidateObject(call.Arguments, &a); err != nil {
		return wire.ToolResult{}, err
	}
	if err = json.Unmarshal(call.Arguments, &a); err != nil {
		return wire.ToolResult{}, err
	}
	if err = t.Guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return wire.ToolResult{}, err
	}
	store := t.Service.Teams.Store
	var payload any
	switch a.Action {
	case "list":
		team, e := store.TeamForLead(ctx, t.ProductID)
		if e != nil && !errors.Is(e, sql.ErrNoRows) {
			return wire.ToolResult{}, e
		}
		if a.After > uint64(len(team.Workers)) {
			return wire.ToolResult{}, errors.New("worker index exceeds bounded Team")
		}
		first := int(a.After)
		end := first + 4
		if end > len(team.Workers) {
			end = len(team.Workers)
		}
		payload = struct {
			Team    hub.TeamRun  `json:"team"`
			Workers []hub.Worker `json:"workers"`
			Next    int          `json:"next_index"`
			HasMore bool         `json:"has_more"`
			Targets []hub.Target `json:"configured_routes"`
		}{team.Team, team.Workers[first:end], end, end < len(team.Workers), t.Service.Targets}

	case "create":
		payload, err = store.CreateTeam(ctx, t.ProductID)
	case "add":
		if a.TargetIndex < 0 || a.TargetIndex >= len(t.Service.Targets) {
			return wire.ToolResult{}, errors.New("unknown configured route index")
		}
		payload, err = t.Service.AddWorker(ctx, t.ProductID, a.TeamID, a.Name, t.Service.Targets[a.TargetIndex], true)
	case "tasks":
		page, e := store.WorkerTasks(ctx, t.ProductID, a.TeamID, a.WorkerID, a.After, 8)
		err = e
		type index struct {
			ID               string         `json:"id"`
			Sequence         uint64         `json:"sequence"`
			Status           engine.Outcome `json:"status"`
			TurnID           string         `json:"turn_id"`
			Excerpt          string         `json:"excerpt"`
			HasOutcome       bool           `json:"has_outcome"`
			RequiredBranchID string         `json:"required_branch_id,omitempty"`
		}
		var entries []index
		for _, task := range page.Tasks {
			entries = append(entries, index{task.ID, task.Sequence, task.Status, task.TurnID, shortText(task.Scope, 160), task.Outcome != nil, task.RequiredBranchID})
		}
		payload = struct {
			Entries []index          `json:"entries"`
			Next    uint64           `json:"next"`
			HasMore bool             `json:"has_more"`
			Pump    WorkerPumpStatus `json:"pump"`
		}{entries, page.Next, page.HasMore, t.Service.Teams.Status(a.WorkerID)}
	case "read":
		if a.Sequence == 0 || a.Offset < 0 {
			return wire.ToolResult{}, errors.New("selected sequence and nonnegative UTF-8 offset required")
		}
		page, e := store.WorkerTasks(ctx, t.ProductID, a.TeamID, a.WorkerID, a.Sequence-1, 1)
		if e != nil {
			return wire.ToolResult{}, e
		}
		if len(page.Tasks) != 1 || page.Tasks[0].Sequence != a.Sequence {
			return wire.ToolResult{}, errors.New("owned task not found")
		}
		data, e := json.Marshal(page.Tasks[0])
		if e != nil {
			return wire.ToolResult{}, e
		}
		if a.Offset > len(data) || (a.Offset < len(data) && !utf8.RuneStart(data[a.Offset])) {
			return wire.ToolResult{}, errors.New("offset is not a UTF-8 boundary")
		}
		end := a.Offset + 2048
		if end > len(data) {
			end = len(data)
		}
		for end > a.Offset && end < len(data) && !utf8.RuneStart(data[end]) {
			end--
		}
		payload = struct {
			Text       string `json:"text"`
			Next       int    `json:"next"`
			Total      int    `json:"total"`
			Processing string `json:"processing"`
		}{string(data[a.Offset:end]), end, len(data), "Selected committed product task byte range only; terminal outcome is actual, absent changed-file/verification claims are not success evidence."}
	case "enqueue":
		task, e := store.EnqueueWorker(ctx, t.ProductID, a.TeamID, a.WorkerID, a.Scope, a.Claims)
		err = e
		payload = map[string]any{"task_id": task.ID, "sequence": task.Sequence, "status": task.Status, "processing": "Queued only; call resume explicitly after consent. Do not resend on observation failure."}
	case "edit":
		err = store.EditQueuedTask(ctx, t.ProductID, a.TeamID, a.WorkerID, a.TaskID, a.Scope, a.Claims)
	case "cancel":
		err = store.CancelQueuedTask(ctx, t.ProductID, a.TeamID, a.WorkerID, a.TaskID)
	case "resume":
		err = t.Service.Teams.Resume(ctx, t.ProductID, a.TeamID, a.WorkerID)
	case "stop":
		err = t.Service.Teams.Stop(ctx, t.ProductID, a.TeamID, a.WorkerID)
	case "active", "hibernated", "archived":
		err = store.SetWorkerLifecycle(ctx, t.ProductID, a.TeamID, a.WorkerID, hub.WorkerLifecycle(a.Action))
	case "end":
		err = store.EndTeam(ctx, t.ProductID, a.TeamID)
	default:
		return wire.ToolResult{}, errors.New("unknown Team action")
	}
	if err != nil {
		return wire.ToolResult{}, err
	}
	if payload == nil {
		payload = map[string]string{"processing": "Action admitted; observe actual durable task/Turn outcome. No fabricated completion."}
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return wire.ToolResult{}, err
	}
	if len(encoded) > 8<<10 {
		return wire.ToolResult{}, errors.New("Team result exceeds 8 KiB index budget; narrow the request")
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(encoded)}}}, nil
}

func shortText(text string, limit int) string {
	if len(text) <= limit {
		return text
	}
	text = text[:limit]
	for !utf8.ValidString(text) {
		text = text[:len(text)-1]
	}
	return text + " [excerpt]"
}

type TeamPlugin struct{ Tool *TeamTool }

func (TeamPlugin) Name() string { return "hub-team" }
func (p TeamPlugin) Register(_ context.Context, r *core.Registrar) error {
	if p.Tool == nil {
		return errors.New("bound product Team tool required")
	}
	if err := r.Tool(p.Tool); err != nil {
		return err
	}
	return r.SystemPrompt("hub_team coordinates visible independent Hub Worker sessions, never native SDK children. List small indexes, read only selected task ranges. Creating/queuing does not start workers; explicit resume owns FIFO execution. Actual terminal status is separate from changed-file/test evidence, which may be unreported. No automatic bypass or inheritance of parent logs.")
}
