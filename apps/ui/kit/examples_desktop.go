// INPUT: The design component catalog (axes and values) and every kit component.
// OUTPUT: Page and Axis, the registry Pages(), and Variants(): one gallery page per component whose cells are the design's Cartesian variants.
// POS: [L7 Kit] Example pages are the gallery's content and the coverage gate's evidence; forced states come from internal/force so Props stay free of Hover/Pressed/Focus.
//go:build desktop

package kit

import (
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
)

// sampleWidth is the gallery width of input samples; it is not a design token.
const sampleWidth = 220

var cellSlots sync.Map // string -> any (*T)

// cellSlot returns the persistent value of one named slot of one gallery cell, created from init on first use, so a
// sample keeps what the user types or toggles across frames. The key is page, axis values and slot name.
func cellSlot[T any](page string, sel []string, slot string, init T) *T {
	key := page + "|" + strings.Join(sel, "|") + "|" + slot
	if v, ok := cellSlots.Load(key); ok {
		return v.(*T)
	}
	v, _ := cellSlots.LoadOrStore(key, &init)
	return v.(*T)
}

// CellText reads a string slot of a gallery cell, for tests that must see what the user typed.
func CellText(page string, sel []string, slot string) (string, bool) {
	if v, ok := cellSlots.Load(page + "|" + strings.Join(sel, "|") + "|" + slot); ok {
		if s, ok := v.(*string); ok {
			return *s, true
		}
	}
	return "", false
}

// ResetCells forgets every cell's state (tests and the gallery's reset).
func ResetCells() { cellSlots.Clear() }

// Axis is one design axis with its values in design order.
type Axis struct {
	Name   string
	Values []string
}

// Page is one component's gallery page.
type Page struct {
	// Name is the kit component; Design names the design component it covers ("" when the design has none).
	Name, Design, Category string
	// Axes are the design's axes; the page has one cell per combination (one cell when empty).
	Axes []Axis
	// Cell renders one variant. sel holds one value per axis, in Axes order.
	Cell func(c *ui.Context, sel []string)
	// Skip returns a reason for a combination the kit does not render yet, or "".
	Skip func(sel []string) string
	// Live renders an interactive demo below the matrix (a real open menu, a real select); nil for none.
	Live func(c *ui.Context)
}

// Variants counts the page's cells.
func (p Page) Variants() int {
	n := 1
	for _, a := range p.Axes {
		n *= len(a.Values)
	}
	return n
}

// Combinations lists every selection in design order (the last axis varies fastest).
func (p Page) Combinations() [][]string {
	out := [][]string{nil}
	for _, a := range p.Axes {
		var next [][]string
		for _, prefix := range out {
			for _, v := range a.Values {
				next = append(next, append(append([]string(nil), prefix...), v))
			}
		}
		out = next
	}
	return out
}

// Skipped counts the cells Skip declines.
func (p Page) Skipped() int {
	if p.Skip == nil {
		return 0
	}
	n := 0
	for _, sel := range p.Combinations() {
		if p.Skip(sel) != "" {
			n++
		}
	}
	return n
}

// Pages is every kit component page in gallery order.
func Pages() []Page {
	return append([]Page{
		textPage(), iconPage(), buttonPage(), iconButtonPage(), inputPage(), textAreaPage(), fieldPage(),
		selectPage(), checkboxPage(), switchPage(), tooltipPage(), menuPage(),
	}, k2Pages()...)
}

func axis(name string, values ...string) Axis { return Axis{Name: name, Values: values} }
