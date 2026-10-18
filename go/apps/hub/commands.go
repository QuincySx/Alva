// INPUT: Explicit command argv/environment snapshots and product next-turn lifecycle/diagnostic choices
// OUTPUT: NativeCommand, HookSpec, DiagnosticSpec and bounded pure CommandSettings validation
// POS: [L7 Hub product] Product configuration values only; no process/protocol/SDK imports, scans or execution
package hub

import (
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

type NativeCommand struct {
	Command            string            `json:"command"`
	Args               []string          `json:"args,omitempty"`
	Env                map[string]string `json:"env,omitempty"`
	TimeoutMS          int               `json:"timeout_ms,omitempty"`
	Enabled            bool              `json:"enabled"`
	HostAccessApproved bool              `json:"host_access_approved"`
}
type HookSpec struct {
	ID      string `json:"id"`
	Event   string `json:"event"`
	Matcher string `json:"matcher,omitempty"`
	NativeCommand
}
type DiagnosticSpec struct {
	ID          string `json:"id"`
	Description string `json:"description,omitempty"`
	NativeCommand
}
type CommandSettings struct {
	Hooks       []HookSpec       `json:"hooks"`
	Diagnostics []DiagnosticSpec `json:"diagnostics"`
}

func validCommandID(id string) bool {
	if id == "" || len(id) > 128 {
		return false
	}
	for _, char := range id {
		if !(char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || strings.ContainsRune("._:-", char)) {
			return false
		}
	}
	return true
}

func (c NativeCommand) validate(maxTimeout int) (int, error) {
	if !filepath.IsAbs(c.Command) || len(c.Command) > 4096 || !utf8.ValidString(c.Command) || strings.ContainsRune(c.Command, 0) || len(c.Args) > 128 || len(c.Env) > 64 || c.TimeoutMS < 0 || c.TimeoutMS > maxTimeout {
		return 0, errors.New("explicit absolute command, bounded argv/env and supported timeout required")
	}
	if c.Enabled && !c.HostAccessApproved {
		return 0, errors.New("enabled native command requires separate full host access approval")
	}
	bytes := len(c.Command)
	for _, arg := range c.Args {
		if strings.ContainsRune(arg, 0) || !utf8.ValidString(arg) {
			return 0, errors.New("invalid native command argument")
		}
		bytes += len(arg)
	}
	for key, value := range c.Env {
		if key == "" || len(key) > 128 || strings.ContainsAny(key, "=\x00") || len(value) > 8192 || strings.ContainsRune(value, 0) || !utf8.ValidString(key+value) {
			return 0, errors.New("invalid explicit native command environment")
		}
		bytes += len(key) + len(value)
	}
	return bytes, nil
}

func (c CommandSettings) Validate() error {
	if len(c.Hooks) > 32 || len(c.Diagnostics) > 16 {
		return errors.New("at most 32 hooks and 16 diagnostic profiles per product session")
	}
	seen := map[string]bool{}
	bytes := 0
	for _, hook := range c.Hooks {
		if !validCommandID(hook.ID) || seen[hook.ID] || len(hook.Matcher) > 256 || !utf8.ValidString(hook.Matcher) || strings.Contains(strings.Trim(hook.Matcher, "*"), "*") {
			return errors.New("invalid/duplicate hook identity or edge-wildcard matcher")
		}
		seen[hook.ID] = true
		switch hook.Event {
		case "PreToolUse", "PostToolUse", "PostToolUseFailure", "SessionStart", "SessionEnd", "Stop", "UserPromptSubmit", "Notification":
		default:
			return errors.New("unknown native lifecycle hook event")
		}
		n, err := hook.NativeCommand.validate(60000)
		if err != nil {
			return err
		}
		bytes += n + len(hook.Matcher)
	}
	seen = map[string]bool{}
	for _, profile := range c.Diagnostics {
		if !validCommandID(profile.ID) || seen[profile.ID] || len(profile.Description) > 512 || !utf8.ValidString(profile.Description) {
			return errors.New("invalid/duplicate diagnostic profile metadata")
		}
		seen[profile.ID] = true
		n, err := profile.NativeCommand.validate(300000)
		if err != nil {
			return err
		}
		bytes += n + len(profile.Description)
	}
	if bytes > 64<<10 {
		return errors.New("native command snapshots exceed 64 KiB total budget")
	}
	return nil
}
