// INPUT: Actual direct native session/page/composer/approval/Inspector state and MyGo controls
// OUTPUT: PersistentPage.Build with bounded session/history/typed-record views and explicit user actions
// POS: [L6 application] Native persistent direct presentation never becomes the conversation fact authority.
//go:build desktop

package desktop

import (
	"fmt"
	"github.com/egoist/mygo/ui"
)

func (p *PersistentPage) Build(c *ui.Context) {
	if p.diagnosticFrame != nil {
		defer p.diagnosticFrame()
	}
	for range 8 {
		select {
		case update := <-p.updates:
			update()
		default:
			goto ready
		}
	}
ready:
	ui.Row(c).Fill().Padding(12).Gap(12).Children(func() {
		ui.Column(c).Width(250).FillHeight().Gap(8).Children(func() {
			ui.Text(c, "ALVA · Direct").Role(ui.RoleHeading).Level(1).Bold().FontSize(21)
			ui.Text(c, "Native durable sessions · direct harness").FontSize(12)
			ui.TextInput(c.Key("direct-title"), &p.title).Label("Session title").Placeholder("Session title")
			ui.Button(c, "New session").Disabled(p.busy).OnClick(p.create)
			ui.Button(c, "Refresh sessions").Disabled(p.listBusy).OnClick(func() { p.sessionBack = nil; p.refreshSessions("") })
			ui.Scroll(c).Grow(1).MinHeight(0).Gap(6).Children(func() {
				for _, row := range p.rows.Sessions {
					row := row
					label := row.Title
					if row.ActiveRunID != "" {
						label = "Running · " + label
					}
					ui.Button(c.Key(row.ID), label).Disabled(p.busy || !row.MetadataAvailable).OnClick(func() { p.selectSession(row.ID) })
				}
			})
			ui.Row(c).Gap(6).Children(func() {
				ui.Button(c, "Previous sessions").Disabled(p.listBusy || len(p.sessionBack) == 0).OnClick(func() {
					last := len(p.sessionBack) - 1
					after := p.sessionBack[last]
					p.sessionBack = p.sessionBack[:last]
					p.refreshSessions(after)
				})
				ui.Button(c, "More sessions").Disabled(p.listBusy || !p.rows.HasMore || len(p.sessionBack) >= 64).OnClick(func() { p.sessionBack = append(p.sessionBack, p.sessionAfter); p.refreshSessions(p.rows.Next) })
			})
			ui.Text(c, "Session history stays in this new Go data directory. No old Rust/CLI/Hub history is imported.").FontSize(11)
		})
		ui.Column(c).Grow(1).FillHeight().MinWidth(0).Gap(8).Children(func() {
			mode := "LIVE · " + p.Service.RouteLabel()
			if p.Service.Demo() {
				mode = "DEMO · local model/tool fixture, no service billing"
			}
			ui.Text(c, mode).FontSize(12)
			if p.problem != "" {
				ui.Text(c, "Problem: "+p.problem).FontSize(12)
			}
			if p.selected == "" {
				ui.Text(c, "Create or select a durable direct session. Nothing executes on startup.").FontSize(18)
				return
			}
			ui.Text(c, p.meta.Title).Role(ui.RoleHeading).Level(2).Bold().FontSize(18)
			ui.Text(c, "Native session "+p.selected).FontSize(11)
			if p.meta.Workspace != "" {
				ui.Text(c, "Saved project: "+p.meta.Workspace).FontSize(11)
			} else {
				ui.Text(c, "No workspace file tools granted. Attachments remain explicit references.").FontSize(11)
			}
			p.buildDirectApprovals(c)
			ui.Row(c).Gap(8).Children(func() {
				ui.Button(c, "Chat").OnClick(func() { p.changed(); p.pane = "" })
				ui.Button(c, "Inspector").Disabled(p.inspectorBusy).OnClick(func() { p.inspectAfter = nil; p.loadInspector(0, 0) })
			})
			if p.pane == "inspector" {
				p.buildDirectInspector(c)
				return
			}
			ui.Row(c).Gap(8).Children(func() {
				ui.Button(c, "Earlier records").Disabled(p.busy || !p.history.HasEarlier).OnClick(func() { p.loadHistory(p.history.After + 1) })
				ui.Button(c, "Latest records").Disabled(p.busy).OnClick(func() { p.loadHistory(0) })
				ui.Button(c, "Attach files").Disabled(p.busy || p.model.ActiveRunID != "" || p.window == nil).OnClick(p.upload)
			})
			for _, artifact := range p.attachments {
				artifact := artifact
				ui.Row(c).Gap(8).Children(func() {
					ui.Text(c, fmt.Sprintf("File: %s · %d bytes · saved original, not automatically model-delivered", artifact.Name, artifact.Bytes)).FontSize(11).Grow(1)
					ui.Button(c.Key(artifact.ID), "Remove draft reference").Disabled(p.busy || p.model.ActiveRunID != "").OnClick(func() { p.changed(); p.removeAttachment(artifact.ID) })
				})
			}
			ui.Text(c, p.model.StatusText()).Role(ui.RoleStatus).FontSize(12)
			if p.model.Error != "" {
				ui.Text(c, "Run error: "+p.model.Error).FontSize(12)
			}
			if p.model.Notice != "" {
				ui.Text(c, p.model.Notice).FontSize(11)
			}
			p.chat.BuildChat(c)
		})
	})
}
func (p *PersistentPage) buildDirectApprovals(c *ui.Context) {
	shown := p.approvals
	if len(shown) > 1 {
		ui.Text(c, fmt.Sprintf("%d actual permissions pending; remaining calls stay blocked.", len(shown))).FontSize(11)
		shown = shown[:1]
	}
	for _, request := range shown {
		request := request
		ui.Column(c).Gap(4).Children(func() {
			ui.Text(c, "Permission required: "+request.ToolName+" · "+string(request.Effect.Kind)).Bold()
			ui.Text(c, request.Effect.Description).FontSize(12)
			for _, resource := range request.Effect.Resources {
				ui.Text(c, string(resource.Access)+": "+resource.Resource).FontSize(11)
			}
			summary := request.InputSummary
			ui.TextArea(c.Key("direct-request-"+request.ID), &summary).Label("Requested input — review before allowing").ReadOnly(true).Height(90)
			ui.Text(c, "Actual native session "+request.SessionID+", call "+request.CallID+". One call only.").FontSize(11)
			ui.Row(c).Gap(8).Children(func() {
				ui.Button(c.Key("direct-allow-"+request.ID), "Allow once").Disabled(p.approvalBusy).OnClick(func() { p.respond(request.ID, true) })
				ui.Button(c.Key("direct-deny-"+request.ID), "Deny once").Disabled(p.approvalBusy).OnClick(func() { p.respond(request.ID, false) })
			})
		})
	}
}
func (p *PersistentPage) buildDirectInspector(c *ui.Context) {
	ui.Scroll(c).Label("Inspector workspace").Grow(1).MinHeight(0).Gap(8).Children(func() {
		ui.Text(c, fmt.Sprintf("Native typed records · fixed high-water %d", p.inspector.HighWater)).Bold()
		ui.Row(c).Gap(6).Children(func() {
			for _, group := range []string{"All", "LLM", "Tools", "Middleware", "Other"} {
				group := group
				ui.Button(c, "Records: "+group).Disabled(p.inspectorBusy).OnClick(func() { p.changed(); p.inspectGroup = group })
			}
		})
		ui.Text(c, "Categories filter this metadata page only; cursors and model history are unchanged.").FontSize(11)
		if p.inspectLoadedQuery != "" {
			ui.Text(c, "Results for literal query: "+p.inspectLoadedQuery).FontSize(11)
		}
		ui.Text(c, "Read-only metadata and explicit original text pages; observation never restores an Agent or executes tools.").FontSize(11)
		ui.TextInput(c.Key("direct-inspector-search"), &p.inspectQuery).Label("Literal record search").Placeholder("Literal search in this session").Disabled(p.inspectorBusy)
		ui.Button(c, "Search / reset Inspector").Disabled(p.inspectorBusy).OnClick(func() { p.inspectAfter = nil; p.loadInspector(0, 0) })
		ui.Scroll(c).Label("Record metadata page").Height(220).Gap(6).Children(func() {
			for _, entry := range p.inspector.Entries {
				if !InspectionGroupVisible(entry, p.inspectGroup) {
					continue
				}
				entry := entry
				ui.Column(c).Gap(4).Children(func() {
					ui.Text(c, fmt.Sprintf("#%d · %s · %s/%s", entry.Sequence, entry.Kind, entry.Namespace, entry.Type)).Bold().FontSize(12)
					ui.Text(c, entry.Summary).FontSize(11)
					ui.Button(c.Key("read-"+entry.ID), "Read original 2 KiB").Disabled(p.inspectorBusy).OnClick(func() { p.readInspector(entry.Value.URI, 0) })
				})
			}
		})
		ui.Button(c, "Export all records at this snapshot").Disabled(p.inspectorBusy || p.inspector.HighWater == 0).OnClick(p.exportInspector)
		ui.Text(c, "Private JSONL export includes all record kinds at this snapshot; category/search filters are not export selection. Original files remain references.").FontSize(11)
		if p.inspectExportPath != "" {
			ui.TextArea(c.Key("direct-export-path"), &p.inspectExportPath).Label("Private Inspector export").ReadOnly(true).Height(64)
		}
		if p.inspectURI != "" {
			body := p.inspectText.Text
			ui.TextArea(c.Key("direct-record-body"), &body).Label("Original record range").ReadOnly(true).Height(140)
			ui.Button(c, "Next original range").Disabled(p.inspectorBusy || !p.inspectText.Truncated).OnClick(func() { p.readInspector(p.inspectURI, p.inspectText.NextOffset) })
		}
		ui.Row(c).Gap(8).Children(func() {
			ui.Button(c, "Previous record page").Disabled(p.inspectorBusy || len(p.inspectAfter) == 0).OnClick(func() {
				last := len(p.inspectAfter) - 1
				after := p.inspectAfter[last]
				p.inspectAfter = p.inspectAfter[:last]
				p.loadInspector(after, p.inspector.HighWater)
			})
			ui.Button(c, "Next record page").Disabled(p.inspectorBusy || !p.inspector.HasMore || len(p.inspectAfter) >= 256).OnClick(func() {
				p.inspectAfter = append(p.inspectAfter, p.inspectCursor)
				p.loadInspector(p.inspector.Next, p.inspector.HighWater)
			})
		})
	})
}
