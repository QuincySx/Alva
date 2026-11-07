// INPUT: Program-bound model routes, optional revision-bound target source and explicit developer requests.
// OUTPUT: Cold plans and a single shared, cancelable developer test/evaluation job with bounded reports.
// POS: [application developer tools] Owns model requests independently of chat sessions; observers never create or stop jobs.
package developer

import (
	"context"
	"encoding/json"
	"errors"
	"sync"
	"time"

	"github.com/QuincySx/Alva/integration/llm/eval"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// Route is supplied by application assembly. Credentials remain in the bound
// model; a UI cannot retarget a configured key to another URL or model.
type Route struct {
	ID       string
	Protocol provider.Kind
	Provider provider.Config
}

// TargetSource supplies cold, redacted metadata and freezes the exact selected
// revisions before any request starts. Resolve must honor cancellation; its
// private routes never enter a plan, snapshot or report.
type TargetSource interface {
	Targets() []eval.Target
	Resolve(context.Context, []eval.Target) ([]Route, error)
}
type Request struct {
	Mode            Mode     `json:"mode"`
	Targets         []string `json:"targets"`
	ToolNames       []string `json:"tool_names,omitempty"`
	Dataset         *Dataset `json:"dataset,omitempty"`
	Repeats         int      `json:"repeats"`
	MaxCalls        int      `json:"max_calls"`
	TimeoutMS       int      `json:"timeout_ms"`
	MaxOutputTokens uint32   `json:"max_output_tokens"`
}
type Snapshot struct {
	ID           uint64 `json:"id"`
	State        string `json:"state"`
	StartedCalls int    `json:"started_calls"`
	Report       Report `json:"report"`
}
type boundTarget struct {
	target eval.Target
	model  abi.LanguageModel
}
type prepared struct {
	request Request
	targets []boundTarget
	cases   []eval.Case
	options eval.Options
	plan    Report
}
type Service struct {
	mu       sync.Mutex
	targets  []boundTarget
	source   TargetSource
	closed   bool
	snapshot Snapshot
	cancel   context.CancelFunc
	done     chan struct{}
}

func NewService(routes []Route) (*Service, error) {
	if len(routes) > 8 {
		return nil, errors.New("at most eight explicit developer model routes")
	}
	s := &Service{snapshot: Snapshot{State: "idle"}}
	seen := map[string]bool{}
	for _, r := range routes {
		if !identifier(r.ID) || seen[r.ID] {
			return nil, errors.New("unique explicit developer route IDs required")
		}
		seen[r.ID] = true
		model, err := provider.Build(r.Protocol, r.Provider)
		if err != nil {
			return nil, errors.New("invalid developer provider route")
		}
		s.targets = append(s.targets, boundTarget{target: eval.Target{ID: r.ID, Protocol: string(r.Protocol), Model: r.Provider.Model}, model: model})
	}
	return s, nil
}

// NewServiceWithSource adds application-owned dynamic routes without changing
// static route construction or letting a caller supply credentials in Request.
func NewServiceWithSource(routes []Route, source TargetSource) (*Service, error) {
	s, err := NewService(routes)
	if err != nil {
		return nil, err
	}
	s.source = source
	return s, nil
}

func (s *Service) available() []boundTarget {
	result := append([]boundTarget(nil), s.targets...)
	if s.source != nil {
		for _, target := range s.source.Targets() {
			result = append(result, boundTarget{target: target})
		}
	}
	return result
}
func (s *Service) Targets() []eval.Target {
	s.mu.Lock()
	defer s.mu.Unlock()
	available := s.available()
	result := make([]eval.Target, 0, len(available))
	for _, target := range available {
		result = append(result, target.target)
	}
	return result
}

func (s *Service) prepare(request Request) (prepared, error) {
	data, err := json.Marshal(request)
	if err != nil || len(data) > 128<<10 {
		return prepared{}, errors.New("bounded explicit developer request required")
	}
	if err = json.Unmarshal(data, &request); err != nil {
		return prepared{}, err
	}
	if request.MaxCalls == 0 {
		request.MaxCalls = 32
	}
	if request.TimeoutMS == 0 {
		request.TimeoutMS = 30000
	}
	if request.TimeoutMS < 1 || request.TimeoutMS > 120000 {
		return prepared{}, errors.New("developer timeout must be 1..120000 ms")
	}
	if request.Repeats == 0 {
		request.Repeats = 1
		if request.Mode == Evaluate {
			request.Repeats = 3
		}
	}
	if request.MaxOutputTokens == 0 {
		request.MaxOutputTokens = 2048
	}
	if request.MaxOutputTokens > 16384 || len(request.Targets) == 0 || len(request.Targets) > 8 || len(request.ToolNames) > 8 {
		return prepared{}, errors.New("bounded selected models, names and output tokens required")
	}
	seenNames := map[string]bool{}
	for _, name := range request.ToolNames {
		if !portableToolName(name) || seenNames[name] {
			return prepared{}, errors.New("unique portable question tool names required")
		}
		seenNames[name] = true
	}
	cases, err := buildCases(request.Mode, request.ToolNames, request.MaxOutputTokens, request.Dataset)
	if err != nil {
		return prepared{}, err
	}
	p := prepared{request: request, cases: cases, options: eval.Options{Repeats: request.Repeats, MaxCalls: request.MaxCalls, Timeout: time.Duration(request.TimeoutMS) * time.Millisecond}, plan: Report{Version: 1, Kind: request.Mode, Mode: "plan", Reports: []eval.Report{}}}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return prepared{}, errors.New("developer service is closed")
	}
	seen := map[string]bool{}
	available := s.available()
	for _, id := range request.Targets {
		if seen[id] {
			return prepared{}, errors.New("duplicate developer target")
		}
		seen[id] = true
		found := false
		for _, target := range available {
			if target.target.ID != id {
				continue
			}
			if found {
				return prepared{}, errors.New("ambiguous developer target")
			}
			found = true
			report, e := eval.Plan(target.target, cases, p.options)
			if e != nil {
				return prepared{}, e
			}
			p.targets = append(p.targets, target)
			p.plan.Reports = append(p.plan.Reports, report)
			p.plan.TotalPlannedCalls += report.PlannedCalls
		}
		if !found {
			return prepared{}, errors.New("select an explicitly configured model route")
		}
	}
	if p.plan.TotalPlannedCalls > request.MaxCalls {
		return prepared{}, errors.New("complete developer matrix exceeds call budget")
	}
	return p, nil
}
func (s *Service) Plan(request Request) (Report, error) {
	p, err := s.prepare(request)
	if err != nil {
		return Report{}, err
	}
	return finalizeOutput(request.Mode, p.request.Dataset, p.plan), nil
}

// Start is the explicit operator execution boundary. Construction, Targets,
// Plan and Snapshot are cold. At most one job can use these routes at a time.
// It returns after admission; route freezing and HTTP share the asynchronous
// job owner, and a failed freeze is visible as a failed or canceled Snapshot.
func (s *Service) Start(request Request) error {
	p, err := s.prepare(request)
	if err != nil {
		return err
	}
	s.mu.Lock()
	if s.closed || s.cancel != nil {
		s.mu.Unlock()
		return errors.New("developer work is already running or service is closed")
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.cancel = cancel
	s.done = make(chan struct{})
	s.snapshot = Snapshot{ID: s.snapshot.ID + 1, State: "running", Report: finalizeOutput(request.Mode, p.request.Dataset, p.plan)}
	s.mu.Unlock()
	go s.execute(ctx, p)
	return nil
}

func (s *Service) bind(ctx context.Context, p *prepared) error {
	var selected []eval.Target
	for _, target := range p.targets {
		if target.model == nil {
			selected = append(selected, target.target)
		}
	}
	if len(selected) > 0 {
		routes, err := s.source.Resolve(ctx, selected)
		if err != nil || len(routes) != len(selected) {
			return errors.New("selected developer routes changed")
		}
		for i, route := range routes {
			target := eval.Target{ID: route.ID, Protocol: string(route.Protocol), Model: route.Provider.Model}
			if target != selected[i] {
				return errors.New("selected developer route identity changed")
			}
			model, err := provider.Build(route.Protocol, route.Provider)
			if err != nil {
				return errors.New("invalid developer provider route")
			}
			for j := range p.targets {
				if p.targets[j].target == target {
					p.targets[j].model = model
				}
			}
		}
	}
	return ctx.Err()
}

type observedModel struct {
	abi.LanguageModel
	started func()
}

func (m observedModel) Stream(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	m.started()
	return m.LanguageModel.Stream(ctx, r, emit)
}
func (s *Service) execute(ctx context.Context, p prepared) {
	result := p.plan
	result.Mode = "explicit-run"
	state := "completed"
	if err := s.bind(ctx, &p); err != nil {
		state = "failed"
		if ctx.Err() != nil {
			state = "canceled"
		}
	} else {
		result.Reports = nil
		for _, target := range p.targets {
			model := observedModel{target.model, func() { s.mu.Lock(); s.snapshot.StartedCalls++; s.mu.Unlock() }}
			report, err := eval.Run(ctx, model, target.target, p.cases, p.options)
			result.Reports = append(result.Reports, report)
			if report.Errors > 0 || p.request.Mode == Test && report.Failed > 0 {
				state = "completed_with_failures"
			}
			if err != nil {
				state = "canceled"
				break
			}
		}
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.snapshot.State = state
	s.snapshot.Report = finalizeOutput(p.request.Mode, p.request.Dataset, result)
	s.cancel()
	s.cancel = nil
	close(s.done)
}
func (s *Service) Snapshot() Snapshot {
	s.mu.Lock()
	defer s.mu.Unlock()
	data, _ := json.Marshal(s.snapshot)
	var snapshot Snapshot
	_ = json.Unmarshal(data, &snapshot)
	return snapshot
}
func (s *Service) stop(ctx context.Context, closeService bool) error {
	s.mu.Lock()
	if closeService {
		s.closed = true
	}
	cancel, done := s.cancel, s.done
	s.mu.Unlock()
	if cancel == nil {
		return nil
	}
	cancel()
	select {
	case <-done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
func (s *Service) Cancel(ctx context.Context) error { return s.stop(ctx, false) }
func (s *Service) Close(ctx context.Context) error  { return s.stop(ctx, true) }
