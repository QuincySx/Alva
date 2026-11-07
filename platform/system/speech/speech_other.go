// INPUT: Explicit local dictation requests on platforms without a native adapter.
// OUTPUT: Truthful unsupported capability; no microphone or network use.
// POS: [platform / OS primitive] Platform boundary for local speech recognition.
//go:build !darwin

package speech

import "context"

func Supported() bool { return false }
func runNative(context.Context, Options, <-chan struct{}, func(State, string) error) error {
	return ErrUnsupported
}
