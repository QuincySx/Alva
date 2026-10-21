// INPUT: Explicit plugin JSON file, actual CLI session, loader snapshots and approved configuration/code pins.
// OUTPUT: Cold inspect/list/enable/disable and per-turn owned JS/Python/WASIp1 Plugin loading.
// POS: [L6 application] Operator plugin selection over existing AEP; raw arguments/environment stay in the selected source file.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"path/filepath"
	"strings"
	"sync"
	"unicode"
	"unicode/utf8"

	"github.com/QuincySx/Alva/extension/loader"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/system/privatefs"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

const cliPluginsType = "plugins"

type cliPluginOptions struct {
	inspect, enable, disable, review string
	list                             bool
}

func (o *cliPluginOptions) flags(f *flag.FlagSet) {
	f.StringVar(&o.inspect, "inspect-plugins", "", "inspect an explicit plugin JSON file and save disabled review pins; executes no code")
	f.BoolVar(&o.list, "plugins", false, "list this session's inspected plugin pins without execution")
	f.StringVar(&o.enable, "enable-plugin", "", "enable exactly one inspected plugin ID for subsequent native turns")
	f.StringVar(&o.review, "approve-plugin-pin", "", "exact inspected review SHA256 for --enable-plugin, including launch settings and code")
	f.StringVar(&o.disable, "disable-plugin", "", "disable exactly one inspected plugin ID")
}
func (o cliPluginOptions) cold() bool {
	return o.inspect != "" || o.list || o.enable != "" || o.disable != ""
}
func (o cliPluginOptions) validate(dataDir, session string) error {
	n := 0
	if o.inspect != "" {
		n++
	}
	if o.list {
		n++
	}
	if o.enable != "" {
		n++
	}
	if o.disable != "" {
		n++
	}
	if n > 1 || o.enable == "" && o.review != "" || o.enable != "" && !validCLIHash(o.review) {
		return errors.New("choose one plugin operation; enable requires the inspected --approve-plugin-pin")
	}
	if o.cold() && (dataDir == "" || o.inspect == "" && session == "") {
		return errors.New("plugin controls require --data-dir and an existing --session; inspect may create a new session")
	}
	return nil
}
func controlCLIPlugins(ctx context.Context, repository storage.Repository, log storage.Session, dir string, o cliPluginOptions, output io.Writer) error {
	if o.list {
		state, _, err := loadCLIPlugins(ctx, repository, log)
		if err != nil {
			return err
		}
		return writeCLIJSON(output, state)
	}
	_, release, err := repository.BeginRun(ctx, cliOwner, log.ID())
	if err != nil {
		return err
	}
	defer release()
	if o.inspect != "" {
		return inspectCLIPlugins(ctx, repository, log, dir, o.inspect, output)
	}
	if o.enable != "" {
		return changeCLIPlugin(ctx, repository, log, o.enable, o.review, true, output)
	}
	return changeCLIPlugin(ctx, repository, log, o.disable, "", false, output)
}

type cliPluginDraft struct {
	ID              string            `json:"id"`
	SourceDirectory string            `json:"source_directory"`
	Interpreter     string            `json:"interpreter,omitempty"`
	InterpreterArgs []string          `json:"interpreter_args,omitempty"`
	Env             map[string]string `json:"env,omitempty"`
}
type cliPluginPin struct {
	ID           string `json:"id"`
	Name         string `json:"name"`
	Version      string `json:"version"`
	Runtime      string `json:"runtime"`
	CodeSHA256   string `json:"code_sha256"`
	ReviewSHA256 string `json:"review_sha256"`
	Enabled      bool   `json:"enabled"`
}
type cliPluginSettings struct {
	Version    int            `json:"version"`
	File       string         `json:"file,omitempty"`
	FileSHA256 string         `json:"file_sha256,omitempty"`
	Plugins    []cliPluginPin `json:"plugins"`
}

func cliSHA(data []byte) string { sum := sha256.Sum256(data); return hex.EncodeToString(sum[:]) }
func validCLIHash(value string) bool {
	b, err := hex.DecodeString(value)
	return err == nil && len(b) == 32
}
func validCLIIdentity(value string, limit int) bool {
	return value != "" && len(value) <= limit && utf8.ValidString(value) && strings.IndexFunc(value, unicode.IsControl) == -1
}
func pluginReview(draft cliPluginDraft, code string) string {
	data, _ := json.Marshal(struct {
		Draft cliPluginDraft
		Code  string
	}{draft, code})
	return cliSHA(append([]byte("alva.cli.aep.review.v1\x00"), data...))
}
func loadCLIPlugins(ctx context.Context, repository storage.Repository, log storage.Session) (cliPluginSettings, uint64, error) {
	state := cliPluginSettings{Version: 1, Plugins: []cliPluginPin{}}
	revision, err := loadCLIValue(ctx, repository, log, cliPluginsType, &state)
	if err != nil {
		return state, revision, err
	}
	if state.Version != 1 || len(state.Plugins) > 8 || (state.File != "" && (!filepath.IsAbs(state.File) || !validCLIIdentity(state.File, 4096) || !validCLIHash(state.FileSHA256))) || (len(state.Plugins) != 0 && state.File == "") {
		return state, revision, errors.New("unsupported or invalid CLI plugin snapshot")
	}
	seen, names := map[string]bool{}, map[string]bool{}
	for _, pin := range state.Plugins {
		if !validCLIIdentity(pin.ID, 128) || !validCLIIdentity(pin.Name, 64) || !validCLIIdentity(pin.Version, 64) || seen[pin.ID] || names[pin.Name] || !validCLIHash(pin.CodeSHA256) || !validCLIHash(pin.ReviewSHA256) || (pin.Runtime != "python" && pin.Runtime != "javascript" && pin.Runtime != "wasm") {
			return state, revision, errors.New("invalid duplicate plugin identity or approval pin")
		}
		seen[pin.ID], names[pin.Name] = true, true
	}
	return state, revision, nil
}

func readPluginDrafts(ctx context.Context, path string) ([]cliPluginDraft, []byte, error) {
	data, err := readCLIJSONFile(ctx, path, 64<<10)
	if err != nil {
		return nil, nil, err
	}
	if trimmed := bytes.TrimSpace(data); len(trimmed) == 0 || trimmed[0] != '[' {
		return nil, nil, errors.New("plugin configuration must be an explicit JSON array")
	}
	var drafts []cliPluginDraft
	var envelope struct {
		Plugins []cliPluginDraft `json:"plugins"`
	}
	if err = decodeCLIJSON(append(append([]byte(`{"plugins":`), data...), '}'), &envelope); err != nil {
		return nil, nil, err
	}
	drafts = envelope.Plugins
	if len(drafts) > 8 {
		return nil, nil, errors.New("at most eight explicit plugins")
	}
	seen := map[string]bool{}
	for _, draft := range drafts {
		if !validCLIIdentity(draft.ID, 128) || seen[draft.ID] || !filepath.IsAbs(draft.SourceDirectory) || !validCLIIdentity(draft.SourceDirectory, 4096) || len(draft.InterpreterArgs) > 128 || len(draft.Env) > 64 {
			return nil, nil, errors.New("plugin requires unique identity, absolute source and bounded launch settings")
		}
		seen[draft.ID] = true
		if draft.Interpreter != "" && (!filepath.IsAbs(draft.Interpreter) || !validCLIIdentity(draft.Interpreter, 4096)) {
			return nil, nil, errors.New("native plugin interpreter must be explicit and absolute")
		}
		for _, arg := range draft.InterpreterArgs {
			if !utf8.ValidString(arg) || strings.ContainsRune(arg, 0) {
				return nil, nil, errors.New("invalid explicit interpreter argument")
			}
		}
		for key, value := range draft.Env {
			if key == "" || len(key) > 128 || strings.ContainsAny(key, "=\x00") || len(value) > 8192 || strings.ContainsRune(value, 0) || !utf8.ValidString(key+value) {
				return nil, nil, errors.New("invalid explicit plugin environment")
			}
		}
	}
	return drafts, data, nil
}
func validatePluginRuntime(draft cliPluginDraft, manifest loader.Manifest) error {
	if manifest.Runtime == "wasm" {
		if draft.Interpreter != "" || len(draft.InterpreterArgs) != 0 || len(draft.Env) != 0 {
			return errors.New("WASM plugins use embedded execution; interpreter/args/env must be empty")
		}
	} else if draft.Interpreter == "" {
		return errors.New("JS/Python plugins require an explicit absolute interpreter")
	}
	return nil
}

func inspectCLIPlugins(ctx context.Context, repository storage.Repository, log storage.Session, dataDir, file string, output io.Writer) error {
	path, err := filepath.Abs(file)
	if err != nil {
		return err
	}
	drafts, data, err := readPluginDrafts(ctx, path)
	if err != nil {
		return err
	}
	_, revision, err := loadCLIPlugins(ctx, repository, log)
	if err != nil {
		return err
	}
	state := cliPluginSettings{Version: 1, File: path, FileSHA256: cliSHA(data), Plugins: []cliPluginPin{}}
	parent, err := privatefs.EnsureDir(filepath.Join(dataDir, "aep-snapshots"))
	if err != nil {
		return err
	}
	for _, draft := range drafts {
		manifest, err := loader.ReadManifest(ctx, draft.SourceDirectory)
		if err != nil {
			return err
		}
		if err = validatePluginRuntime(draft, manifest); err != nil {
			return err
		}
		bundle, err := loader.Stage(ctx, draft.SourceDirectory, parent, manifest)
		if err != nil {
			return err
		}
		pin := cliPluginPin{ID: draft.ID, Name: manifest.Name, Version: manifest.Version, Runtime: manifest.Runtime, CodeSHA256: bundle.Digest(), ReviewSHA256: pluginReview(draft, bundle.Digest())}
		if err = bundle.Close(); err != nil {
			return &cliBundleResource{bundle: bundle, failure: err}
		}
		for _, existing := range state.Plugins {
			if existing.Name == pin.Name {
				return errors.New("duplicate manifest plugin names")
			}
		}
		state.Plugins = append(state.Plugins, pin)
	}
	if err = commitCLIValue(ctx, log, cliPluginsType, revision, state); err != nil {
		return err
	}
	return writeCLIJSON(output, state)
}

func changeCLIPlugin(ctx context.Context, repository storage.Repository, log storage.Session, id, review string, enable bool, output io.Writer) error {
	state, revision, err := loadCLIPlugins(ctx, repository, log)
	if err != nil {
		return err
	}
	for i := range state.Plugins {
		if state.Plugins[i].ID != id {
			continue
		}
		if enable && (!validCLIHash(review) || state.Plugins[i].ReviewSHA256 != review) {
			return errors.New("enable requires the exact inspected review pin; source, interpreter, args and environment are included")
		}
		state.Plugins[i].Enabled = enable
		if err = commitCLIValue(ctx, log, cliPluginsType, revision, state); err != nil {
			return err
		}
		return writeCLIJSON(output, state)
	}
	return errors.New("plugin ID is not in this session's inspected configuration")
}

func openCLIPlugins(ctx context.Context, repository storage.Repository, log storage.Session, state cliPluginSettings, dataDir, workspace string, guard *security.Guard, retain func(cliResource), launched func(uint64), diagnostics io.Writer) ([]core.Plugin, []abi.Tool, error) {
	active := false
	for _, pin := range state.Plugins {
		active = active || pin.Enabled
	}
	if !active {
		return nil, nil, nil
	}
	drafts, data, err := readPluginDrafts(ctx, state.File)
	if err != nil {
		return nil, nil, err
	}
	if cliSHA(data) != state.FileSHA256 || len(drafts) != len(state.Plugins) {
		return nil, nil, errors.New("plugin configuration changed since inspection; inspect and approve again")
	}
	parent, err := privatefs.EnsureDir(filepath.Join(dataDir, "aep-snapshots"))
	if err != nil {
		return nil, nil, err
	}
	var plugins []core.Plugin
	var tools []abi.Tool
	for index, pin := range state.Plugins {
		if !pin.Enabled {
			continue
		}
		draft := drafts[index]
		if draft.ID != pin.ID || pluginReview(draft, pin.CodeSHA256) != pin.ReviewSHA256 {
			return nil, nil, errors.New("plugin launch settings differ from the approved pin")
		}
		manifest, err := loader.ReadManifest(ctx, draft.SourceDirectory)
		if err != nil {
			return nil, nil, err
		}
		if err = validatePluginRuntime(draft, manifest); err != nil {
			return nil, nil, err
		}
		if manifest.Name != pin.Name || manifest.Version != pin.Version || manifest.Runtime != pin.Runtime {
			return nil, nil, errors.New("plugin manifest changed since inspection")
		}
		bundle, err := loader.Stage(ctx, draft.SourceDirectory, parent, manifest)
		if err != nil {
			return nil, nil, err
		}
		staged := &cliBundleResource{bundle: bundle}
		retain(staged)
		if bundle.Digest() != pin.CodeSHA256 {
			return nil, nil, errors.New("plugin code changed since approval")
		}
		plugin, err := loader.OpenPlugin(ctx, loader.PluginConfig{Bundle: bundle, Interpreter: draft.Interpreter, InterpreterArgs: draft.InterpreterArgs, Env: draft.Env, Workspace: workspace, Repository: repository, Owner: cliOwner, NativeID: log.ID(), Guard: guard, ApprovedHostAccess: true, BeforeLaunch: func(ctx context.Context) error {
			if err := ctx.Err(); err != nil {
				return err
			}
			sequence, err := commitCLILaunch(ctx, log, pin)
			if err != nil {
				return err
			}
			launched(sequence)
			return writeString(diagnostics, fmt.Sprintf("Approved plugin %s (%s), review %s; execution risk committed before launch.\n", pin.ID, pin.Runtime, pin.ReviewSHA256))
		}})
		if plugin != nil {
			staged.mu.Lock()
			staged.transferred = true
			staged.mu.Unlock()
			retain(plugin)
		}
		if err != nil {
			return nil, nil, err
		}
		plugins, tools = append(plugins, plugin), append(tools, plugin.Tool())
	}
	return plugins, tools, nil
}

// Staged code remains owned even if OpenPlugin fails before transferring it.
// After transfer the Plugin owns deletion, ordered after its actual peer join.
type cliBundleResource struct {
	mu                   sync.Mutex
	bundle               *loader.Bundle
	transferred, cleaned bool
	failure              error
}

func (b *cliBundleResource) Close(ctx context.Context) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.transferred || b.cleaned {
		return nil
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	b.failure = b.bundle.Close()
	b.cleaned = b.failure == nil
	return b.failure
}
func (b *cliBundleResource) CleanupComplete() bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.transferred || b.cleaned
}
func (b *cliBundleResource) Error() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return "plugin bundle cleanup incomplete: " + fmt.Sprint(b.failure)
}

func commitCLILaunch(ctx context.Context, log storage.Session, pin cliPluginPin) (uint64, error) {
	data, _ := json.Marshal(struct{ ID, ReviewSHA256, Runtime string }{pin.ID, pin.ReviewSHA256, pin.Runtime})
	records, err := log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: cliNamespace, Type: "plugin_launch_intent", SchemaVersion: 1, Payload: data}})
	if err != nil {
		return 0, err
	}
	if len(records) != 1 || records[0].Sequence == 0 {
		return 0, errors.New("plugin launch intent was not committed")
	}
	return records[0].Sequence, nil
}

type cliLaunchSettlement struct {
	Version     int    `json:"version"`
	Through     uint64 `json:"through"`
	Disposition string `json:"disposition"`
}

func cliPendingLaunch(ctx context.Context, repository storage.Repository, log storage.Session) (uint64, error) {
	var intent struct{ ID, ReviewSHA256, Runtime string }
	sequence, err := loadCLIValue(ctx, repository, log, "plugin_launch_intent", &intent)
	if err != nil || sequence == 0 {
		return 0, err
	}
	var settlement cliLaunchSettlement
	revision, err := loadCLIValue(ctx, repository, log, "plugin_launch_settlement", &settlement)
	if err != nil {
		return 0, err
	}
	if revision != 0 && (settlement.Version != 1 || settlement.Through == 0 || settlement.Through > sequence || settlement.Disposition != "resources_reaped" && settlement.Disposition != "operator_acknowledged_unknown") {
		return 0, errors.New("invalid CLI plugin launch settlement")
	}
	if sequence > settlement.Through {
		return sequence, nil
	}
	return 0, nil
}
func settleCLILaunch(ctx context.Context, repository storage.Repository, log storage.Session, sequence uint64, disposition string) error {
	var previous cliLaunchSettlement
	revision, err := loadCLIValue(ctx, repository, log, "plugin_launch_settlement", &previous)
	if err != nil {
		return err
	}
	return commitCLIValue(ctx, log, "plugin_launch_settlement", revision, cliLaunchSettlement{Version: 1, Through: sequence, Disposition: disposition})
}
