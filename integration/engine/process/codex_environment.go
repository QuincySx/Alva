// INPUT: Approved native environment snapshot, explicit overrides and CLI configuration directory
// OUTPUT: Codex-only composed environment and bounded known credential snapshot
// POS: [integration / native external-engine adapter] Native toolchain context is inherited without reading configuration, trust or credential files
package process

import (
	"path/filepath"
	"runtime"
	"sort"
	"strings"

	"github.com/QuincySx/Alva/platform/storage/redaction"
	private "github.com/QuincySx/Alva/platform/system/privatefs"
)

// Environment merge/defaults follow makecindy/cindy@5f9a219d8d789ac96de779e17c61b8126845f1bd:
// packages/maker-core/src/agents/codex/env-builder.ts and agents/shared/terminal-output.ts.
// Its desktop runtime-configs.ts supplies no HOME override. Alva has no bundled
// PATH prepend or optional toolchain-cap setting to contribute here.
func (a *Adapter) codexEnvironment() (map[string]string, error) {
	codexHome := a.config.CLIConfigDirectory
	if codexHome == "" {
		var err error
		codexHome, err = private.EnsureDir(filepath.Join(a.config.StateDirectory, "home", "codex"))
		if err != nil {
			return nil, err
		}
	}
	return composeCodexEnvironment(a.nativeEnvironment(), a.config.Env, codexHome, runtime.GOOS == "windows"), nil
}

func composeCodexEnvironment(inherited []string, overrides map[string]string, codexHome string, windows bool) map[string]string {
	env := make(map[string]string)
	for _, entry := range inherited {
		if key, value, ok := strings.Cut(entry, "="); ok {
			env[key] = value
		}
	}
	set := func(key, value string) {
		if windows {
			for inherited := range env {
				if strings.EqualFold(inherited, key) {
					delete(env, inherited)
				}
			}
		}
		env[key] = value
	}
	for key, value := range overrides {
		if key == "NO_PROXY" || key == "no_proxy" || windows && strings.EqualFold(key, "NO_PROXY") {
			env[key] = value
		} else {
			set(key, value)
		}
	}
	set("CODEX_HOME", codexHome)
	for key, value := range map[string]string{"PYTHONUTF8": "1", "PYTHONIOENCODING": "utf-8", "NO_COLOR": "1", "CLICOLOR": "0", "FORCE_COLOR": "0", "TERM": "dumb", "PSStyle__OutputRendering": "PlainText"} {
		_, exists := env[key]
		if !exists && windows {
			for inherited := range env {
				if strings.EqualFold(inherited, key) {
					exists = true
					break
				}
			}
		}
		if !exists {
			set(key, value)
		}
	}
	var bypass []string
	seen := make(map[string]bool)
	proxyValues := []string{env["NO_PROXY"], env["no_proxy"]}
	if windows {
		var aliases []string
		for key := range env {
			if key != "NO_PROXY" && key != "no_proxy" && strings.EqualFold(key, "NO_PROXY") {
				aliases = append(aliases, key)
			}
		}
		sort.Strings(aliases)
		for _, key := range aliases {
			proxyValues = append(proxyValues, env[key])
		}
	}
	for _, value := range append(proxyValues, "127.0.0.1,localhost,::1") {
		for _, entry := range strings.Split(value, ",") {
			entry = strings.TrimSpace(entry)
			if entry != "" && !seen[entry] {
				seen[entry] = true
				bypass = append(bypass, entry)
			}
		}
	}
	delete(env, "no_proxy")
	set("NO_PROXY", strings.Join(bypass, ","))
	return env
}

// Inherited public paths/locales are not credentials. Only known credential
// keys join the existing conservative policy for explicitly supplied overrides.
func codexEnvironmentSecrets(effective, overrides map[string]string) []string {
	known := make(map[string]bool)
	for _, value := range redaction.EnvironmentSecrets(overrides) {
		known[value] = true
	}
	for key, value := range effective {
		key = strings.ToUpper(strings.NewReplacer("-", "_", ".", "_").Replace(key))
		credential := key == "KEY" || key == "TOKEN" || key == "SECRET" || key == "APIKEY" || key == "PASSWORD" || key == "AUTH" || key == "AUTHORIZATION" || key == "COOKIE" || key == "SET_COOKIE" || strings.HasSuffix(key, "_KEY") || strings.HasSuffix(key, "_TOKEN") || strings.HasSuffix(key, "_SECRET") || strings.HasSuffix(key, "_PASSWORD") || strings.HasSuffix(key, "_AUTH") || strings.HasSuffix(key, "_COOKIE")
		if value != "" && credential {
			known[value] = true
		}
	}
	values := make([]string, 0, len(known))
	for value := range known {
		values = append(values, value)
	}
	sort.Slice(values, func(i, j int) bool {
		if len(values[i]) != len(values[j]) {
			return len(values[i]) > len(values[j])
		}
		return values[i] < values[j]
	})
	return values
}
