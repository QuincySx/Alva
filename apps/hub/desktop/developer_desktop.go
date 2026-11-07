// INPUT: Shared developer job owner, explicitly configured model targets and native control drafts.
// OUTPUT: Separate integration-test and model-evaluation pages, cold plans, explicit Run/Cancel and bounded reports.
// POS: [L7 Hub UI] Application-level developer options work without a chat session and never execute on observation.
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/json"
	"fmt"
	"strconv"
	"strings"
	"time"

	"github.com/QuincySx/Alva/apps/developer"
	"github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/egoist/mygo/ui"
)

type developerPane struct {
	mode                                               developer.Mode
	targets                                            map[string]bool
	names, repeats, maxCalls, timeout, tokens, dataset string
	plan                                               developer.Report
	planBinding                                        string
	targetBinding                                      string
	planText                                           string
	snapshot                                           developer.Snapshot
	reportText                                         string
	problem                                            string
	stopping                                           bool
}

func (p *NativePage) openDeveloper() {
	p.setPane("developer")
	if p.developer.mode == "" {
		data, _ := json.MarshalIndent(developer.DefaultDataset(), "", "  ")
		p.developer = developerPane{mode: developer.Test, targets: map[string]bool{}, names: "ask_human,AskUserQuestion,request_user_input", repeats: "1", maxCalls: "32", timeout: "30000", tokens: "2048", dataset: string(data)}
		if p.Service.Developer != nil {
			targets := p.Service.Developer.Targets()
			if len(targets) > 0 {
				p.developer.targets[targets[0].ID] = true
			}
		}
	}
	p.pollDeveloper()
}
func (p *NativePage) pollDeveloper() {
	if p.pane != "developer" || p.Service.Developer == nil {
		return
	}
	p.refreshDeveloperTargets()
	snapshot := p.Service.Developer.Snapshot()
	if snapshot.ID != p.developer.snapshot.ID || snapshot.State != p.developer.snapshot.State {
		data, _ := json.MarshalIndent(snapshot.Report, "", "  ")
		p.developer.reportText = string(data)
	}
	p.developer.snapshot = snapshot
}

func (p *NativePage) refreshDeveloperTargets() {
	targets := p.Service.Developer.Targets()
	data, _ := json.Marshal(targets)
	binding := string(data)
	if p.developer.targetBinding != "" && binding != p.developer.targetBinding {
		available := make(map[string]bool, len(targets))
		for _, target := range targets {
			available[target.ID] = true
		}
		for id := range p.developer.targets {
			if !available[id] {
				delete(p.developer.targets, id)
			}
		}
		p.developer.planBinding = ""
		p.developer.planText = ""
	}
	p.developer.targetBinding = binding
}
func (p *NativePage) developerRequest() (developer.Request, string, error) {
	p.refreshDeveloperTargets()
	r := developer.Request{Mode: p.developer.mode}
	for _, target := range p.Service.Developer.Targets() {
		if p.developer.targets[target.ID] {
			r.Targets = append(r.Targets, target.ID)
		}
	}
	for _, field := range []struct {
		text  string
		value *int
	}{{p.developer.repeats, &r.Repeats}, {p.developer.maxCalls, &r.MaxCalls}, {p.developer.timeout, &r.TimeoutMS}} {
		n, err := strconv.Atoi(field.text)
		if err != nil {
			return r, "", fmt.Errorf("use explicit integer budgets")
		}
		*field.value = n
	}
	n, err := strconv.ParseUint(p.developer.tokens, 10, 32)
	if err != nil {
		return r, "", fmt.Errorf("use an explicit output token budget")
	}
	r.MaxOutputTokens = uint32(n)
	if r.Mode == developer.Test {
		for _, name := range strings.Split(p.developer.names, ",") {
			r.ToolNames = append(r.ToolNames, strings.TrimSpace(name))
		}
	} else {
		var dataset developer.Dataset
		data := []byte(p.developer.dataset)
		if len(data) > 64<<10 || strictjson.ValidateObject(data, &dataset) != nil || json.Unmarshal(data, &dataset) != nil {
			return r, "", fmt.Errorf("invalid versioned task-set JSON; at most 64 KiB")
		}
		r.Dataset = &dataset
	}
	data, _ := json.Marshal(r)
	return r, string(data), nil
}
func (p *NativePage) planDeveloper() {
	request, binding, err := p.developerRequest()
	if err == nil {
		p.developer.plan, err = p.Service.Developer.Plan(request)
	}
	if err != nil {
		p.developer.problem = err.Error()
		p.developer.planBinding = ""
		return
	}
	p.developer.problem = ""
	p.developer.planBinding = binding
	data, _ := json.MarshalIndent(p.developer.plan, "", "  ")
	p.developer.planText = string(data)
}
func (p *NativePage) runDeveloper() {
	request, binding, err := p.developerRequest()
	if err == nil && (p.developer.planBinding == "" || binding != p.developer.planBinding) {
		p.developer.problem = "Preview the current plan before running; edited options invalidate the preview."
		return
	}
	if err == nil {
		err = p.Service.Developer.Start(request)
	}
	if err != nil {
		p.developer.problem = err.Error()
		return
	}
	p.developer.problem = ""
	p.developer.planBinding = ""
	p.pollDeveloper()
}
func (p *NativePage) cancelDeveloper() {
	if p.developer.stopping {
		return
	}
	p.developer.stopping = true
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		err := p.Service.Developer.Cancel(ctx)
		p.dispatch(func() {
			p.developer.stopping = false
			if err != nil {
				p.developer.problem = "Cancellation is still joining the actual model request; retry Stop."
			}
			p.pollDeveloper()
		})
	}()
}
func (p *NativePage) buildDeveloper(c *ui.Context) {
	ui.Text(c, "Developer options").Bold().FontSize(20)
	ui.Row(c).Gap(8).Children(func() {
		ui.Button(c, "Integration tests").OnClick(func() {
			p.developer.mode = developer.Test
			p.developer.repeats = "1"
			p.developer.planBinding = ""
			p.developer.planText = ""
		})
		ui.Button(c, "Model evaluation").OnClick(func() {
			p.developer.mode = developer.Evaluate
			p.developer.repeats = "3"
			p.developer.planBinding = ""
			p.developer.planText = ""
		})
	})
	if p.Service.Developer == nil {
		ui.Text(c, "Configure a live model route to use developer tests or evaluation. Opening this page does not call a model.")
		return
	}
	if len(p.Service.Developer.Targets()) == 0 {
		ui.Text(c, "Configure a live model route to use developer tests or evaluation. Opening this page does not call a model.")
		if p.developer.snapshot.ID == 0 {
			return
		}
	}
	if p.developer.mode == developer.Test {
		ui.Text(c, "Integration tests verify tool selection and parameters in one response. They do not rank model quality or prove multi-turn permissions.").FontSize(workspaceFontCaption)
	} else {
		ui.Text(c, "Evaluation scores an explicit versioned task set over repeated trials. Scores cover these tasks only. Execution errors and incomplete coverage are reported separately.").FontSize(workspaceFontCaption)
	}
	ui.Text(c, "Run uses the selected model accounts and can incur inference charges. No chat history or host tools are executed.").FontSize(workspaceFontCaption)
	if p.developer.problem != "" {
		ui.Text(c, p.developer.problem)
	}
	running := p.developer.snapshot.State == "running"
	ui.Row(c).Gap(8).Children(func() {
		ui.Button(c, "Preview developer plan").Disabled(running).OnClick(p.planDeveloper)
		label := "Run integration tests"
		if p.developer.mode == developer.Evaluate {
			label = "Run model evaluation"
		}
		ui.Button(c, label).Disabled(running || p.developer.planBinding == "").OnClick(p.runDeveloper)
		ui.Button(c, "Stop developer run").Disabled(!running || p.developer.stopping).OnClick(p.cancelDeveloper)
	})
	if p.developer.snapshot.ID > 0 {
		ui.Text(c, fmt.Sprintf("Developer run %d: %s · started requests %d/%d", p.developer.snapshot.ID, p.developer.snapshot.State, p.developer.snapshot.StartedCalls, p.developer.snapshot.Report.TotalPlannedCalls))
		if p.developer.snapshot.State == "failed" {
			ui.Text(c, "Selected model routes could not be frozen. Refresh the connection settings and preview current targets before running again.")
		}
	}
	ui.Scroll(c.Key("developer-content")).Label("Developer details").Grow(1).MinHeight(0).Children(func() {
		ui.Column(c).Gap(8).Children(func() {
			for _, target := range p.Service.Developer.Targets() {
				prefix := "[ ] "
				if p.developer.targets[target.ID] {
					prefix = "[x] "
				}
				ui.Button(c, prefix+target.ID+" · "+target.Protocol+" · "+target.Model).Disabled(running).OnClick(func() { p.developer.targets[target.ID] = !p.developer.targets[target.ID]; p.developer.planBinding = "" })
			}
			ui.Row(c).Gap(8).Children(func() {
				ui.TextInput(c.Key("developer-repeats"), &p.developer.repeats).Label("Trials per case").Width(120).Disabled(running)
				ui.TextInput(c.Key("developer-max-calls"), &p.developer.maxCalls).Label("Total call budget").Width(120).Disabled(running)
				ui.TextInput(c.Key("developer-timeout"), &p.developer.timeout).Label("Timeout (ms)").Width(120).Disabled(running)
				ui.TextInput(c.Key("developer-tokens"), &p.developer.tokens).Label("Output tokens").Width(120).Disabled(running)
			})
			if p.developer.mode == developer.Test {
				ui.TextInput(c.Key("developer-names"), &p.developer.names).Label("Question tool names (comma-separated)").Disabled(running)
			} else {
				ui.TextArea(c.Key("developer-dataset"), &p.developer.dataset).Label("Versioned evaluation task set JSON").Height(200).Disabled(running)
			}
			if p.developer.planText != "" {
				ui.TextArea(c.Key("developer-plan"), &p.developer.planText).Label("Cold plan — no requests sent").ReadOnly(true).Height(140)
			}
			if p.developer.reportText != "" {
				for _, score := range p.developer.snapshot.Report.Scores {
					if score.WeightedScore != nil {
						ui.Text(c, fmt.Sprintf("%s · %.1f%% task score · consistent passes %d/%d · graded %d · execution errors %d", score.TargetID, *score.WeightedScore, score.ConsistentTaskPasses, score.TotalTasks, score.GradedTrials, score.ExecutionErrors))
					}
				}
				ui.TextArea(c.Key("developer-report"), &p.developer.reportText).Label("Latest developer report JSON").ReadOnly(true).Height(240)
			}
		})
	})
}
