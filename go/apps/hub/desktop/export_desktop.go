// INPUT: Actual selected branch/typed child Inspector snapshot and native-only private export action
// OUTPUT: Explicit redacted JSONL save using the product's exact authorization and bounded native facts
// POS: [L7 Hub product] UI group/query filters never masquerade as export scope or model-side context policy.
//go:build desktop

package hubdesktop

import (
	"fmt"
	"github.com/QuincySx/Alva/go/apps/hub/product"
)

func (p *NativePage) exportInspector() {
	if p.paneBusy || p.selected == "" || p.inspectPage.NativeID == "" {
		return
	}
	p.paneBusy = true
	p.inspectExportBusy = true
	id, branch, generation, high, children := p.selected, p.inspectBranch, p.generation, p.inspectPage.HighWater, p.inspectExportChildren
	path := append([]product.ChildHop(nil), p.inspectPath...)
	go func() {
		result, err := p.Service.SaveInspectorBundleAt(p.ctx, id, branch, path, high, children)
		p.dispatch(func() {
			p.paneBusy = false
			p.inspectExportBusy = false
			if p.selected != id || p.generation != generation || p.inspectBranch != branch {
				return
			}
			if err != nil {
				p.problem = "Inspector export did not complete: " + err.Error()
				return
			}
			p.inspectExportPath = fmt.Sprintf("%s\n%d sessions · %d records · %d bytes · SHA256 %s", result.Path, result.Summary.Sessions, result.Summary.Records, result.Summary.Bytes, result.Summary.SHA256)
		})
	}()
}
