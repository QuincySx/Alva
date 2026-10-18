// INPUT: context, filepath, testing, storage/session, agent/security
// OUTPUT: Native child binding construction and exact grant identity regressions
// POS: [L5 harness] Rejects parent value tools whose dynamic configuration cannot prove identity.
package harness

import (
	"context"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/security"
)

type identityBindingTool struct {
	echoTool
	configuration any
}

func TestChildGrantIdentityUsesDynamicValues(t *testing.T) {
	for _, values := range [][2]any{{map[string]string{}, "comparable"}, {"comparable", []string{}}, {map[string]string{}, map[string]string{}}} {
		func() {
			defer func() {
				if value := recover(); value != nil {
					t.Errorf("child grant comparison panicked: %v", value)
				}
			}()
			if sameTool(identityBindingTool{configuration: values[0]}, identityBindingTool{configuration: values[1]}) {
				t.Error("noncomparable value proved grant identity")
			}
		}()
	}
	if !sameTool(identityBindingTool{configuration: "same"}, identityBindingTool{configuration: "same"}) {
		t.Fatal("comparable value identity lost")
	}
}

func TestNativeChildFactoryRejectsNonComparableBindingValue(t *testing.T) {
	cwd := t.TempDir()
	repository, err := session.Open(t.Context(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer repository.Close()
	if _, err := repository.Create(t.Context(), "owner", "parent", ""); err != nil {
		t.Fatal(err)
	}
	guard, err := security.NewGuard(cwd, nil, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	called := false
	factory, err := NewNativeChildFactory(t.Context(), NativeChildConfig{Repository: repository, Owner: "owner", ParentID: "parent", Cwd: cwd, Guard: guard, Bindings: []ChildToolBinding{{Parent: identityBindingTool{configuration: map[string]string{}}, Bind: func(context.Context, ChildBinding) (BoundChildTool, error) {
		called = true
		return BoundChildTool{}, nil
	}}}})
	if err == nil || factory != nil || called {
		t.Fatal("unprovable parent binding accepted", factory, called, err)
	}
}
