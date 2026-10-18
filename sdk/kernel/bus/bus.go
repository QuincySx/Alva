// INPUT: errors, reflect, sync
// OUTPUT: Writer, Handle, New, Provide, Resolve
// POS: [L0 foundation] Typed capability registry with separate registration and read-only handles.
package bus

import (
	"errors"
	"reflect"
	"sync"
)

type registry struct {
	mu     sync.RWMutex
	sealed bool
	values map[reflect.Type]any
}
type Writer struct{ registry *registry }
type Handle struct{ registry *registry }

func New() (Writer, Handle) {
	shared := &registry{values: make(map[reflect.Type]any)}
	return Writer{shared}, Handle{shared}
}
func (w Writer) Handle() Handle { return Handle{w.registry} }
func (w Writer) Seal() {
	if w.registry != nil {
		w.registry.mu.Lock()
		w.registry.sealed = true
		w.registry.mu.Unlock()
	}
}

// The chosen type T is the capability contract, usually an interface. Duplicate
// provision is an error, never implicit last-writer-wins behavior.
func Provide[T any](writer Writer, value T) error {
	if writer.registry == nil {
		return errors.New("uninitialized bus writer")
	}
	typeID := reflect.TypeFor[T]()
	if nilValue(value) {
		return errors.New("nil capability")
	}
	writer.registry.mu.Lock()
	defer writer.registry.mu.Unlock()
	if writer.registry.sealed {
		return errors.New("bus registration sealed")
	}
	if _, exists := writer.registry.values[typeID]; exists {
		return errors.New("capability already registered: " + typeID.String())
	}
	writer.registry.values[typeID] = value
	return nil
}

func nilValue(value any) bool {
	reflected := reflect.ValueOf(value)
	if !reflected.IsValid() {
		return true
	}
	switch reflected.Kind() {
	case reflect.Pointer, reflect.Interface, reflect.Map, reflect.Func, reflect.Slice, reflect.Chan:
		return reflected.IsNil()
	default:
		return false
	}
}

func Resolve[T any](handle Handle) (T, bool) {
	var zero T
	if handle.registry == nil {
		return zero, false
	}
	handle.registry.mu.RLock()
	value, exists := handle.registry.values[reflect.TypeFor[T]()]
	handle.registry.mu.RUnlock()
	if !exists {
		return zero, false
	}
	typed, ok := value.(T)
	return typed, ok
}
