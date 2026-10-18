// INPUT: Native-only folder selection, inspected product AEP pins and explicit next-turn host consent
// OUTPUT: Bounded native Plugin editor, disabled save and separate approve/disable controls
// POS: [L7 Hub product] MyGo product controls; never executes on selection/save/observation or imports browser host paths
//go:build desktop

package hubdesktop

import (
	"encoding/json"
	"fmt"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) openAEP() {
	if p.selected == "" || p.paneBusy {
		return
	}
	p.setPane("plugins")
	id, generation := p.selected, p.generation
	p.paneBusy = true
	go func() {
		value, err := p.Service.Store.Settings(p.ctx, id)
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.settings = value
			entries := value.AEP
			if entries == nil {
				entries = []hub.AEPBundle{}
			}
			data, _ := json.MarshalIndent(entries, "", "  ")
			p.aepDraft = string(data)
		})
	}()
}

func (p *NativePage) aepAction(id string, enabled bool) {
	if p.selected == "" || p.paneBusy || p.model.ActiveRunID != "" {
		return
	}
	session, raw := p.selected, p.aepDraft
	p.paneBusy = true
	go func() {
		var err error
		if id == "" {
			err = p.Service.SaveAEPDraft(p.ctx, session, raw)
		} else {
			err = p.Service.Store.SetAEPEnabled(p.ctx, session, id, enabled, enabled)
		}
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != session {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			p.openAEP()
		})
	}()
}

func (p *NativePage) pickAEP() {
	if p.window == nil || p.paneBusy || p.model.ActiveRunID != "" {
		return
	}
	id, generation := p.selected, p.generation
	var entries []hub.AEPBundle
	if json.Unmarshal([]byte(p.aepDraft), &entries) != nil || len(entries) >= 8 {
		p.problem = "Use a valid JSON array with fewer than 8 entries before adding a source folder."
		return
	}
	p.paneBusy = true
	go func() {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: p.window, Directory: true, Title: "Select explicit AEP source folder — no execution or automatic approval"})
		var entryID string
		if err == nil && len(paths) > 0 {
			entryID, err = hub.NewID("aep-")
		}
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			if len(paths) == 0 {
				return
			}
			entries = append(entries, hub.AEPBundle{ID: entryID, SourceDirectory: paths[0]})
			data, marshalErr := json.MarshalIndent(entries, "", "  ")
			if marshalErr != nil {
				p.problem = "AEP draft could not be encoded"
				return
			}
			p.aepDraft = string(data)
			p.problem = "Source selected only. Set an explicit absolute interpreter path and any required approved args/env before inspecting."
		})
	}()
}

func (p *NativePage) buildAEP(c *ui.Context) {
	ui.Scroll(c).Grow(1).MinHeight(0).Gap(8).Children(func() {
		ui.Text(c, "Native AEP Plugins · explicit next-turn snapshots").Bold()
		ui.Text(c, "Native code has full host access, not an OS sandbox. Selection/save only copies a bounded source snapshot to inspect its manifest and code digest, then removes it; no interpreter starts. A separate approval enables the inspected pin only for the next Alva attempt. No global scan, package install or existing config import.").FontSize(11)
		ui.Text(c, "JSON: id, source_directory, interpreter (absolute paths), interpreter_args, env. Saving clears every AEP approval and derives name/version/sha256; imported flags and pins cannot self-approve. The interpreter binary is not hashed: an upgrade at the same executable path is not detected.").FontSize(11)
		ui.Button(c, "Select explicit source folder").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(p.pickAEP)
		ui.TextArea(c.Key("aep-json"), &p.aepDraft).Label("Native AEP configuration JSON").Height(230).Disabled(p.paneBusy || p.model.ActiveRunID != "")
		ui.Button(c, "Inspect pins and save all AEP disabled").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.aepAction("", false) })
		for _, entry := range p.settings.AEP {
			entry := entry
			ui.Text(c, fmt.Sprintf("%s · %s %s · enabled=%v", entry.ID, entry.Name, entry.Version, entry.Enabled)).Bold().FontSize(12)
			ui.Text(c, "Source: "+entry.SourceDirectory+"; interpreter: "+entry.Interpreter+"; approved code SHA-256: "+entry.SHA256).FontSize(11)
			if entry.Enabled {
				ui.Button(c.Key(entry.ID), "Disable AEP next turn").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.aepAction(entry.ID, false) })
			} else {
				ui.Button(c.Key(entry.ID), "Approve inspected code and full host access next turn").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.aepAction(entry.ID, true) })
			}
		}
		ui.Text(c, "At execution, the exact source pin is checked again; changes fail closed. Native launch risk is committed before the process starts, so failed preparation cannot trigger unsafe fallback. Tools still use the actual Guard/HITL; schemas, results and full redacted I/O stay native references. AEP is not automatically delegated to SDK children or other engines.").FontSize(11)
	})
}
