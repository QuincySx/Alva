// INPUT: runtime, path/filepath, testing
// OUTPUT: abs, a platform-neutral absolute path fixture helper for Guard tests
// POS: [L3 capability library] Lets Unix-style fixture paths be absolute on Windows too, so Guard construction never silently fails.
package security

import (
	"path/filepath"
	"runtime"
	"testing"
)

// abs turns a slash-style fixture path such as "/work/a" into a path that is
// absolute on the running OS: unchanged on Unix, "C:\work\a" on Windows.
func abs(path string) string {
	if runtime.GOOS == "windows" {
		return `C:` + filepath.FromSlash(path)
	}
	return path
}

func TestAbsFixturesAreAbsoluteOnEveryPlatform(t *testing.T) {
	if !filepath.IsAbs(abs("/work")) || !filepath.IsAbs(abs("/work/notes.txt")) {
		t.Fatal("fixture helper produced a non-absolute path")
	}
	if filepath.IsAbs("relative") {
		t.Fatal("relative fixture must stay relative")
	}
}
