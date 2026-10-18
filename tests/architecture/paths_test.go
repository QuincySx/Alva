// INPUT: runtime/filepath and reviewed executable, desktop and verification package roles
// OUTPUT: Shared role paths for dependency and layout guards
// POS: Keeps physical role locations explicit while negative fixtures independently verify exact package boundaries.
package architecture_test

import (
	"path/filepath"
	"runtime"
)

const (
	commandRoot       = "cmd"
	directDesktopRoot = "apps/direct/desktop"
	hubDesktopRoot    = "apps/hub/desktop"
	sharedChatRoot    = "apps/ui/chat"
	sharedModulesRoot = "apps/ui/modules"
	validationRoot    = "tests/architecture"
	extensibilityRoot = "tests/extensibility"
)

func goRoot() string {
	_, source, _, _ := runtime.Caller(0)
	return filepath.Dir(filepath.Dir(filepath.Dir(source)))
}

func desktopPackage(name string) bool {
	return under(name, directDesktopRoot) || under(name, hubDesktopRoot) || under(name, sharedChatRoot) || under(name, sharedModulesRoot)
}
