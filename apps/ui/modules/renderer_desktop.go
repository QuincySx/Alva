// INPUT: MyGo native ui/markdown and bounded versioned module values
// OUTPUT: Registry, Register and Options with real table/bar-chart renderers and raw fallback
// POS: [L6 application] Trusted Go renderer extension surface; untrusted message data never becomes code
//go:build desktop

package modules

import (
	"encoding/json"
	"errors"
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
)

type Renderer func(*ui.Context, json.RawMessage) error
type key struct {
	kind    string
	version int
}
type Registry struct{ renderers map[key]Renderer }

func NewRegistry() *Registry {
	r := &Registry{renderers: make(map[key]Renderer)}
	_ = r.Register("table", 1, renderTable)
	_ = r.Register("bar-chart", 1, renderChart)
	return r
}
func (r *Registry) Register(kind string, version int, renderer Renderer) error {
	if kind == "" || version < 1 || renderer == nil {
		return errors.New("trusted renderer identity/function required")
	}
	k := key{kind, version}
	if _, exists := r.renderers[k]; exists {
		return errors.New("renderer replacement requires an explicit registry rebuild")
	}
	r.renderers[k] = renderer
	return nil
}
func (r *Registry) Options() *markdown.Options {
	return &markdown.Options{Code: map[string]func(*ui.Context, *markdown.CodeBlockView){"alva-module": func(c *ui.Context, block *markdown.CodeBlockView) {
		if block.Open {
			markdown.DefaultCodeBlock(c, block)
			return
		}
		module, err := Decode(block.Code())
		if err == nil {
			if render := r.renderers[key{module.Kind, module.Version}]; render != nil {
				if err = render(c, module.Data); err == nil {
					return
				}
			}
		}
		ui.Text(c, "Module unavailable or invalid; original data shown below.").FontSize(12)
		markdown.DefaultCodeBlock(c, block)
	}}}
}
func renderTable(c *ui.Context, data json.RawMessage) error {
	var value Table
	if err := json.Unmarshal(data, &value); err != nil {
		return err
	}
	if err := value.Validate(); err != nil {
		return err
	}
	ui.Column(c).Gap(6).Children(func() {
		ui.Row(c).Gap(12).Children(func() {
			for _, column := range value.Columns {
				ui.Text(c, column).Bold().Grow(1)
			}
		})
		for _, row := range value.Rows {
			ui.Row(c).Gap(12).Children(func() {
				for _, cell := range row {
					ui.Text(c, cell).Grow(1)
				}
			})
		}
	})
	return nil
}
func renderChart(c *ui.Context, data json.RawMessage) error {
	var value Chart
	if err := json.Unmarshal(data, &value); err != nil {
		return err
	}
	if err := value.Validate(); err != nil {
		return err
	}
	max := float64(1)
	for _, v := range value.Values {
		if v > max {
			max = v
		}
	}
	ui.Column(c).Gap(6).Children(func() {
		for i, label := range value.Labels {
			ui.Row(c).Gap(8).Children(func() {
				ui.Text(c, label).Width(120)
				ui.Box(c).Width(float32(value.Values[i] / max * 240)).Height(12).Background(c.Theme().Accent)
				ui.Text(c, fmt.Sprintf("%g", value.Values[i]))
			})
		}
	})
	return nil
}
