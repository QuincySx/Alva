// INPUT: context, encoding/json, errors, fmt, os, os/exec, path/filepath, testing, time, platform/storage/session, agent/core, kernel/abi, llm/wire
// OUTPUT: Cross-process approval-stage recovery and unknown-side-effect no-replay probes
// POS: [architecture / executable probes] Persists an outer workflow through existing extension records while keeping actual model/tool execution in the standard loop.

//go:build !js && !wasip1

package extensibility_test

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	native "github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

const workflowOwner = "probe-owner"
const workflowSession = "approval-session"
const approvalID = "approval-1"

var errUnknownEffect = errors.New("effect outcome needs reconciliation; no automatic replay")
var errUnapprovedEffect = errors.New("change not approved for this session")
var errNoEffect = errors.New("no change was executed")
var errApprovalReset = errors.New("approval request cannot reset an approved execution")

type approvalPhase struct {
	RequestID string `json:"request_id"`
	Stage     string `json:"stage"`
	RunID     string `json:"run_id,omitempty"`
	CallID    string `json:"call_id,omitempty"`
}

func readPhase(ctx context.Context, session abi.SessionStore) (approvalPhase, error) {
	records, err := session.Records(ctx, 0)
	if err != nil {
		return approvalPhase{}, err
	}
	var phase approvalPhase
	for _, record := range records {
		if record.Kind != abi.RecordExtension || record.Namespace != "probe.approval" || record.Type != "phase" {
			continue
		}
		if record.SchemaVersion != 1 {
			return phase, errors.New("unsupported workflow version")
		}
		if err := json.Unmarshal(record.Payload, &phase); err != nil {
			return phase, err
		}
		if phase.RequestID != approvalID {
			return phase, errors.New("foreign approval identity")
		}
		switch phase.Stage {
		case "waiting", "approved":
		case "applying", "done":
			if phase.RunID == "" || phase.CallID == "" {
				return phase, errors.New("execution identity missing")
			}
		default:
			return phase, errors.New("unknown workflow stage")
		}
	}
	return phase, nil
}

func writePhase(ctx context.Context, session abi.ConversationLog, phase approvalPhase) error {
	payload, err := json.Marshal(phase)
	if err != nil {
		return err
	}
	_, err = session.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "probe.approval", Type: "phase", SchemaVersion: 1, Payload: payload}})
	return err
}

func finishApproval(ctx context.Context, session *native.Session) error {
	phase, err := readPhase(ctx, session)
	if err != nil {
		return err
	}
	if phase.Stage != "applying" {
		return errNoEffect
	}
	records, err := session.Records(ctx, 0)
	if err != nil {
		return err
	}
	for _, record := range records {
		outcome := record.ToolOutcome
		if outcome != nil && outcome.RunID == phase.RunID && outcome.CallID == phase.CallID && outcome.Status == abi.OutcomeSucceeded && !outcome.Result.IsError {
			phase.Stage = "done"
			return writePhase(ctx, session, phase)
		}
	}
	return errUnknownEffect
}

// This is a business-stage coordinator, not an instruction-level continuation.
// BeginRun serializes ownership and settles any old unknown execution first.
func runApprovalWorkflow(ctx context.Context, repository *native.Repository, session *native.Session, directory, requestID string, approve, crash bool) error {
	_, release, err := repository.BeginRun(ctx, workflowOwner, workflowSession)
	if err != nil {
		return err
	}
	defer release()
	phase, err := readPhase(ctx, session)
	if err != nil {
		return err
	}
	if phase.Stage == "applying" {
		return errUnknownEffect
	}
	if approve && requestID != approvalID {
		return errors.New("approval does not match this request")
	}
	if phase.Stage == "done" {
		return nil
	}
	if approve && phase.Stage == "waiting" {
		if err := writePhase(ctx, session, approvalPhase{RequestID: approvalID, Stage: "approved"}); err != nil {
			return err
		}
		phase.Stage = "approved"
	}
	if phase.Stage == "waiting" {
		return nil
	}
	if approve && phase.Stage == "" {
		return errors.New("no approval was requested")
	}
	toolName, finalText := "request_approval", "waiting for approval"
	if phase.Stage == "approved" {
		toolName, finalText = "apply_change", "change completed"
	}
	turn := 0
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if turn == 1 {
			return requestTool(emit, toolName, toolName+"-call")
		}
		return finishText(emit, finalText)
	})
	agent, err := core.NewBuilder(model, session).Plugin(approvalPlugin(session, directory, crash)).Build(ctx)
	if err != nil {
		return err
	}
	if err := agent.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, toolName)}, discard); err != nil {
		return err
	}
	if phase.Stage == "approved" {
		return finishApproval(ctx, session)
	}
	return nil
}

func approvalPlugin(session *native.Session, directory string, crash bool) plugin {
	return plugin{name: "approval-workflow", register: func(_ context.Context, r *core.Registrar) error {
		if err := r.Tool(tool{name: "request_approval", execute: func(ctx context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
			phase, err := readPhase(ctx, session)
			if err != nil {
				return wire.ToolResult{}, err
			}
			if phase.Stage != "" && phase.Stage != "waiting" {
				return wire.ToolResult{}, errApprovalReset
			}
			if phase.Stage == "" {
				if err := writePhase(ctx, session, approvalPhase{RequestID: approvalID, Stage: "waiting"}); err != nil {
					return wire.ToolResult{}, err
				}
			}
			return toolResult(call, "request "+approvalID+" is durably waiting"), nil
		}}); err != nil {
			return err
		}
		return r.Tool(tool{name: "apply_change", execute: func(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
			current, err := readPhase(ctx, session)
			if err != nil {
				return wire.ToolResult{}, err
			}
			if current.Stage != "approved" || execution.SessionID != session.ID() {
				return wire.ToolResult{}, errUnapprovedEffect
			}
			if err := writePhase(ctx, session, approvalPhase{RequestID: approvalID, Stage: "applying", RunID: execution.RunID, CallID: execution.CallID}); err != nil {
				return wire.ToolResult{}, err
			}
			if err := ctx.Err(); err != nil {
				return wire.ToolResult{}, err
			}
			file, err := os.OpenFile(filepath.Join(directory, "effect-count"), os.O_WRONLY|os.O_CREATE|os.O_APPEND, 0600)
			if err != nil {
				return wire.ToolResult{}, err
			}
			_, writeErr := file.WriteString("x")
			flushErr := file.Sync()
			closeErr := file.Close()
			if err := errors.Join(writeErr, flushErr, closeErr); err != nil {
				return wire.ToolResult{}, err
			}
			if crash {
				os.Exit(37)
			} // Intent + applying + actual effect, no outcome.
			return toolResult(call, "change applied"), nil
		}})
	}}
}

// The parent invokes only this exact test in its own binary with an explicit
// private directory. No repository, account, workspace or provider is discovered.
func TestDurableProbeProcess(t *testing.T) {
	mode := os.Getenv("ALVA_EXTENSIBILITY_CHILD")
	if mode == "" {
		t.Skip("subprocess fixture only")
	}
	directory := os.Getenv("ALVA_EXTENSIBILITY_DIRECTORY")
	if directory == "" {
		t.Fatal("private fixture directory missing")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	repository, err := native.Open(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := repository.Close(); err != nil {
			t.Error(err)
		}
	}()
	var session *native.Session
	if mode == "prepare" {
		session, err = repository.Create(ctx, workflowOwner, workflowSession, "")
	} else {
		session, err = repository.OpenSession(ctx, workflowOwner, workflowSession)
	}
	if err != nil {
		t.Fatal(err)
	}
	err = runApprovalWorkflow(ctx, repository, session, directory, os.Getenv("ALVA_EXTENSIBILITY_REQUEST"), mode != "prepare", mode == "crash")
	if mode == "recover" && errors.Is(err, errUnknownEffect) {
		return
	}
	if mode == "wrong-request" && err != nil && err.Error() == "approval does not match this request" {
		return
	}
	if err != nil {
		t.Fatal(err)
	}
}

func runWorkflowProcess(t *testing.T, directory, mode, request string, expectedExit int) {
	t.Helper()
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, executable, "-test.run=^TestDurableProbeProcess$", "-test.count=1", "-test.timeout=15s")
	command.Env = []string{"ALVA_EXTENSIBILITY_CHILD=" + mode, "ALVA_EXTENSIBILITY_DIRECTORY=" + directory, "ALVA_EXTENSIBILITY_REQUEST=" + request}
	if root := os.Getenv("SYSTEMROOT"); root != "" {
		command.Env = append(command.Env, "SYSTEMROOT="+root)
	}
	output, err := command.CombinedOutput()
	exit := 0
	if err != nil {
		var failure *exec.ExitError
		if !errors.As(err, &failure) {
			t.Fatal(err)
		}
		exit = failure.ExitCode()
	}
	if exit != expectedExit {
		t.Fatalf("workflow %s: exit %d, expected %d\n%s", mode, exit, expectedExit, output)
	}
}

func inspectWorkflow(t *testing.T, directory string) (approvalPhase, []abi.SessionRecord, string) {
	t.Helper()
	repository, err := native.Open(context.Background(), directory)
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := repository.Close(); err != nil {
			t.Error(err)
		}
	}()
	session, err := repository.OpenSession(context.Background(), workflowOwner, workflowSession)
	if err != nil {
		t.Fatal(err)
	}
	phase, err := readPhase(context.Background(), session)
	if err != nil {
		t.Fatal(err)
	}
	effects, err := os.ReadFile(filepath.Join(directory, "effect-count"))
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		t.Fatal(err)
	}
	return phase, recordList(t, session), string(effects)
}

func privateDirectory(t *testing.T) string {
	t.Helper()
	directory := t.TempDir()
	if err := os.Chmod(directory, 0700); err != nil {
		t.Fatal(err)
	}
	return directory
}

func TestBusinessStageResumesAcrossProcessesWithoutCoreChanges(t *testing.T) {
	directory := privateDirectory(t)
	runWorkflowProcess(t, directory, "prepare", "", 0)
	phase, records, effects := inspectWorkflow(t, directory)
	if phase.Stage != "waiting" || effects != "" {
		t.Fatal(phase, effects)
	}
	for _, record := range records {
		if record.ToolIntent != nil && record.ToolIntent.Call.Name == "apply_change" {
			t.Fatal("effect was attempted before approval")
		}
	}
	runWorkflowProcess(t, directory, "wrong-request", "foreign-approval", 0)
	phase, _, effects = inspectWorkflow(t, directory)
	if phase.Stage != "waiting" || effects != "" {
		t.Fatal("foreign approval changed workflow", phase, effects)
	}
	runWorkflowProcess(t, directory, "approve", approvalID, 0)
	phase, records, effects = inspectWorkflow(t, directory)
	if phase.Stage != "done" || effects != "x" {
		t.Fatal(phase, effects)
	}
	before := len(records)
	runWorkflowProcess(t, directory, "approve", approvalID, 0)
	_, records, effects = inspectWorkflow(t, directory)
	if effects != "x" || len(records) != before {
		t.Fatal("completed workflow was repeated", effects, len(records), before)
	}
}

func TestUnknownSideEffectRequiresReconciliationAfterRealProcessExit(t *testing.T) {
	directory := privateDirectory(t)
	runWorkflowProcess(t, directory, "prepare", "", 0)
	runWorkflowProcess(t, directory, "crash", approvalID, 37)
	phase, _, effects := inspectWorkflow(t, directory)
	if phase.Stage != "applying" || effects != "x" {
		t.Fatal(phase, effects)
	}
	for i := 0; i < 2; i++ {
		runWorkflowProcess(t, directory, "recover", approvalID, 0)
	}
	phase, records, effects := inspectWorkflow(t, directory)
	if phase.Stage != "applying" || effects != "x" {
		t.Fatal("unknown execution was replayed or marked successful", phase, effects)
	}
	unknown := 0
	for _, record := range records {
		if record.Namespace == "alva.native.recovery" && record.Type == "execution_interrupted" {
			unknown++
		}
		if record.ToolOutcome != nil && record.ToolOutcome.CallID == "apply_change-call" && (!record.ToolOutcome.Result.IsError || record.ToolOutcome.Status != abi.OutcomeCancelled) {
			t.Fatal("unknown outcome was invented", record.ToolOutcome)
		}
	}
	if unknown != 1 {
		t.Fatal("unknown evidence must survive repeated recovery once", unknown)
	}
}

func TestUnknownWorkflowVersionDoesNotExecute(t *testing.T) {
	directory := privateDirectory(t)
	repository, err := native.Open(context.Background(), directory)
	if err != nil {
		t.Fatal(err)
	}
	defer repository.Close()
	session, err := repository.Create(context.Background(), workflowOwner, workflowSession, "")
	if err != nil {
		t.Fatal(err)
	}
	_, err = session.AppendRecords(context.Background(), []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "probe.approval", Type: "phase", SchemaVersion: 2, Payload: json.RawMessage(fmt.Sprintf(`{"request_id":%q,"stage":"approved"}`, approvalID))}})
	if err != nil {
		t.Fatal(err)
	}
	if err := runApprovalWorkflow(context.Background(), repository, session, directory, approvalID, true, false); err == nil || err.Error() != "unsupported workflow version" {
		t.Fatal(err)
	}
	if len(recordList(t, session)) != 1 {
		t.Fatal("unknown workflow version was interpreted or changed")
	}
}

func TestWorkflowPluginHandlesAdversarialModelCalls(t *testing.T) {
	for _, test := range []struct {
		name, stage           string
		calls                 []string
		wantStage, wantEffect string
		wantErr               error
	}{
		{name: "unapproved-effect", stage: "waiting", calls: []string{"apply_change"}, wantStage: "waiting", wantErr: errUnapprovedEffect},
		{name: "duplicate-effects-in-one-batch", stage: "approved", calls: []string{"apply_change", "apply_change"}, wantStage: "applying", wantEffect: "x", wantErr: errUnapprovedEffect},
		{name: "reset-after-effect", stage: "approved", calls: []string{"apply_change", "request_approval"}, wantStage: "applying", wantEffect: "x", wantErr: errApprovalReset},
		{name: "successful-text-without-effect", stage: "approved", wantStage: "approved", wantErr: errNoEffect},
	} {
		t.Run(test.name, func(t *testing.T) {
			directory := privateDirectory(t)
			repository, err := native.Open(context.Background(), directory)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() {
				if err := repository.Close(); err != nil {
					t.Error(err)
				}
			})
			session, err := repository.Create(context.Background(), workflowOwner, workflowSession, "")
			if err != nil {
				t.Fatal(err)
			}
			_, release, err := repository.BeginRun(context.Background(), workflowOwner, workflowSession)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(release)
			if err := writePhase(context.Background(), session, approvalPhase{RequestID: approvalID, Stage: test.stage}); err != nil {
				t.Fatal(err)
			}
			model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				if len(test.calls) == 0 {
					return finishText(emit, "I claim the change is complete")
				}
				for i, name := range test.calls {
					if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: fmt.Sprintf("call-%d", i), Name: name, Arguments: json.RawMessage(`{}`)}}); err != nil {
						return err
					}
				}
				return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
			})
			agent := build(t, core.NewBuilder(model, session).Plugin(approvalPlugin(session, directory, false)))
			err = agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "adversarial request")}, discard)
			if err == nil {
				err = finishApproval(context.Background(), session)
			}
			if !errors.Is(err, test.wantErr) {
				t.Fatal("wrong execution decision", err)
			}
			phase, err := readPhase(context.Background(), session)
			if err != nil {
				t.Fatal(err)
			}
			effects, err := os.ReadFile(filepath.Join(directory, "effect-count"))
			if err != nil && !errors.Is(err, os.ErrNotExist) {
				t.Fatal(err)
			}
			if phase.Stage != test.wantStage || string(effects) != test.wantEffect {
				t.Fatal("effect or workflow state was invented", phase, string(effects))
			}
		})
	}
}
