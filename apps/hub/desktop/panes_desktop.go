// INPUT: MyGo native controls, private product settings and read-only bounded Inspector service
// OUTPUT: Explicit next-turn capability approvals and typed-child Inspector paging/search/reads
// POS: [L7 Hub product] Product settings/Inspector surfaces never feed complete logs or start processes on observation
//go:build desktop

package hubdesktop

import (
	"fmt"

	"github.com/egoist/mygo/ui"

	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/platform/storage"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

type InspectorFrame struct {
	Path               []product.ChildHop
	Page               product.InspectorPage
	Cursor             uint64
	After              []uint64
	Query, URI         string
	LoadedQuery, Group string
	Text               agentcontext.TextPage
}

func (p *NativePage) drillInspector(sequence uint64) {
	if p.paneBusy || len(p.inspectPath) >= 8 {
		return
	}
	p.inspectFrames = append(p.inspectFrames, InspectorFrame{Path: append([]product.ChildHop(nil), p.inspectPath...), Page: p.inspectPage, Cursor: p.inspectCursor, After: append([]uint64(nil), p.inspectAfter...), Query: p.inspectQuery, URI: p.inspectURI, Text: p.inspectText, LoadedQuery: p.inspectLoadedQuery, Group: p.inspectGroup})
	p.inspectPath = append(append([]product.ChildHop(nil), p.inspectPath...), product.ChildHop{LinkSequence: sequence})
	p.inspectPage = product.InspectorPage{}
	p.inspectAfter = nil
	p.inspectQuery = ""
	p.inspectLoadedQuery = ""
	p.inspectGroup = ""
	p.inspectExportPath = ""
	p.inspectURI = ""
	p.inspectText = agentcontext.TextPage{}
	p.loadInspector(0, 0)
}
func (p *NativePage) parentInspector() {
	if p.paneBusy || len(p.inspectFrames) == 0 {
		return
	}
	index := len(p.inspectFrames) - 1
	frame := p.inspectFrames[index]
	p.inspectFrames = p.inspectFrames[:index]
	p.inspectPath = frame.Path
	p.inspectPage = frame.Page
	p.inspectCursor = frame.Cursor
	p.inspectAfter = frame.After
	p.inspectQuery = frame.Query
	p.inspectLoadedQuery = frame.LoadedQuery
	p.inspectGroup = frame.Group
	p.inspectExportPath = ""
	p.inspectURI = frame.URI
	p.inspectText = frame.Text
}

func (p *NativePage) openInspector() {
	if p.selected == "" || p.paneBusy {
		return
	}
	p.setPane("inspector")
	if p.inspectBranch == "" {
		p.inspectBranch = p.detail.Session.ActiveBranchID
	}
	p.inspectAfter = nil
	p.inspectPath = nil
	p.inspectFrames = nil
	p.loadInspector(0, 0)
}
func (p *NativePage) loadInspector(after, high uint64) {
	if p.selected == "" || p.paneBusy {
		return
	}
	query := p.inspectLoadedQuery
	if high == 0 {
		query = p.inspectQuery
	}
	id, branch, generation := p.selected, p.inspectBranch, p.generation
	path := append([]product.ChildHop(nil), p.inspectPath...)
	p.paneBusy = true
	go func() {
		page, err := p.Service.InspectorAt(p.ctx, id, branch, path, query, after, high)
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation || p.inspectBranch != branch {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			p.inspectPage = page
			p.inspectLoadedQuery = query
			p.inspectCursor = after
		})
	}()
}
func (p *NativePage) readInspector(uri string, offset int64) {
	p.readInspectorBound(uri, offset, nil)
}

func (p *NativePage) readInspectorBound(uri string, offset int64, accept func() bool) {
	if p.paneBusy || p.selected == "" {
		return
	}
	id, branch, generation := p.selected, p.inspectBranch, p.generation
	path := append([]product.ChildHop(nil), p.inspectPath...)
	p.paneBusy = true
	go func() {
		page, err := p.Service.InspectorReadAt(p.ctx, id, branch, path, uri, offset)
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation || p.inspectBranch != branch || accept != nil && !accept() {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			p.inspectURI = uri
			p.inspectText = page
		})
	}()
}
func (p *NativePage) buildInspector(c *ui.Context) {
	ui.Scroll(c).Label("Inspector workspace").Grow(1).MinHeight(0).Gap(8).Children(func() {
		ui.Text(c, "Committed execution Inspector · read-only").Bold()
		ui.Row(c).Gap(6).Children(func() {
			for _, group := range []string{"All", "LLM", "Tools", "Middleware", "Other"} {
				group := group
				ui.Button(c, "Records: "+group).Disabled(p.paneBusy).OnClick(func() { p.inspectGroup = group; p.inspectList.ScrollTo(0, ui.Start) })
			}
		})
		ui.Text(c, "Categories filter this metadata page only; cursors and model context are unchanged.").FontSize(11)
		ui.Row(c).Gap(6).Children(func() {
			ui.Button(c, "Parent Agent page").Disabled(p.paneBusy || len(p.inspectFrames) == 0).OnClick(p.parentInspector)
			ui.Text(c, fmt.Sprintf("Native target %s · typed depth %d", p.inspectPage.NativeID, len(p.inspectPath))).FontSize(11)
		})
		ui.Text(c, "Select a retained branch. Metadata/search and 2 KiB UTF-8 body reads are explicit; no Agent resume, blob-wide conversation projection, tool replay or MCP startup.").FontSize(11)
		ui.Row(c).Gap(6).Wrap().Children(func() {
			for _, branch := range p.detail.Branches {
				branch := branch
				descriptor, registered := p.Service.Runtime.Descriptor(branch.Target.EngineID)
				available := registered && descriptor.Inspector != nil && descriptor.Inspector.InspectionCapabilities().Historical
				label := branch.Target.EngineID + " · " + branch.ID
				if !available {
					label += " · inspection unsupported"
				}
				ui.Button(c, label).Disabled(p.paneBusy || branch.NativeSessionID == "" || !available).OnClick(func() {
					p.inspectBranch = branch.ID
					p.inspectExportPath = ""
					p.inspectURI = ""
					p.inspectText.Text = ""
					p.inspectAfter = nil
					p.inspectPath = nil
					p.inspectFrames = nil
					p.loadInspector(0, 0)
				})
			}
		})
		ui.Row(c).Gap(8).Children(func() {
			ui.TextInput(c.Key("inspect-query"), &p.inspectQuery).Label("Literal record search").Grow(1).Disabled(p.paneBusy)
			ui.Button(c, "Search / refresh snapshot").Disabled(p.paneBusy).OnClick(func() { p.inspectAfter = nil; p.loadInspector(0, 0) })
		})
		if p.inspectLoadedQuery != "" {
			ui.Text(c, "Results for literal query: "+p.inspectLoadedQuery).FontSize(11)
		}
		visible := make([]storage.RecordEntry, 0, len(p.inspectPage.Entries))
		for _, entry := range p.inspectPage.Entries {
			if view.InspectionGroupVisible(entry, p.inspectGroup) {
				visible = append(visible, entry)
			}
		}
		if len(visible) == 0 {
			ui.Text(c, "No records in this category on the current page.").FontSize(11)
		}
		p.inspectList.Key = func(i int) any { return visible[i].ID }
		ui.List(c.Key("native-records"), &p.inspectList, len(visible), func(i int) {
			entry := visible[i]
			ui.Column(c).Gap(3).Children(func() {
				ui.Button(c, fmt.Sprintf("#%d · %s/%s · %s", entry.Sequence, entry.Namespace, entry.Type, entry.Summary)).OnClick(func() { p.readInspector(entry.Value.URI, 0) })
				if entry.Kind == abi.RecordExtension && entry.Namespace == "alva.agent" && entry.Type == "child_session_linked" && entry.SchemaVersion == 1 {
					ui.Button(c.Key("child-"+entry.ID), "Inspect linked child Agent").Disabled(p.paneBusy || len(p.inspectPath) >= 8).OnClick(func() { p.drillInspector(entry.Sequence) })
				}
			})
		}).Height(180)
		ui.Row(c).Gap(8).Children(func() {
			ui.Button(c, "Previous metadata page").Disabled(p.paneBusy || len(p.inspectAfter) == 0).OnClick(func() {
				last := len(p.inspectAfter) - 1
				after := p.inspectAfter[last]
				p.inspectAfter = p.inspectAfter[:last]
				p.loadInspector(after, p.inspectPage.HighWater)
			})
			ui.Button(c, "Next metadata page").Disabled(p.paneBusy || !p.inspectPage.HasMore).OnClick(func() {
				p.inspectAfter = append(p.inspectAfter, p.inspectCursor)
				p.loadInspector(p.inspectPage.Next, p.inspectPage.HighWater)
			})
		})
		ui.Text(c, p.inspectPage.Processing).FontSize(11)
		ui.Text(c, fmt.Sprintf("Fixed high-water %d; next cursor %d", p.inspectPage.HighWater, p.inspectPage.Next)).FontSize(11)
		ui.Checkbox(c, &p.inspectExportChildren, "Include verified linked children in export").Disabled(p.paneBusy)
		ui.Button(c, "Export all records at this snapshot").Disabled(p.paneBusy || p.inspectPage.NativeID == "").OnClick(p.exportInspector)
		ui.Text(c, "Export writes redacted JSONL to this app's private exports folder; current query/category filters are not export selection. Referenced files stay references. Limit:8MiB/2048 records.").FontSize(11)
		if p.inspectExportPath != "" {
			ui.TextArea(c.Key("inspector-export-path"), &p.inspectExportPath).Label("Private Inspector export").ReadOnly(true).Height(64)
		}
		ui.TextArea(c.Key("native-record-body"), &p.inspectText.Text).Label("Explicit selected record text").ReadOnly(true).Height(180)
		ui.Button(c, "Read next 2 KiB of selected reference").Disabled(p.paneBusy || !p.inspectText.Truncated).OnClick(func() { p.readInspector(p.inspectURI, p.inspectText.NextOffset) })
		ui.Text(c, "Child navigation revalidates exact committed typed links from this branch root; parent page state is preserved. Categories and export grant no model log access, Agent restore or execution.").FontSize(11)
	})
}
