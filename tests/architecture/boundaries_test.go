// INPUT: go/parser, filesystem and the Go implementation import graph
// OUTPUT: Layered SDK/integration/platform/product dependency firewall and negative regressions
// POS: Test-only enforcement of preserved architecture across directory moves and tagged native UI.
package architecture_test

import (
	"fmt"
	"go/parser"
	"go/token"
	"io/fs"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"testing"
)

const module = "github.com/QuincySx/Alva/"

type importGraph map[string][]string

func TestSourceDependencyBoundaries(t *testing.T) {
	root := goRoot()
	graph := make(importGraph)
	err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() {
			if entry.Name() == "build" || entry.Name() == ".mygo" || entry.Name() == "vendor" {
				return filepath.SkipDir
			}
			return nil
		}
		if !strings.HasSuffix(path, ".go") || strings.HasSuffix(path, "_test.go") {
			return nil
		}
		rel, err := filepath.Rel(root, filepath.Dir(path))
		if err != nil {
			return err
		}
		name := filepath.ToSlash(rel)
		file, err := parser.ParseFile(token.NewFileSet(), path, nil, parser.ImportsOnly)
		if err != nil {
			return err
		}
		for _, declaration := range file.Imports {
			value, err := strconv.Unquote(declaration.Path.Value)
			if err != nil {
				return err
			}
			graph[name] = append(graph[name], value)
		}
		if _, ok := graph[name]; !ok {
			graph[name] = nil
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	for _, required := range []string{"sdk/llm/wire", "sdk/kernel/bus", "sdk/kernel/abi", "sdk/kernel/core", "sdk/agent/abi", "sdk/agent/core", "sdk/sandbox/abi", "integration/engine", "apps/hub"} {
		if _, found := graph[required]; !found {
			t.Errorf("required independent layer %s has no source", required)
		}
	}
	for _, err := range validate(graph) {
		t.Error(err)
	}
}

func validate(graph importGraph) []error {
	var failures []error
	var roots []string
	for name := range graph {
		roots = append(roots, name)
	}
	sort.Strings(roots)
	for _, root := range roots {
		visited := map[string]bool{root: true}
		var walk func(string, []string)
		walk = func(current string, path []string) {
			for _, imported := range graph[current] {
				dependency := strings.TrimPrefix(imported, module)
				next := append(append([]string(nil), path...), dependency)
				if under(dependency, "github.com/QuantumNous/new-api/relaykit") {
					failures = append(failures, fmt.Errorf("%s: retired RelayKit dependency must not re-enter the Go implementation", strings.Join(next, " -> ")))
				}
				if under(dependency, "integration/llm/codec/internal/magpie") && !under(current, "integration/llm/codec") {
					failures = append(failures, fmt.Errorf("%s: absorbed Magpie IR is private to codec", strings.Join(next, " -> ")))
				}
				_, local := graph[dependency]
				if reason := forbidden(root, dependency, imported != dependency || local || workspacePackage(dependency)); reason != "" {
					failures = append(failures, fmt.Errorf("%s: %s", strings.Join(next, " -> "), reason))
				}
				if _, local := graph[dependency]; local && !visited[dependency] {
					visited[dependency] = true
					walk(dependency, next)
				}
			}
		}
		walk(root, []string{root})
	}
	return failures
}

func under(name, prefix string) bool { return name == prefix || strings.HasPrefix(name, prefix+"/") }

// Graph membership also catches a future workspace top-level directory, rather
// than letting an outdated list turn a local package into a standard-library one.
func workspacePackage(name string) bool {
	for _, prefix := range []string{"sdk", "integration", "extension", "harness", "platform", "apps", commandRoot, "tests", "tools"} {
		if under(name, prefix) {
			return true
		}
	}
	return legacyWorkspacePackage(name)
}

func legacyWorkspacePackage(name string) bool {
	for _, prefix := range []string{"llm", "kernel", "agent", "app", "host", "desktop", "protocol", "engine", "hub", "native", "storage", "sandbox", "apps/cmd", "architecture", "apps/desktop"} {
		if under(name, prefix) {
			return true
		}
	}
	return false
}

func forbidden(root, dependency string, local bool) string {
	if (under(dependency, "tests") || under(dependency, "tools")) && !under(root, "tests") && !under(root, "tools") {
		return "production packages cannot depend on repository validation or maintenance tools"
	}
	if under(root, "apps/ui") && (under(dependency, "apps/direct") || under(dependency, "apps/hub") || under(dependency, "harness") || under(dependency, "platform/host") || under(dependency, "integration/engine")) {
		return "shared presentation cannot acquire product or execution ownership"
	}
	if under(root, "extension/utility/timer") {
		if (local && !under(dependency, "sdk") && !under(dependency, "extension/utility/timer")) || (!local && strings.Contains(dependency, ".")) {
			return "portable timer Tool may depend only on standard library and SDK, never native Plugin/storage/host"
		}
	}
	if under(root, "integration/llm/codec/internal/magpie") {
		allowed := map[string]bool{"bytes": true, "cmp": true, "crypto/sha256": true, "encoding/base64": true, "encoding/hex": true, "encoding/json": true, "errors": true, "fmt": true, "maps": true, "net/netip": true, "net/url": true, "path": true, "regexp": true, "slices": true, "sort": true, "strconv": true, "strings": true, "sync/atomic": true, "time": true, "unicode/utf8": true, "github.com/tidwall/gjson": true}
		if !allowed[dependency] {
			return "absorbed Magpie conversion is limited to pure value/JSON helpers"
		}
	}

	if legacyWorkspacePackage(dependency) {
		return "retired workspace import path cannot become a compatibility layer"
	}
	if under(root, "integration/llm/codec") {
		if dependency == "net/http" || dependency == "net" {
			return "pure codec does not own HTTP or network I/O"
		}
		if local && dependency != "sdk/llm/wire" && !under(dependency, "integration/llm/codec") {
			return "pure protocol codec may consume wire, never execution/HTTP/product policy"
		}
	}
	if under(root, "sdk/llm/wire") || under(root, "sdk/kernel/bus") {
		if local {
			return "L0 must have no workspace dependencies"
		}
	}
	if under(root, "sdk/sandbox/abi") && dependency != "sdk/llm/wire" {
		if local {
			return "sandbox ABI may share wire values, never runtime/host/product policy"
		}
		if strings.Contains(dependency, ".") {
			return "sandbox ABI must be standard-library and pure wire values only"
		}
	}
	if under(root, "sdk/kernel") && under(dependency, "sdk/agent") {
		return "kernel cannot depend on agent semantics or SDK assembly"
	}
	if under(root, "sdk/kernel/abi") && under(dependency, "sdk/kernel/core") {
		return "execution contracts cannot import their implementation"
	}
	if under(root, "sdk") {
		if local && !under(dependency, "sdk") {
			return "portable SDK cannot import a concrete provider, protocol, host or product"
		}
		if !under(root, "sdk/sandbox/abi") && under(dependency, "sdk/sandbox/abi") {
			return "agent/kernel SDK cannot acquire sandbox-specific policy through the SDK container"
		}
		if !local && strings.Contains(dependency, ".") {
			return "portable SDK must remain standard-library only"
		}
	}
	if under(dependency, "github.com/egoist/mygo") && !desktopPackage(root) && !under(root, commandRoot) {
		return "desktop framework is confined to desktop products and command entries, not all apps"
	}
	if root == "integration/engine" {
		if local || strings.Contains(dependency, ".") {
			return "engine contract root must remain standard-library only"
		}
	}
	if root == "apps/hub" || under(root, "apps/hub/core") {
		if (local && dependency != "integration/engine") || (!local && strings.Contains(dependency, ".")) {
			return "Hub dispatcher depends only on the engine contract root and standard library, never its own product/bootstrap subpackages"
		}
	}
	if root == "cmd/alva" || root == "cmd/alva-desktop" || under(root, "apps/direct/desktop") || under(root, "apps/direct") {
		if under(dependency, "integration/engine") || under(dependency, "apps/hub") {
			return "native harness entry cannot depend on multi-engine Hub runtime"
		}
	}
	if under(dependency, commandRoot) && !under(root, commandRoot) {
		return "libraries and products cannot depend on executable entrypoints"
	}
	if !under(root, "apps") && !under(root, commandRoot) && under(dependency, "apps") {
		return "SDK, harness, integrations and platform cannot depend upward on applications"
	}
	return ""
}

func TestFirewallRejectsTransitiveHostDependency(t *testing.T) {
	errors := validate(importGraph{"sdk/agent/core": {"sdk/kernel/abi"}, "sdk/kernel/abi": {"platform/host"}, "platform/host": nil})
	if len(errors) == 0 {
		t.Fatal("SDK -> contracts -> host must fail")
	}
	var chain bool
	for _, err := range errors {
		chain = chain || strings.Contains(err.Error(), "sdk/agent/core -> sdk/kernel/abi -> platform/host")
	}
	if !chain {
		t.Fatal("diagnostic must show the transitive dependency path")
	}
}

func TestFirewallRejectsTransitiveUnknownWorkspaceDependency(t *testing.T) {
	graph := importGraph{
		"sdk/agent/core": {"sdk/kernel/abi"},
		"sdk/kernel/abi": {"future/runtime"},
		"future/runtime": {"context"},
	}
	for _, failure := range validate(graph) {
		if strings.Contains(failure.Error(), "sdk/agent/core -> sdk/kernel/abi -> future/runtime") {
			return
		}
	}
	t.Fatal("unknown new workspace containers must fail through the entire transitive SDK path")
}

func TestFirewallRetainsExplicitModuleImportIdentity(t *testing.T) {
	// No scanned source node is required: a generated, excluded or missing local
	// package must not turn into an apparent standard-library dependency.
	graph := importGraph{
		"sdk/agent/core": {module + "sdk/kernel/abi"},
		"sdk/kernel/abi": {module + "future/runtime"},
	}
	for _, failure := range validate(graph) {
		if strings.Contains(failure.Error(), "sdk/agent/core -> sdk/kernel/abi -> future/runtime") {
			return
		}
	}
	t.Fatal("explicit module imports must retain local identity even without a source node")
}

func TestFirewallRejectsKernelAgentAndFrameworkLeakage(t *testing.T) {
	for _, graph := range []importGraph{
		{"sdk/kernel/core": {"sdk/agent/abi"}},
		{"sdk/kernel/abi": {"sdk/kernel/core"}},
		{"sdk/kernel/bus": {"sdk/llm/wire"}},
		{"harness": {"github.com/egoist/mygo/ui"}},
		{"sdk/agent/core": {"net/http", "github.com/egoist/mygo"}},
		{"apps/hub/core": {"harness"}},
		{"apps/hub": {"platform/storage/session"}},
		{"sdk/agent/context": {"platform/storage/session"}},
		{"sdk/agent/memory": {"platform/system/process"}},
		{"sdk/llm/wire": {"sdk/sandbox/abi"}},
		{"sdk/agent/core": {"sdk/sandbox/abi"}},
		{"sdk/sandbox/abi": {"platform/sandbox/host"}},
		{"sdk/sandbox/abi": {"sdk/kernel/abi"}},
		{"sdk/sandbox/abi": {"github.com/tetratelabs/wazero"}},
		{"sdk/kernel/abi": {"github.com/example/js-engine"}},
		{"cmd/alva": {"integration/engine"}},
		{"cmd/alva-desktop": {"apps/hub"}},
		{"sdk/kernel/abi": {"integration/llm/codec"}, "integration/llm/codec": {"github.com/QuantumNous/new-api/relaykit/dto"}},
		{"harness": {"github.com/QuantumNous/new-api/relaykit/dto"}},
		{"integration/llm/provider": {"github.com/QuantumNous/new-api/relaykit/dto"}},
		{"integration/llm/codec": {"sdk/kernel/abi"}},
		{"integration/llm/codec": {"github.com/QuantumNous/new-api/relaykit/relayconvert"}},
		{"integration/llm/provider": {"integration/llm/codec/internal/magpie"}},
		{"integration/llm/codec/internal/magpie": {"os"}},
		{"integration/llm/codec/internal/magpie": {"integration/llm/provider"}},
		{"integration/llm/codec/internal/magpie": {"github.com/yetone/magpie/internal/provider"}},
		{"integration/llm/codec": {"net/http", "integration/llm/provider"}},
	} {
		if failures := validate(graph); len(failures) == 0 {
			t.Errorf("forbidden graph accepted: %v", graph)
		}
	}
}

func TestFirewallAcceptsSeparateSDKHarnessAndNativeUI(t *testing.T) {
	graph := importGraph{
		"sdk/llm/wire": {"encoding/json"}, "sdk/kernel/bus": {"sync"},
		"sdk/kernel/abi": {"sdk/llm/wire", "context"}, "sdk/kernel/core": {"sdk/kernel/abi", "sdk/llm/wire"},
		"sdk/agent/core": {"sdk/kernel/core", "sdk/kernel/abi"}, "harness": {"sdk/agent/core"},
		"sdk/sandbox/abi": {"sdk/llm/wire", "encoding/json"},
		"platform/host":   {"harness"}, "apps/direct/desktop": {"platform/host", "github.com/egoist/mygo/ui"},
		"cmd/alva": {"platform/host"}, "cmd/alva-desktop": {"apps/direct/desktop"},
		"apps/gateway": {"integration/llm/codec"}, "integration/llm/provider": {"integration/llm/codec"},
		"integration/llm/codec":                 {"sdk/llm/wire", "integration/llm/codec/internal/magpie"},
		"integration/llm/codec/internal/magpie": {"encoding/json", "github.com/tidwall/gjson"},
		"integration/engine":                    {"context"}, "apps/hub": {"integration/engine", "sync"},
		"apps/hub/product":        {"apps/hub", "integration/engine/alva"},
		"integration/engine/alva": {"harness"},
		"apps/hub/desktop":        {"apps/hub/product", "github.com/egoist/mygo/ui"},
		"cmd/alva-hub-desktop":    {"apps/hub/desktop"},
		"cmd/alva-hub":            {"apps/hub/bootstrap", "apps/hub/product"},
	}
	if failures := validate(graph); len(failures) != 0 {
		t.Fatal(failures)
	}
}

func TestFirewallRejectsContainerAndTransitiveBypasses(t *testing.T) {
	for name, graph := range map[string]importGraph{
		"Hub bootstrap is not its contract":     {"apps/hub": {"apps/hub/bootstrap"}},
		"Hub product is not its contract":       {"apps/hub": {"apps/hub/product"}},
		"Hub SQLite is not its contract":        {"apps/hub": {"apps/hub/sqlite"}},
		"Hub server is not its contract":        {"apps/hub": {"apps/hub/server"}},
		"Hub desktop is not its contract":       {"apps/hub": {"apps/hub/desktop"}},
		"Hub adapter is not engine ABI":         {"apps/hub": {"integration/engine/alva"}},
		"Hub third party dependency":            {"apps/hub": {"example.com/runtime"}},
		"engine ABI imports its adapter":        {"integration/engine": {"integration/engine/process"}},
		"engine ABI imports SDK":                {"integration/engine": {"sdk/llm/wire"}},
		"engine ABI third party dependency":     {"integration/engine": {"example.com/runtime"}},
		"gateway is not desktop":                {"apps/gateway": {"github.com/egoist/mygo/ui"}},
		"Hub product is not desktop":            {"apps/hub/product": {"github.com/egoist/mygo/ui"}},
		"Hub bootstrap is not desktop":          {"apps/hub/bootstrap": {"github.com/egoist/mygo/ui"}},
		"apps prefix is not desktop":            {"apps/other": {"github.com/egoist/mygo/ui"}},
		"desktop prefix is exact":               {"apps/direct/desktop-extra": {"github.com/egoist/mygo/ui"}},
		"shared UI cannot own direct":           {"apps/ui/chat": {"apps/direct"}},
		"shared UI cannot own Hub":              {"apps/ui/chat": {"apps/hub/product"}},
		"shared UI cannot own execution":        {"apps/ui/modules": {"platform/host"}},
		"shared UI cannot inherit execution":    {"apps/ui/chat": {"platform/storage/session"}, "platform/storage/session": {"harness"}},
		"shared UI cannot own engines":          {"apps/ui/chat": {"integration/engine"}},
		"UI container is not an allowlist":      {"apps/ui/other": {"github.com/egoist/mygo/ui"}},
		"chat prefix is exact":                  {"apps/ui/chat-extra": {"github.com/egoist/mygo/ui"}},
		"command prefix is exact":               {"cmd-extra": {"github.com/egoist/mygo/ui"}},
		"SDK cannot import an unscanned entry":  {"sdk/agent/core": {"cmd/future"}},
		"platform cannot import an entry":       {"platform/system/process": {"cmd/alva"}},
		"products cannot import an entry":       {"apps/hub/product": {"cmd/alva-hub"}},
		"retired command location":              {"cmd/alva": {"apps/cmd/alva"}},
		"production cannot import validation":   {"harness": {"tests/architecture"}},
		"production cannot import probes":       {"apps/direct": {"tests/extensibility"}},
		"production cannot import tools":        {"cmd/alva": {"tools/qa"}},
		"transitive CLI engine import":          {"cmd/alva": {"platform/host"}, "platform/host": {"integration/engine"}},
		"transitive desktop Hub import":         {"apps/direct/desktop": {"platform/host"}, "platform/host": {"apps/hub"}},
		"transitive desktop entry adapter":      {"cmd/alva-desktop": {"apps/direct/desktop"}, "apps/direct/desktop": {"integration/engine/alva"}},
		"persistent direct is not a Hub bridge": {"apps/direct": {"platform/host"}, "platform/host": {"apps/hub/product"}},
		"transitive harness apps import":        {"harness": {"platform/host"}, "platform/host": {"apps/direct/desktop"}},
		"system upwards apps import":            {"platform/system/process": {"cmd/alva"}},
		"storage upwards apps import":           {"platform/storage/session": {"apps/hub"}},
		"engine adapter upwards apps":           {"integration/engine/alva": {"apps/hub/product"}},
		"protocol upwards apps":                 {"integration/protocol/mcp": {"apps/hub/bootstrap"}},
		"extension upwards apps":                {"extension/mcp": {"apps/hub/product"}},
		"timer is not its native Plugin":        {"extension/utility/timer": {"extension/utility"}},
		"timer cannot load native storage":      {"extension/utility/timer": {"platform/storage/session"}},
		"timer cannot load external SQLite":     {"extension/utility/timer": {"github.com/ncruces/go-sqlite3"}},
		"SDK new local top directory":           {"sdk/agent/core": {"future/runtime"}, "future/runtime": nil},
		"codec new local top directory":         {"integration/llm/codec": {"future/runtime"}, "future/runtime": nil},
		"SDK third party via another SDK":       {"sdk/agent/core": {"sdk/kernel/abi"}, "sdk/kernel/abi": {"example.com/runtime"}},
	} {
		t.Run(name, func(t *testing.T) {
			if failures := validate(graph); len(failures) == 0 {
				t.Fatalf("forbidden graph accepted: %v", graph)
			}
		})
	}
}

func TestL0RejectsEveryWorkspaceContainerAndUnknownPackage(t *testing.T) {
	for _, root := range []string{"sdk/llm/wire", "sdk/kernel/bus", "sdk/sandbox/abi"} {
		for _, dependency := range []string{"sdk/agent/abi", "integration/llm/codec", "extension/builtin", "harness", "platform/system/process", "apps/hub", "architecture", "future/runtime"} {
			t.Run(root+"->"+dependency, func(t *testing.T) {
				if failures := validate(importGraph{root: {dependency}, dependency: nil}); len(failures) == 0 {
					t.Fatal("L0 acquired workspace dependency through a container")
				}
			})
		}
	}
}

func TestFirewallRejectsRetiredImportPaths(t *testing.T) {
	for _, retired := range []string{"llm/wire", "kernel/abi", "agent/core", "app", "host", "desktop", "protocol/mcp", "engine", "hub", "cmd/alva", "native/process", "storage/session", "sandbox/abi"} {
		t.Run(retired, func(t *testing.T) {
			if failures := validate(importGraph{"harness": {retired}}); len(failures) == 0 {
				t.Fatalf("retired import %s could silently bypass the moved source graph", retired)
			}
		})
	}
}
