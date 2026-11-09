// INPUT: kit Field and Input.
// OUTPUT: kitInput, a labelled text input the form tests place inside a form's field slot.
// POS: [L7 Kit blocks tests] Keeps the tests from repeating the same two kit calls.
//go:build desktop

package blocks

import (
	"github.com/QuincySx/Alva/apps/ui/kit"
	"github.com/egoist/mygo/ui"
)

func kitInput(c *ui.Context, value *string, title string) {
	kit.Field(c, kit.FieldProps{Title: title, Key: "f-" + title}, func() { kit.Input(c, value, kit.InputProps{Key: "i-" + title}) })
}

var _ = ui.KeyTab
