// INPUT: Selected direct native Inspector snapshot and explicit user export request
// OUTPUT: PersistentPage private redacted JSONL export and truthful result metadata
// POS: [L6 application] Saving an Inspector copy never restores an Agent, executes tools or fetches referenced artifacts.
//go:build desktop

package desktop

import "fmt"

func (p *PersistentPage) exportInspector() {
	if p.inspectorBusy || p.selected == "" || p.inspector.HighWater == 0 {
		return
	}
	p.inspectorBusy = true
	p.inspectExportBusy = true
	id, generation, high := p.selected, p.generation, p.inspector.HighWater
	go func() {
		result, err := p.Service.ExportInspector(p.ctx, id, high)
		p.dispatch(func() {
			p.inspectorBusy = false
			p.inspectExportBusy = false
			if p.selected != id || p.generation != generation {
				return
			}
			if err != nil {
				p.problem = "Inspector export did not complete: " + err.Error()
				return
			}
			p.inspectExportPath = fmt.Sprintf("%s\n%d records · %d bytes · SHA256 %s", result.Path, result.Summary.Records, result.Summary.Bytes, result.Summary.SHA256)
		})
	}()
}
