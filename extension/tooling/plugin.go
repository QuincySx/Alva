// INPUT: bytes, context, encoding/json, errors, io, filepath, reflect, strings, sync, time, utf8, core/security, ABI/bus, process/session.
// OUTPUT: Config, Profile, ProfileInfo, Manager, Request, Report, Plugin and New for real explicit diagnostic command execution.
// POS: [L4 extension] Optional replaceable diagnostics manager has a real Tool consumer, not a fake empty LSP server scaffold.
package tooling

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/platform/system/process"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type Profile struct {
	ID, Description    string
	Process            process.Config
	Timeout            time.Duration
	ApprovedHostAccess bool
}
type ProfileInfo struct {
	ID          string `json:"id"`
	Description string `json:"description"`
}
type Activity struct {
	ID, Name, SessionID, RunID, CallID string
	Effect                             abi.EffectKind
}
type Config struct {
	Repository            *session.Repository
	Owner, SessionID, Cwd string
	Guard                 *security.Guard
	Profiles              []Profile
	Manager               Manager
	Report                func(context.Context, Activity) error
}
type Request struct {
	ProfileID string
	Execution abi.ToolExecutionContext
	Call      wire.ToolCall
}
type Report struct {
	SessionID         string         `json:"session_id"`
	RunID             string         `json:"run_id"`
	CallID            string         `json:"call_id"`
	ProfileID         string         `json:"profile_id"`
	Status            string         `json:"status"`
	ExitCode          int            `json:"exit_code"`
	Started           bool           `json:"started"`
	Reaped            bool           `json:"reaped"`
	CapturedComplete  bool           `json:"captured_complete"`
	DiagnosticsParsed bool           `json:"diagnostics_parsed"`
	Stdout            wire.Reference `json:"stdout"`
	Stderr            wire.Reference `json:"stderr"`
	Error             string         `json:"error,omitempty"`
	SourceNotice      string         `json:"source_notice"`
}
type Manager interface {
	Catalog() []ProfileInfo
	Run(context.Context, Request) (Report, error)
}
type Plugin struct {
	config            Config
	log               *session.Session
	mu                sync.Mutex
	runID             string
	tool              *diagnosticTool
	manager           Manager
	catalog           []ProfileInfo
	gate              chan struct{}
	executions, bytes int
}

func New(ctx context.Context, config Config) (*Plugin, error) {
	if config.Repository == nil || config.Guard == nil || config.Report == nil || config.SessionID == "" || !filepath.IsAbs(config.Cwd) || len(config.Profiles) > 16 {
		return nil, errors.New("explicit diagnostics authority/guard/reporter/cwd and bounded profiles required")
	}
	if config.Manager != nil && len(config.Profiles) > 0 {
		return nil, errors.New("choose an explicit diagnostics manager or native profiles, not ignored settings")
	}
	if config.Manager != nil {
		value := reflect.ValueOf(config.Manager)
		switch value.Kind() {
		case reflect.Pointer, reflect.Map, reflect.Slice, reflect.Func, reflect.Chan, reflect.Interface:
			if value.IsNil() {
				return nil, errors.New("typed nil diagnostics manager")
			}
		}
	}
	log, err := config.Repository.OpenSession(ctx, config.Owner, config.SessionID)
	if err != nil {
		return nil, err
	}
	p := &Plugin{config: config, log: log, gate: make(chan struct{}, 1)}
	profiles := make([]Profile, len(config.Profiles))
	seen := map[string]bool{}
	for index, profile := range config.Profiles {
		if !validInfo(ProfileInfo{ID: profile.ID, Description: profile.Description}) || seen[profile.ID] || !profile.ApprovedHostAccess || profile.Process.Command == "" || profile.Process.Cwd != config.Cwd {
			return nil, errors.New("invalid/duplicate/unapproved diagnostic profile")
		}
		seen[profile.ID] = true
		if profile.Timeout == 0 {
			profile.Timeout = 30 * time.Second
		}
		if profile.Timeout <= 0 || profile.Timeout > 5*time.Minute {
			return nil, errors.New("diagnostic timeout must be within five minutes")
		}
		profile.Process.Args = append([]string(nil), profile.Process.Args...)
		env := map[string]string{}
		for key, value := range profile.Process.Env {
			env[key] = value
		}
		profile.Process.Env = env
		profiles[index] = profile
	}
	p.config.Profiles = profiles
	p.manager = config.Manager
	if p.manager == nil {
		p.manager = &nativeManager{plugin: p}
	}
	p.catalog = append([]ProfileInfo(nil), p.manager.Catalog()...)
	if len(p.catalog) < 1 || len(p.catalog) > 16 {
		return nil, errors.New("at least one explicit real diagnostics profile required")
	}
	seen = map[string]bool{}
	for _, info := range p.catalog {
		if !validInfo(info) || seen[info.ID] {
			return nil, errors.New("invalid diagnostics manager catalog")
		}
		seen[info.ID] = true
	}
	p.tool = &diagnosticTool{plugin: p}
	return p, nil
}
func validInfo(info ProfileInfo) bool {
	if info.ID == "" || len(info.ID) > 128 || len(info.Description) > 512 || !utf8.ValidString(info.Description) {
		return false
	}
	for _, char := range info.ID {
		if !(char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || strings.ContainsRune("._:-", char)) {
			return false
		}
	}
	return true
}
func (*Plugin) Name() string     { return "tooling" }
func (p *Plugin) Tool() abi.Tool { return p.tool }
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	if err := r.Tool(p.tool); err != nil {
		return err
	}
	if err := r.Middleware(p); err != nil {
		return err
	}
	if err := bus.Provide[Manager](r.BusWriter(), p.manager); err != nil {
		return err
	}
	return r.SystemPrompt("project_diagnostics runs only explicitly configured approved command profiles on demand, never auto-detects/starts LSP or scans provider accounts. Commands may compile/write/access the network and require host approval. Results retain actual exit/completeness and same-session stdout/stderr references; use context_read/search for selected evidence. No diagnostic parser is implied: exit0 means the selected command exited0, not proof of zero problems. This is not an attached LSP server and no command is automatically replayed.")
}
func (p *Plugin) OnRunStart(_ context.Context, r abi.RuntimeContext) error {
	if r.State == nil || !p.log.SameAuthority(r.State.Session()) || r.Workspace != p.config.Cwd || r.RunID == "" {
		return errors.New("diagnostics actual session/workspace mismatch")
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID != "" {
		return errors.New("diagnostics manager already active")
	}
	p.runID = r.RunID
	p.executions = 0
	p.bytes = 0
	return nil
}
func (p *Plugin) OnRunEnd(_ context.Context, r abi.RuntimeContext, _ error) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID == r.RunID {
		p.runID = ""
	}
	return nil
}
func (p *Plugin) check(e abi.ToolExecutionContext) error {
	p.mu.Lock()
	active := p.runID
	p.mu.Unlock()
	if active == "" || active != e.RunID || e.SessionID != p.config.SessionID || !p.log.SameAuthority(e.Session) || e.Workspace != p.config.Cwd {
		return errors.New("diagnostics does not belong to actual active native run")
	}
	return nil
}

type diagnosticTool struct{ plugin *Plugin }

func (*diagnosticTool) Name() string { return "project_diagnostics" }
func (*diagnosticTool) Description() string {
	return "List explicit diagnostic profiles, or run one approved command without arbitrary new arguments. Returns truthful exit/output references, not a fake empty LSP result. Does not auto-select binaries, install tools, understand source or claim zero diagnostics."
}
func (*diagnosticTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["action"],"properties":{"action":{"type":"string","enum":["list","run"]},"profile_id":{"type":"string","maxLength":128},"after":{"type":"string","maxLength":128}}}`)
}
func (*diagnosticTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.SerialGlobal, ManagesOwnTimeout: true}
}
func (*diagnosticTool) IsReadOnly(call wire.ToolCall) bool {
	var args struct{ Action string }
	return json.Unmarshal(call.Arguments, &args) == nil && args.Action == "list"
}
func (t *diagnosticTool) DescribeEffect(call wire.ToolCall) abi.Effect {
	kind := abi.EffectUnknown
	if t.IsReadOnly(call) {
		kind = abi.EffectRead
	}
	return abi.Effect{Kind: kind, Description: "Use explicitly bound diagnostic profile catalog or execute its approved native command; no sandbox or no-write/no-network guarantee"}
}
func (t *diagnosticTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	p := t.plugin
	actual, ok := e.ActualTool.(*diagnosticTool)
	if !ok || actual != t || t != p.tool {
		return wire.ToolResult{}, errors.New("diagnostics requires its actual dispatched tool authority")
	}
	if call.ID == "" || e.CallID != call.ID || call.Name != t.Name() {
		return wire.ToolResult{}, errors.New("diagnostics actual tool/call mismatch")
	}
	if err := p.check(e); err != nil {
		return wire.ToolResult{}, err
	}
	args, err := decodeArguments(call.Arguments)
	if err != nil {
		return wire.ToolResult{}, err
	}
	if err := p.config.Guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return wire.ToolResult{}, err
	}
	if args.Action == "list" {
		start := 0
		if args.After != "" {
			found := false
			for index, profile := range p.catalog {
				if profile.ID == args.After {
					start = index + 1
					found = true
					break
				}
			}
			if !found {
				return wire.ToolResult{}, errors.New("unknown diagnostics catalog cursor")
			}
		}
		count := min(len(p.catalog)-start, 5)
		var data []byte
		for {
			next := ""
			if count > 0 {
				next = p.catalog[start+count-1].ID
			}
			data, _ = json.Marshal(map[string]any{"profiles": p.catalog[start : start+count], "next": next, "has_more": start+count < len(p.catalog), "automatic_detection": false})
			if len(data) <= 4096 {
				break
			}
			count--
			if count < 1 {
				return wire.ToolResult{}, errors.New("diagnostics catalog entry exceeds output budget")
			}
		}
		return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, nil
	}
	found := false
	for _, profile := range p.catalog {
		if profile.ID == args.ProfileID {
			found = true
			break
		}
	}
	if !found {
		return wire.ToolResult{}, errors.New("unknown configured diagnostic profile")
	}
	report, err := p.manager.Run(ctx, Request{ProfileID: args.ProfileID, Execution: e, Call: wire.CloneToolCall(call)})
	err = errors.Join(err, ctx.Err(), p.check(e))
	if report.SessionID != e.SessionID || report.RunID != e.RunID || report.CallID != call.ID || report.ProfileID != args.ProfileID {
		return wire.ToolResult{}, errors.Join(err, errors.New("diagnostics manager returned foreign execution evidence"))
	}
	if err == nil && (!report.Started || !report.Reaped || !report.CapturedComplete || report.Stdout.URI == "" || report.Stderr.URI == "" || report.SourceNotice == "" || !(report.Status == "completed" && report.ExitCode == 0 || report.Status == "nonzero_exit" && report.ExitCode != 0)) {
		return wire.ToolResult{}, errors.New("diagnostics manager returned incomplete or contradictory success evidence")
	}
	if err == nil {
		for _, ref := range []wire.Reference{report.Stdout, report.Stderr} {
			if ref.Validate() != nil {
				return wire.ToolResult{}, errors.New("diagnostics manager returned invalid source reference")
			}
			info, sourceErr := p.config.Repository.ArtifactMetadata(ctx, p.config.Owner, p.config.SessionID, ref.URI)
			if sourceErr != nil {
				return wire.ToolResult{}, errors.Join(errors.New("diagnostics manager source is outside actual native authority"), sourceErr)
			}
			if info.Reference() != ref {
				return wire.ToolResult{}, errors.New("diagnostics manager source metadata differs from retained evidence")
			}
		}
	}
	data, encodeErr := json.Marshal(report)
	if encodeErr != nil || len(data) > 4096 {
		return wire.ToolResult{}, errors.Join(err, encodeErr, errors.New("diagnostics evidence metadata exceeds 4 KiB"))
	}
	return wire.ToolResult{CallID: call.ID, IsError: err != nil || report.ExitCode != 0, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, err
}
