// INPUT: Explicit durable CLI operations, the existing journal/controller and committed SDK events.
// OUTPUT: Cold wait inspection/resolution/abandonment and honest suspended or completed CLI output.
// POS: [L6 application] Restarts from persisted waiting facts; unknown execution is never replayed or shown as success.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/QuincySx/Alva/harness/durable"
	durablejournal "github.com/QuincySx/Alva/harness/durable/journal"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type cliDurableOptions struct {
	enabled, resume, list, abandon bool
	resolve, binding, resultFile   string
	revision                       uint64
	revisionSet                    bool
}

func (o *cliDurableOptions) flags(flags *flag.FlagSet) {
	flags.BoolVar(&o.enabled, "durable", false, "opt in to persisted waits/approvals; requires native --data-dir")
	flags.BoolVar(&o.resume, "resume-wait", false, "explicitly continue this session's resolved durable checkpoint, with no new prompt")
	flags.BoolVar(&o.list, "waits", false, "read the bounded durable checkpoint without starting execution")
	flags.StringVar(&o.resolve, "resolve-wait", "", "resolve exactly this pending request ID without executing an Agent")
	flags.StringVar(&o.binding, "wait-binding", "", "exact inspected payload binding for --resolve-wait")
	flags.StringVar(&o.resultFile, "wait-result-file", "", "explicit regular JSON result file, at most16 KiB; never discovered")
	flags.BoolVar(&o.abandon, "abandon-wait", false, "explicitly reset the reviewed durable state; does not cancel external work or undo effects")
	flags.Func("wait-revision", "exact reviewed revision required by --abandon-wait (including initial revision0)", func(value string) error {
		n, err := strconv.ParseUint(value, 10, 64)
		if err == nil {
			o.revision, o.revisionSet = n, true
		}
		return err
	})
}
func (o cliDurableOptions) cold() bool { return o.list || o.resolve != "" || o.abandon }
func (o cliDurableOptions) validate(dataDir, session, prompt, sandbox string, repl bool, attachments int) error {
	n := 0
	if o.list {
		n++
	}
	if o.resolve != "" {
		n++
	}
	if o.abandon {
		n++
	}
	if n > 1 || o.cold() && (o.enabled || o.resume) {
		return errors.New("choose one durable control or execution operation")
	}
	if (o.enabled || o.resume || o.cold()) && (dataDir == "" || sandbox != "") {
		return errors.New("durable waits require explicit native --data-dir")
	}
	if (o.resume || o.cold()) && session == "" {
		return errors.New("durable control/resume requires an existing --session")
	}
	if o.resume && (prompt != "" || attachments != 0 || repl) {
		return errors.New("--resume-wait supplies no new prompt, attachments or REPL input")
	}
	if o.resolve == "" && (o.binding != "" || o.resultFile != "") || o.resolve != "" && (!validCLIHash(o.binding) || o.resultFile == "") {
		return errors.New("resolve requires request ID, exact binding and an explicit result file")
	}
	if o.abandon != o.revisionSet {
		return errors.New("abandon requires the exact reviewed --wait-revision")
	}
	return nil
}

func controlCLIWait(ctx context.Context, repository storage.Repository, log storage.Session, o cliDurableOptions, output io.Writer) error {
	backend, err := durablejournal.Open(ctx, repository, cliOwner, log.ID())
	if err != nil {
		return err
	}
	var selected durable.Backend = backend
	if o.abandon {
		selected = cliReviewedBackend{Backend: backend, expected: o.revision, repository: repository, log: log}
	}
	controller, err := durable.New(durable.Config{Backend: selected, TTL: 24 * time.Hour})
	if err != nil {
		return err
	}
	if o.resolve != "" {
		data, err := readCLIJSONFile(ctx, o.resultFile, 16<<10)
		if err != nil {
			return err
		}
		if !wire.ValidJSON(data) {
			return errors.New("invalid JSON wait result")
		}
		if err = controller.Resolve(ctx, durable.Resolution{RequestID: o.resolve, Binding: o.binding, Result: data}); err != nil {
			return err
		}
	}
	if o.abandon {
		if err = controller.Abandon(ctx); err != nil {
			return err
		}
	}
	snapshot, err := controller.Snapshot(ctx)
	if err != nil {
		return err
	}
	pending, err := cliPendingLaunch(ctx, repository, log)
	if err != nil {
		return err
	}
	return writeCLIJSON(output, struct {
		durable.Snapshot
		PluginStartupUnknown bool `json:"plugin_startup_unknown,omitempty"`
	}{snapshot, pending != 0})
}

type cliReviewedBackend struct {
	durable.Backend
	expected   uint64
	repository storage.Repository
	log        storage.Session
}

func (b cliReviewedBackend) Commit(ctx context.Context, revision uint64, state durable.State, messages []wire.Message) (durable.Snapshot, error) {
	if revision != b.expected {
		return durable.Snapshot{}, durable.ErrConflict
	}
	pending, err := cliPendingLaunch(ctx, b.repository, b.log)
	if err != nil {
		return durable.Snapshot{}, err
	}
	snapshot, err := b.Backend.Commit(ctx, revision, state, messages)
	if err != nil {
		return durable.Snapshot{}, err
	}
	if pending != 0 {
		err = settleCLILaunch(ctx, b.repository, b.log, pending, "operator_acknowledged_unknown")
	}
	return snapshot, err
}

type cliDurableRecipe struct {
	Version int    `json:"version"`
	SHA256  string `json:"sha256"`
}

func bindCLIDurableRecipe(ctx context.Context, repository storage.Repository, log storage.Session, route string, resume bool) error {
	var recipe cliDurableRecipe
	revision, err := loadCLIValue(ctx, repository, log, "durable_recipe", &recipe)
	if err != nil {
		return err
	}
	if revision != 0 && (recipe.Version != 1 || !validCLIHash(recipe.SHA256)) {
		return errors.New("invalid durable CLI recipe binding")
	}
	if resume {
		if revision == 0 || recipe.SHA256 != route {
			return errors.New("durable resume requires the inspected model, workspace, permission and enabled plugin recipe")
		}
		return nil
	}
	backend, err := durablejournal.Open(ctx, repository, cliOwner, log.ID())
	if err != nil {
		return err
	}
	snapshot, err := backend.Load(ctx)
	if err != nil {
		return err
	}
	if snapshot.State.Status == durable.Running || snapshot.State.Status == durable.Interrupted {
		return durable.ErrInterrupted
	}
	if snapshot.State.Status != durable.Idle {
		return errors.New("waiting checkpoint requires explicit --resume-wait; new input was not submitted")
	}
	if revision != 0 && recipe.SHA256 == route {
		return nil
	}
	return commitCLIValue(ctx, log, "durable_recipe", revision, cliDurableRecipe{Version: 1, SHA256: route})
}

func cliRecipeHash(config cliNativeConfig, workspace string, state cliPluginSettings) (string, error) {
	canonical, err := filepath.EvalSymlinks(workspace)
	if err != nil {
		return "", err
	}
	var enabled []cliPluginPin
	for _, pin := range state.Plugins {
		if pin.Enabled {
			enabled = append(enabled, pin)
		}
	}
	configSHA := ""
	if len(enabled) > 0 {
		configSHA = state.FileSHA256
	}
	data, _ := json.Marshal(struct {
		Route     string
		Workspace string
		Mode      string
		Memory    bool
		ConfigSHA string
		Plugins   []cliPluginPin
	}{config.RouteHash, canonical, string(config.Mode), config.Memory, configSHA, enabled})
	return cliSHA(append([]byte("alva.cli.durable.recipe.v1\x00"), data...)), nil
}

func runCLIDurable(ctx context.Context, runtime *cliNativeRuntime, input []wire.Message, output, diagnostics io.Writer) error {
	var streamed strings.Builder
	var committed string
	emit := func(event abi.Event) error {
		if event.Kind == abi.EventTextDelta {
			if streamed.Len()+len(event.Text) > 1<<20 {
				return errors.New("CLI draft exceeds 1 MiB")
			}
			if err := writeString(output, event.Text); err != nil {
				return err
			}
			streamed.WriteString(event.Text)
		}
		if event.Kind == abi.EventMessageCommitted && event.Message != nil && event.Message.Role == wire.RoleAssistant {
			committed = event.Message.Text()
			for _, block := range event.Message.Content {
				if block.Kind == wire.ContentToolCall {
					committed = ""
					break
				}
			}
			if len(committed) > 1<<20 {
				return errors.New("CLI committed response exceeds 1 MiB")
			}
		}
		return ctx.Err()
	}
	// Consume events in the caller while the actual execution owner can be
	// cancelled and joined independently. A waiting result has no host success
	// terminal, so it must not pass through host.Controller's normal Run(nil).
	operation, cancel := context.WithCancel(ctx)
	defer cancel()
	events := make(chan abi.Event, 128)
	done := make(chan error, 1)
	go func() {
		err := runtime.Run(operation, input, func(event abi.Event) error {
			select {
			case events <- abi.CloneEvent(event):
				return nil
			case <-operation.Done():
				return operation.Err()
			}
		})
		close(events)
		done <- err
	}()
	stop := func(cause error) error {
		cancel()
		timer := time.NewTimer(5 * time.Second)
		defer timer.Stop()
		select {
		case err := <-done:
			return errors.Join(cause, err)
		case <-timer.C:
			return errors.Join(cause, fmt.Errorf("durable execution shutdown incomplete: %w", context.DeadlineExceeded))
		}
	}
	for events != nil {
		select {
		case <-ctx.Done():
			return stop(ctx.Err())
		case event, open := <-events:
			if !open {
				events = nil
				break
			}
			if err := emit(event); err != nil {
				return stop(err)
			}
		}
	}
	if err := <-done; err != nil {
		return err
	}
	runtime.mu.Lock()
	outcome := runtime.outcome
	runtime.mu.Unlock()
	if outcome.Suspended {
		if streamed.Len() > 0 {
			if err := writeString(output, "\n"); err != nil {
				return err
			}
		}
		if err := writeString(diagnostics, "Task suspended at a durable checkpoint. Inspect --waits, resolve an exact request binding, then explicitly --resume-wait. No external task was automatically started.\n"); err != nil {
			return err
		}
		return writeCLIJSON(output, struct {
			Status    string            `json:"status"`
			SessionID string            `json:"session_id"`
			Requests  []durable.Request `json:"requests"`
		}{"waiting", runtime.log.ID(), outcome.Requests})
	}
	if !strings.HasPrefix(committed, streamed.String()) {
		if err := writeString(output, "\n[Committed response replaces the streamed draft]\n"+committed); err != nil {
			return err
		}
	} else if err := writeString(output, committed[streamed.Len():]); err != nil {
		return err
	}
	return writeString(output, "\n")
}

func cliSuspended(runner interface{}) bool {
	r, ok := runner.(*cliNativeRuntime)
	if !ok {
		return false
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.outcome.Suspended
}
