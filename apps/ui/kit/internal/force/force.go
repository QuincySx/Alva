// INPUT: Gallery and test code asking a component to render as if hovered, pressed or keyboard-focused.
// OUTPUT: A scoped process-wide state read by kit components while they build; never part of any Props type.
// POS: [L7 Kit] Spike result: Hover/Pressed/Focus cannot be staged with a real pointer in a variant matrix, so builders read this scope. Internal to apps/ui/kit.
package force

import "sync/atomic"

// State is a forced interaction state.
type State uint32

const (
	// None leaves state to the runtime.
	None State = iota
	Hover
	Pressed
	Focus
)

var current atomic.Uint32

// Current is the state forced for the control being built. Components OR it with the real state.
func Current() State { return State(current.Load()) }

// With runs build with s forced and restores the previous state. Builds are synchronous on the UI
// thread, so a scope never leaks into a later frame. Tests that use it must not run in parallel.
func With(s State, build func()) {
	previous := current.Swap(uint32(s))
	defer current.Store(previous)
	build()
}

// Take returns the forced state and clears it for the rest of the enclosing With, so a control that
// forces its own state never leaks it into the controls nested inside it (an input's inline action).
func Take() State { return State(current.Swap(uint32(None))) }

// Hovered reports whether the real or forced state is hover or pressed.
func (s State) Hovered(real bool) bool { return real || s == Hover || s == Pressed }

// Pressed reports whether the real or forced state is pressed.
func (s State) Pressed(real bool) bool { return real || s == Pressed }

// Focused reports whether the real or forced state shows a keyboard focus ring.
func (s State) Focused(real bool) bool { return real || s == Focus }
