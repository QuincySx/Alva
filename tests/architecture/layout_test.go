// INPUT: Repository Go directory layout and explicit layered SDK/product ownership
// OUTPUT: Required layer locations and regressions rejecting retired directory resurrection
// POS: Keeps the physical architecture readable without retaining import or directory compatibility bridges.
package architecture_test

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestLayeredDirectoryLayout(t *testing.T) {
	for _, directory := range []string{
		"sdk/llm/wire", "sdk/kernel/bus", "sdk/kernel/abi", "sdk/kernel/core",
		"sdk/agent/abi", "sdk/agent/core", "sdk/agent/context", "sdk/agent/memory", "sdk/agent/security", "sdk/agent/graph", "sdk/sandbox/abi",
		"extension", "harness", "integration/llm/codec", "integration/llm/provider", "integration/protocol", "integration/engine",
		"extension/files", "extension/security", "platform/host/workspace",
		"extension/diagnostics", "platform/system/strictjson",
		"platform/system", "platform/host", "platform/storage", "platform/sandbox/host", "platform/sandbox/guest",
		commandRoot, directDesktopRoot, sharedChatRoot, sharedModulesRoot, "apps/hub", "apps/hub/bootstrap", hubDesktopRoot, "apps/gateway", validationRoot, extensibilityRoot, "tools/qa",
	} {
		t.Run(directory, func(t *testing.T) {
			info, err := os.Stat(filepath.Join(goRoot(), directory))
			if err != nil || !info.IsDir() {
				t.Fatalf("required independent layer %s is missing: %v", directory, err)
			}
		})
	}
	for _, retired := range []string{
		"llm", "kernel", "agent", "app", "host", "desktop", "protocol", "engine", "hub", "apps/cmd", "native", "storage", "sandbox",
		"apps/hub/native", "apps/desktop/hub", "platform/sandbox/abi",
		"architecture",
		"apps/desktop",
		"extension/builtin",
		"extension/tooling", "platform/system/httpjson",
		"platform/httpjson", "platform/loopback", "platform/media", "platform/process", "platform/secretinput",
	} {
		t.Run("retired/"+retired, func(t *testing.T) {
			_, err := os.Lstat(filepath.Join(goRoot(), retired))
			if !os.IsNotExist(err) {
				t.Fatalf("retired path %s must not be retained or recreated (including aliases/symlinks): %v", retired, err)
			}
		})
	}
}

func TestLayerContainerMaps(t *testing.T) {
	// These organizing directories intentionally need not contain .go files, so
	// the source-header guard alone cannot protect their navigation documents.
	for _, directory := range []string{
		"sdk", "sdk/llm", "sdk/kernel", "sdk/agent", "sdk/sandbox",
		"integration", "integration/llm", "integration/protocol",
		"platform", "platform/system", "platform/storage", "platform/sandbox",
		"extension", "apps", "apps/ui", commandRoot, "tests", "tools", "tools/qa",
	} {
		t.Run(directory, func(t *testing.T) {
			path := filepath.Join(goRoot(), directory, "AGENTS.md")
			data, err := os.ReadFile(path)
			if err != nil {
				t.Fatalf("layer container requires its navigation map: %v", err)
			}
			for _, section := range []string{"## 地位", "## 逻辑", "## 约束", "## 业务域清单"} {
				if !strings.Contains(string(data), section) {
					t.Errorf("%s: missing %s", path, section)
				}
			}
		})
	}
}
