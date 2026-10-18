// INPUT: MyGo headless Tester and trusted native renderer registry
// OUTPUT: Versioned table/chart rendering, duplicate registration and raw fallback regressions
// POS: [L6 application] Pure-data modules never become executable markup or downloaded script
//go:build desktop

package modules

import (
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"testing"
)

func TestNativeModulesAndRawFallback(t *testing.T) {
	registry := NewRegistry()
	options := registry.Options()
	render := func(source string) *ui.Tester {
		return ui.NewTester(func(c *ui.Context) { ui.Column(c).Fill().Children(func() { markdown.View(c, source, options) }) }, 900, 700)
	}
	table := render("```alva-module\n" + `{"kind":"table","version":1,"data":{"columns":["Name","Value"],"rows":[["Actual", "42"]]}}` + "\n```")
	if !table.HasText("Actual") || !table.HasText("42") {
		t.Fatal("trusted native table not rendered")
	}
	chart := render("```alva-module\n" + `{"kind":"bar-chart","version":1,"data":{"labels":["First"],"values":[3]}}` + "\n```")
	if !chart.HasText("First") {
		t.Fatal("native chart label missing")
	}
	fallback := render("```alva-module\n" + `{"kind":"remote-script","version":1,"data":{"source":"untrusted"}}` + "\n```")
	if !fallback.HasText("Module unavailable or invalid; original data shown below.") {
		t.Fatal("unknown renderer swallowed original")
	}
	if err := registry.Register("table", 1, renderTable); err == nil {
		t.Fatal("duplicate trusted renderer silently replaced")
	}
}
