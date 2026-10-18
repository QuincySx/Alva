// INPUT: Reviewed executable and desktop package roles
// OUTPUT: Shared role paths for dependency and layout guards
// POS: Keeps physical role locations explicit while negative fixtures independently verify exact package boundaries.
package architecture_test

const (
	commandRoot       = "cmd"
	directDesktopRoot = "apps/desktop"
	hubDesktopRoot    = "apps/hub/desktop"
)

func desktopPackage(name string) bool {
	return under(name, directDesktopRoot) || under(name, hubDesktopRoot)
}
