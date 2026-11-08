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
	t := c.Theme()
	settingsPageTitle(c, "开发者", "")
	ui.Row(c).Gap(8).Children(func() {
		kind := func(mode developer.Mode) string {
			if p.developer.mode == mode {
				return "primary"
			}
			return "secondary"
		}
		workspaceControlButton(c, "developer-mode-test", "接入测试", kind(developer.Test)).Label("Integration tests").OnClick(func() {
			p.developer.mode = developer.Test
			p.developer.repeats = "1"
			p.developer.planBinding = ""
			p.developer.planText = ""
		})
		workspaceControlButton(c, "developer-mode-evaluate", "模型评估", kind(developer.Evaluate)).Label("Model evaluation").OnClick(func() {
			p.developer.mode = developer.Evaluate
			p.developer.repeats = "3"
			p.developer.planBinding = ""
			p.developer.planText = ""
		})
	})
	p.buildDeveloperDebug(c)
	if p.Service.Developer == nil {
		ui.Text(c, "需要先配置可用的模型连接才能使用接入测试或模型评估。打开本页不会调用模型。").FontSize(workspaceFontBody).TextColor(t.TextMuted).Wrap()
		return
	}
	if len(p.Service.Developer.Targets()) == 0 {
		ui.Text(c, "需要先配置可用的模型连接才能使用接入测试或模型评估。打开本页不会调用模型。").FontSize(workspaceFontBody).TextColor(t.TextMuted).Wrap()
		if p.developer.snapshot.ID == 0 {
			return
		}
	}
	if p.developer.mode == developer.Test {
		ui.Text(c, "接入测试检查模型在一次回复里是否选对工具、填对参数；它不评价模型质量，也不验证多轮权限。").FontSize(workspaceFontCaption).TextColor(t.TextMuted).Wrap()
	} else {
		ui.Text(c, "模型评估在一组带版本的任务上重复运行并打分。分数只代表这组任务；执行错误和覆盖不全单独报告。").FontSize(workspaceFontCaption).TextColor(t.TextMuted).Wrap()
	}
	ui.Text(c, "运行会使用所选模型账户并可能产生费用；不会读取聊天记录，也不会执行本机工具。").FontSize(workspaceFontCaption).TextColor(t.Warning).Wrap()
	if p.developer.problem != "" {
		ui.Text(c, p.developer.problem).Role(ui.RoleStatus).TextColor(t.Danger).Wrap()
	}
	running := p.developer.snapshot.State == "running"
	ui.Row(c).Gap(8).Children(func() {
		workspaceControlButton(c, "developer-plan", "预览计划", "secondary", running).Label("Preview developer plan").OnClick(p.planDeveloper)
		label, text := "Run integration tests", "运行接入测试"
		if p.developer.mode == developer.Evaluate {
			label, text = "Run model evaluation", "运行模型评估"
		}
		workspaceControlButton(c, "developer-run", text, "primary", running || p.developer.planBinding == "").Label(label).OnClick(p.runDeveloper)
		workspaceControlButton(c, "developer-stop", "停止", "danger", !running || p.developer.stopping).Label("Stop developer run").OnClick(p.cancelDeveloper)
	})
	if p.developer.snapshot.ID > 0 {
		ui.Text(c, fmt.Sprintf("第 %d 次运行：%s · 已发出请求 %d/%d", p.developer.snapshot.ID, p.developer.snapshot.State, p.developer.snapshot.StartedCalls, p.developer.snapshot.Report.TotalPlannedCalls)).FontSize(workspaceFontBody)
		if p.developer.snapshot.State == "failed" {
			ui.Text(c, "无法固定所选模型路线。请刷新连接设置并重新预览后再运行。").FontSize(workspaceFontBody).TextColor(t.Danger).Wrap()
		}
	}
	ui.Scroll(c.Key("developer-content")).Label("Developer details").Grow(1).MinHeight(0).Children(func() {
		ui.Column(c).FillWidth().Gap(8).Children(func() {
			ui.Text(c, "模型路线").FontSize(workspaceFontSubtitle).FontWeight(600)
			for _, target := range p.Service.Developer.Targets() {
				target := target
				selected := p.developer.targets[target.ID]
				workspaceControlCheckbox(c, "developer-target-"+target.ID, &selected, target.ID+" · "+target.Protocol+" · "+target.Model).OnChange(func() {
					p.developer.targets[target.ID] = selected
					p.developer.planBinding = ""
				})
			}
			ui.Row(c).Gap(8).Children(func() {
				for _, field := range []struct {
					key, title, label string
					value             *string
				}{
					{"developer-repeats", "每个用例的次数", "Trials per case", &p.developer.repeats},
					{"developer-max-calls", "总请求上限", "Total call budget", &p.developer.maxCalls},
					{"developer-timeout", "超时（毫秒）", "Timeout (ms)", &p.developer.timeout},
					{"developer-tokens", "输出 token 上限", "Output tokens", &p.developer.tokens},
				} {
					field := field
					ui.Column(c).Gap(4).Children(func() {
						ui.Text(c, field.title).FontSize(workspaceFontCaption)
						workspaceControlInput(c, field.key, field.value).Label(field.label).Width(140).Disabled(running)
					})
				}
			})
			if p.developer.mode == developer.Test {
				ui.Text(c, "提问工具名称（逗号分隔）").FontSize(workspaceFontCaption)
				workspaceControlInput(c, "developer-names", &p.developer.names).Label("Question tool names (comma-separated)").FillWidth().Disabled(running)
			} else {
				ui.Text(c, "评估任务集（带版本的 JSON）").FontSize(workspaceFontCaption)
				workspaceControlTextArea(c, "developer-dataset", &p.developer.dataset).Label("Versioned evaluation task set JSON").Height(200).FillWidth().Disabled(running)
			}
			if p.developer.planText != "" {
				ui.Text(c, "计划预览（尚未发出请求）").FontSize(workspaceFontCaption)
				workspaceControlTextArea(c, "developer-plan-text", &p.developer.planText).Label("Cold plan — no requests sent").ReadOnly(true).Height(140).FillWidth()
			}
			if p.developer.reportText != "" {
				for _, score := range p.developer.snapshot.Report.Scores {
					if score.WeightedScore != nil {
						ui.Text(c, fmt.Sprintf("%s · 任务得分 %.1f%% · 稳定通过 %d/%d · 已评分 %d · 执行错误 %d", score.TargetID, *score.WeightedScore, score.ConsistentTaskPasses, score.TotalTasks, score.GradedTrials, score.ExecutionErrors)).FontSize(workspaceFontBody)
					}
				}
				ui.Text(c, "最新报告").FontSize(workspaceFontCaption)
				workspaceControlTextArea(c, "developer-report", &p.developer.reportText).Label("Latest developer report JSON").ReadOnly(true).Height(240).FillWidth()
			}
		})
	})
}

// buildDeveloperDebug collects the task debugging views that ordinary settings
// pages no longer show. Each opens the existing read-only owner.
func (p *NativePage) buildDeveloperDebug(c *ui.Context) {
	settingsGroup(c, "developer-debug", "调试", "查看所选任务的执行记录、等待中的检查点、路由与交接。只读，不会启动 Agent。", func() {
		if p.selected == "" || !p.workspaceNavigationDetailReady() {
			ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Text(c, "先选择一个任务。").FontSize(workspaceFontBody).TextColor(c.Theme().TextMuted)
			})
			return
		}
		ui.Row(c).Gap(8).Children(func() {
			workspaceControlIconButton(c, "developer-debug-records", "会话记录与导出", "file-text", "secondary", p.paneBusy).Label("View and export session records").OnClick(p.openInspector)
			workspaceControlIconButton(c, "developer-debug-waits", "等待中的任务", "clock", "secondary", p.paneBusy).Label("Open waiting tasks").OnClick(p.openWaits)
			workspaceControlIconButton(c, "developer-debug-routing", "路由与故障切换", "git-branch", "secondary", p.paneBusy).Label("Open routing and fallback").OnClick(func() { p.setPane("routing") })
			workspaceControlIconButton(c, "developer-debug-handoffs", "交接记录", "list", "secondary", p.paneBusy).Label("Open handoff records").OnClick(func() { p.setPane("handoffs"); p.handoffText = "" })
		})
	})
}
