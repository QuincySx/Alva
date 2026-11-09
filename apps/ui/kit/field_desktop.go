// INPUT: Field title/hint/error and the control built inside it.
// OUTPUT: Field: a labelled column that passes its Invalid/Disabled/ReadOnly/Busy state and accessible name to the control it wraps.
// POS: [L7 Kit] The scope is a build-time stack read by Input/TextArea/Select; builds are synchronous on the UI thread, so it never leaks.
//go:build desktop

package kit

import (
	"sync/atomic"

	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// FieldProps configure Field. The zero value is an enabled field with only a title.
type FieldProps struct {
	Title string
	// Hint is caption text under the control; Error replaces it and marks the control invalid.
	Hint, Error string
	Disabled    bool
	ReadOnly    bool
	// Busy shows the loader in the control's trailing slot.
	Busy bool
	Key  string
}

var fieldScope atomic.Pointer[FieldProps]

// fieldDefaults is the enclosing Field's props, or the zero value outside a Field.
func fieldDefaults() FieldProps {
	if f := fieldScope.Load(); f != nil {
		return *f
	}
	return FieldProps{}
}

// Field wraps a control with a title, a hint or error, and the states the design shares across fields.
// The control's own Label may be empty: the Field's Title becomes its accessible name, so several
// controls in one Field share that name; give them their own Label when they must differ.
func Field(c *ui.Context, p FieldProps, control func()) ui.Element {
	if outer := fieldScope.Load(); outer != nil {
		// A nested Field keeps what its parent imposes (disabled, read-only, busy); its own Error wins.
		p.Disabled, p.ReadOnly, p.Busy = p.Disabled || outer.Disabled, p.ReadOnly || outer.ReadOnly, p.Busy || outer.Busy
	}
	previous := fieldScope.Swap(&p)
	defer fieldScope.Store(previous)
	return ui.Column(keyed(c, p.Key)).FillWidth().Gap(theme.Space1).Children(func() {
		if p.Title != "" {
			Text(c, p.Title, TextProps{Strong: true, SingleLine: true, Key: "title"})
		}
		control()
		switch {
		case p.Error != "":
			Text(c, p.Error, TextProps{Role: RoleCaption, Tone: ToneDanger, Key: "error"})
		case p.Hint != "":
			Text(c, p.Hint, TextProps{Role: RoleCaption, Tone: ToneSecondary, Key: "hint"})
		}
	})
}
