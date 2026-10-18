// INPUT: context, crypto/sha256, encoding/hex/json, errors, fmt, strings, time, utf8, host/command, kernel/abi, storage/session.
// OUTPUT: Native diagnostic Manager with prior activity, explicit process execution and owned reference evidence.
// POS: [L4 extension] Command success is not misrepresented as parsed diagnostics or a silent empty no-server result.
package diagnostics

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/host/command"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const sourceNotice = "Untrusted output from an explicitly approved diagnostic command, not an attached LSP server. Known credentials redacted; structured JSON normalized; invalid UTF-8 replaced if present. No diagnostic parsing performed: actual exit/completeness is not proof of zero issues. Incomplete/failed execution is never retried automatically."

var ErrExecution = errors.New("configured diagnostic command failed")

type nativeManager struct{ plugin *Plugin }

func (m *nativeManager) Catalog() []ProfileInfo {
	var infos []ProfileInfo
	for _, p := range m.plugin.config.Profiles {
		infos = append(infos, ProfileInfo{ID: p.ID, Description: p.Description})
	}
	return infos
}
func (m *nativeManager) Run(ctx context.Context, request Request) (report Report, runErr error) {
	p := m.plugin
	e := request.Execution
	report = Report{SessionID: e.SessionID, RunID: e.RunID, CallID: e.CallID, ProfileID: request.ProfileID, Status: "not_started", SourceNotice: sourceNotice}
	if err := p.check(e); err != nil {
		return report, err
	}
	actual, ok := e.ActualTool.(*diagnosticTool)
	if !ok || actual != p.tool || request.Call.Name != p.tool.Name() || request.Call.ID == "" || request.Call.ID != e.CallID {
		return report, errors.New("diagnostics manager requires actual dispatched tool/call authority")
	}
	selected, decodeErr := decodeArguments(request.Call.Arguments)
	if decodeErr != nil || selected.Action != "run" || selected.ProfileID != request.ProfileID {
		return report, errors.Join(errors.New("diagnostics profile differs from actual authorized call"), decodeErr)
	}
	if err := p.config.Guard.Authorize(ctx, e, request.Call, p.tool.DescribeEffect(request.Call)); err != nil {
		return report, err
	}
	select {
	case p.gate <- struct{}{}:
	case <-ctx.Done():
		return report, ctx.Err()
	}
	defer func() { <-p.gate }()
	// An explicit Manager consumer may have waited behind another command
	// while its run ended. Admission must still belong to the actual active
	// run at the point at which this command can become executable.
	if err := p.check(e); err != nil {
		return report, err
	}
	p.mu.Lock()
	if p.executions >= 32 || p.bytes+(2<<20) > 32<<20 {
		p.mu.Unlock()
		return report, errors.New("diagnostic execution/transcript budget exceeded")
	}
	p.executions++
	p.mu.Unlock()
	var profile *Profile
	for index := range p.config.Profiles {
		if p.config.Profiles[index].ID == request.ProfileID {
			profile = &p.config.Profiles[index]
			break
		}
	}
	if profile == nil {
		return report, errors.New("no approved native diagnostics profile")
	}
	hash := sha256.Sum256([]byte(e.RunID + "\x00" + e.CallID))
	id := "diagnostic-" + hex.EncodeToString(hash[:16])
	if err := p.config.Report(ctx, Activity{ID: id, Name: "Project diagnostic command", SessionID: e.SessionID, RunID: e.RunID, CallID: e.CallID, Effect: abi.EffectUnknown}); err != nil {
		return report, err
	}
	intent, _ := json.Marshal(map[string]any{"invocation_id": id, "profile_id": profile.ID, "run_id": e.RunID, "call_id": e.CallID, "status": "start_attempted", "effect": "unknown", "source_notice": sourceNotice})
	if err := p.append(ctx, "diagnostic_intent", intent); err != nil {
		return report, err
	}
	deadline, cancel := context.WithTimeout(ctx, profile.Timeout)
	defer cancel()
	result, runErr := command.Run(deadline, profile.Process, nil, 1<<20)
	p.mu.Lock()
	p.bytes += len(result.Stdout) + len(result.Stderr)
	p.mu.Unlock()
	if runErr != nil && ctx.Err() == nil && abi.OnlyCancellation(runErr) {
		runErr = errors.Join(ErrExecution, runErr)
	}
	report.Started = result.Started
	report.Reaped = result.Reaped
	report.CapturedComplete = result.Complete
	report.ExitCode = result.ExitCode
	report.Status = "completed"
	if result.ExitCode != 0 {
		report.Status = "nonzero_exit"
	}
	if runErr != nil {
		report.Status = "failed"
		report.Error = runErr.Error()
		if len(report.Error) > 1024 {
			report.Error = "Diagnostic command failed; details exceed receipt limit. Inspect bounded native evidence."
		}
	}
	cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer end()
	var err error
	report.Stdout, err = p.retain(cleanup, profile.ID, "stdout", result.Stdout, result.Complete)
	runErr = errors.Join(runErr, err)
	report.Stderr, err = p.retain(cleanup, profile.ID, "stderr", result.Stderr, result.Complete)
	runErr = errors.Join(runErr, err)
	if runErr != nil {
		report.Status = "failed"
	}
	data, _ := json.Marshal(report)
	runErr = errors.Join(runErr, p.append(cleanup, "diagnostic_outcome", data))
	return report, runErr
}
func (p *Plugin) append(ctx context.Context, kind string, payload json.RawMessage) error {
	_, err := p.log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.tooling", Type: kind, SchemaVersion: 1, Payload: payload}})
	if err != nil {
		return &abi.SessionError{Operation: "append " + kind, SessionID: p.config.SessionID, Err: err}
	}
	if err = p.log.Flush(ctx); err != nil {
		return &abi.SessionError{Operation: "flush " + kind, SessionID: p.config.SessionID, Err: err}
	}
	return nil
}
func (p *Plugin) retain(ctx context.Context, profileID, kind string, data []byte, complete bool) (wire.Reference, error) {
	sourceBytes := len(data)
	valid := utf8.Valid(data)
	omittedTail := 0
	if !complete {
		keep := bytes.LastIndexByte(data, '\n') + 1
		omittedTail = len(data) - keep
		data = data[:keep]
	}
	text := strings.ToValidUTF8(string(data), "�")
	known := []string{}
	for _, entry := range p.config.Profiles {
		if entry.ID == profileID {
			known = session.EnvironmentSecrets(entry.Process.Env)
			break
		}
	}

	var sanitized []byte
	var err error
	if json.Valid([]byte(text)) {
		sanitized, err = session.RedactJSON([]byte(text), known...)
	} else {
		encoded, _ := json.Marshal(text)
		sanitized, err = session.RedactJSON(encoded, known...)
		if err == nil {
			err = json.Unmarshal(sanitized, &text)
			sanitized = []byte(text)
		}
	}
	if err != nil {
		return wire.Reference{}, err
	}
	info, err := p.config.Repository.PutArtifact(ctx, p.config.Owner, p.config.SessionID, session.ArtifactInput{Kind: "diagnostic-" + kind, Name: profileID, MediaType: "text/plain; charset=utf-8", Processing: fmt.Sprintf("Known-credential-redacted diagnostic %s; JSON normalized; complete_capture=%v; original_utf8=%v; captured_source_bytes=%d; incomplete_tail_omitted_bytes=%d; invalid UTF-8 replaced; no parsing or model delivery", kind, complete, valid, sourceBytes, omittedTail), Data: sanitized, Text: true})
	if err != nil {
		return wire.Reference{}, &abi.SessionError{Operation: "retain diagnostic " + kind, SessionID: p.config.SessionID, Err: err}
	}
	return info.Reference(), nil
}
