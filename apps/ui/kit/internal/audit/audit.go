// INPUT: Interactive kit constructors reporting the accessible name they gave a control.
// OUTPUT: An optional sink that tests install to assert no interactive control is nameless.
// POS: [L7 Kit] Runtime half of the "icon buttons must have a name" rule (the compile-time half is IconButton's positional label). Internal to apps/ui/kit.
package audit

import (
	"strings"
	"sync"
)

// Entry is one reported control.
type Entry struct{ Kind, Name string }

var (
	mu   sync.Mutex
	sink *[]Entry
)

// Named records that a control of kind has the accessible name. It is a no-op without a sink.
func Named(kind, name string) {
	name = strings.TrimSpace(name)
	mu.Lock()
	defer mu.Unlock()
	if sink != nil {
		*sink = append(*sink, Entry{kind, name})
	}
}

// Described records the accessible description a control exposes, so tests can assert it without a screen reader.
func Described(kind, description string) { Named("description:"+kind, description) }

// Capture installs a sink and returns a reader and a restore function.
func Capture() (entries func() []Entry, restore func()) {
	mu.Lock()
	defer mu.Unlock()
	previous := sink
	collected := &[]Entry{}
	sink = collected
	return func() []Entry {
			mu.Lock()
			defer mu.Unlock()
			return append([]Entry(nil), *collected...)
		}, func() {
			mu.Lock()
			defer mu.Unlock()
			sink = previous
		}
}
