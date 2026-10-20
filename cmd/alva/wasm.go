// INPUT: Explicit CLI flags, sandbox/host, native session authority and host-only LanguageModel
// OUTPUT: Real WASIp1 CLI job execution, bounded native reference/history proxy and durable IO/audit receipts
// POS: [L6 application] Product harness policy; no provider/key/approval mode or database is mounted into the guest.
package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"

	sandboxhost "github.com/QuincySx/Alva/platform/sandbox/host"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/redaction"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	sandboxabi "github.com/QuincySx/Alva/sdk/sandbox/abi"
)

type grantFlags []sandboxhost.Grant

func (g *grantFlags) String() string         { return "explicit HOST=GUEST grants" }
func (g *grantFlags) Set(value string) error { return g.add(value, sandboxhost.ReadWrite) }
func (g *grantFlags) add(value string, access sandboxhost.Access) error {
	host, guest, ok := strings.Cut(value, "=")
	if !ok || host == "" || !sandboxabi.GuestPath(guest) || path.Clean(guest) != guest || guest == "/" || len(*g) >= 16 {
		return errors.New("grant requires existing HOST=/absolute/guest (not /), at most 16")
	}
	absolute, err := filepath.Abs(host)
	if err != nil {
		return err
	}
	canonical, err := filepath.EvalSymlinks(absolute)
	if err != nil {
		return err
	}
	info, err := os.Stat(canonical)
	if err != nil || !info.IsDir() {
		return errors.New("grant host must be an existing directory")
	}
	*g = append(*g, sandboxhost.Grant{Host: canonical, Guest: guest, Access: access})
	return nil
}

type readOnlyGrants struct{ grants *grantFlags }

func (readOnlyGrants) String() string       { return "explicit readonly HOST=GUEST grants" }
func (g readOnlyGrants) Set(v string) error { return g.grants.add(v, sandboxhost.ReadOnly) }

type domainFlags []string

func (d *domainFlags) String() string { return "explicit host fetch domains" }
func (d *domainFlags) Set(v string) error {
	if v == "" || len(*d) >= 256 {
		return errors.New("fetch domain must be explicit, at most 256")
	}
	*d = append(*d, v)
	return nil
}

type sandboxOptions struct {
	kind, module                          string
	grants                                grantFlags
	domains                               domainFlags
	deadline                              time.Duration
	memoryMiB                             uint64
	fuel                                  uint64
	scripts                               bool
	dangerous, approvalStdin, approvalTTY bool
}

func (o *sandboxOptions) flags(f *flag.FlagSet) {
	f.StringVar(&o.kind, "sandbox", "", "explicit execution backend; wasm requires a compiled --wasm-module")
	f.StringVar(&o.module, "wasm-module", "", "explicit Go WASIp1 worker module; no download/discovery")
	f.Var(&o.grants, "grant", "explicit HOST=/guest directory grant; host forces read-only in ask/plan")
	f.Var(readOnlyGrants{&o.grants}, "grant-readonly", "explicit always-readonly HOST=/guest directory grant")
	f.Var(&o.domains, "fetch-domain", "explicit exact or *.domain host fetch allowlist entry; default none")
	f.DurationVar(&o.deadline, "wasm-timeout", 5*time.Minute, "actual total WASM job deadline, including cold compilation; <=1h")
	f.Uint64Var(&o.memoryMiB, "wasm-memory-mib", 256, "actual guest linear memory limit, 16..512 MiB")
	f.Uint64Var(&o.fuel, "wasm-fuel", 0, "explicit Core V2 instruction budget (1..1e12); 0 selects deadline/memory/IO limits only")
	f.BoolVar(&o.scripts, "wasm-scripts", false, "explicitly delegate fresh isolated QuickJS with this job's existing file/fetch grants")
	f.BoolVar(&o.dangerous, "dangerously-allow-host-shell", false, "allow requesting unsandboxed /bin/sh only with independent approval for EACH invocation")
	f.BoolVar(&o.approvalStdin, "approval-stdin", false, "explicit one-line per-request approval channel; incompatible with --api-key-stdin")
	f.BoolVar(&o.approvalTTY, "approval-tty", false, "explicit /dev/tty per-request approval channel; no key is printed")
}
func (o sandboxOptions) validate(workspace string, memory, keyStdin bool) error {
	if o.kind != "" && o.kind != "wasm" {
		return errors.New("unknown --sandbox; only wasm is implemented")
	}
	if o.fuel > sandboxhost.MaxFuel {
		return errors.New("WASM instruction fuel exceeds 1e12")
	}
	if o.approvalStdin && keyStdin {
		return errors.New("--approval-stdin conflicts with --api-key-stdin; choose explicit --approval-tty")
	}
	if o.approvalStdin && o.approvalTTY {
		return errors.New("choose one explicit approval channel")
	}
	if o.kind == "" {
		if o.module != "" || len(o.grants) > 0 || len(o.domains) > 0 || o.fuel != 0 || o.scripts || o.dangerous || o.approvalStdin || o.approvalTTY {
			return errors.New("WASM capabilities require explicit --sandbox wasm")
		}
		return nil
	}
	if o.module == "" || workspace == "" || len(o.grants) == 0 {
		return errors.New("--sandbox wasm requires --wasm-module, --workspace HOST and explicit --grant[-readonly] HOST=/guest")
	}
	if memory {
		return errors.New("--memory native workspace scanning is not delegated to WASM")
	}
	if o.deadline < time.Millisecond || o.deadline > time.Hour || o.memoryMiB < 16 || o.memoryMiB > 512 {
		return errors.New("WASM deadline or memory limit out of range")
	}
	if (o.approvalStdin || o.approvalTTY) && !o.dangerous {
		return errors.New("approval channel does not enable host execution; explicit --dangerously-allow-host-shell is also required")
	}
	if o.dangerous && !o.approvalStdin && !o.approvalTTY {
		return errors.New("host shell requires an explicit per-invocation approval channel")
	}
	return nil
}
func (o sandboxOptions) guestWorkspace(host string) (string, error) {
	absolute, err := filepath.Abs(host)
	if err != nil {
		return "", err
	}
	canonical, err := filepath.EvalSymlinks(absolute)
	if err != nil {
		return "", err
	}
	var best string
	length := -1
	for _, g := range o.grants {
		relative, err := filepath.Rel(g.Host, canonical)
		if err != nil || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
			continue
		}
		if len(g.Host) > length {
			best = path.Join(g.Guest, filepath.ToSlash(relative))
			length = len(g.Host)
		}
	}
	if length < 0 {
		return "", errors.New("workspace must lie within an explicit directory grant")
	}
	return best, nil
}

func runWASM(ctx context.Context, o sandboxOptions, workspace string, mode security.PermissionMode, model abi.LanguageModel, prompt string, attachments []string, repository storage.Repository, native storage.Session, input io.Reader, output, diagnostics io.Writer) (runErr error) {
	if mode == security.Plan && o.dangerous {
		return errors.New("plan mode does not delegate host shell execution")
	}
	guestCwd, err := o.guestWorkspace(workspace)
	if err != nil {
		return err
	}
	file, err := os.Open(o.module)
	if err != nil {
		return err
	}
	stat, statErr := file.Stat()
	if statErr != nil || !stat.Mode().IsRegular() {
		file.Close()
		return errors.New("worker module must be a regular file")
	}
	module, readErr := io.ReadAll(io.LimitReader(file, sandboxhost.MaxModuleBytes+1))
	closeErr := file.Close()
	if err := errors.Join(readErr, closeErr); err != nil {
		return err
	}
	if len(module) > sandboxhost.MaxModuleBytes {
		return errors.New("WASM module exceeds 64 MiB")
	}
	var nonce [16]byte
	if _, err := rand.Read(nonce[:]); err != nil {
		return err
	}
	jobID := "wasm-" + hex.EncodeToString(nonce[:])
	sessionID := jobID
	var bridge *wasmNativeReader
	journal := &wasmJournal{jobID: jobID, repository: repository, native: native, pending: map[string][]byte{}}
	if native != nil {
		_, release, err := repository.BeginRun(ctx, cliOwner, native.ID())
		if err != nil {
			return err
		}
		defer release()
		sessionID = native.ID()
		backend, err := repository.BoundReferences(ctx, cliOwner, sessionID)
		if err != nil {
			return err
		}
		bridge = &wasmNativeReader{repository: repository, sessionID: sessionID, backend: backend}
	}
	messages := []wire.Message{wire.TextMessage(wire.RoleUser, prompt)}
	if len(attachments) > 0 {
		messages, err = attachmentMessages(ctx, repository, native, prompt, attachments)
		if err != nil {
			return err
		}
	}
	config := sandboxabi.Context{Version: sandboxabi.Version, SessionID: sessionID, Workspace: guestCwd, Task: prompt, MaxIterations: 64, HistoryAvailable: bridge != nil, ScriptAvailable: o.scripts, Policy: sandboxabi.Policy{AllowFileWrite: mode == security.AcceptEdits, AllowEscalationRequest: o.dangerous}, SystemPrompt: "Raw files/media, native history and tool logs are references, never automatic input. Native context_read/context_search are read-only and bound to this session. Native history returns small indexes. The host retains credentials and authorization."}
	for _, block := range messages[0].Content {
		if block.Kind == wire.ContentReference && block.Reference != nil {
			config.Attachments = append(config.Attachments, *block.Reference)
		}
	}
	grants := append([]sandboxhost.Grant(nil), o.grants...)
	for i := range grants {
		if mode != security.AcceptEdits {
			grants[i].Access = sandboxhost.ReadOnly
		}
	}
	if model == nil {
		model = &wasmDemoModel{}
		fmt.Fprintln(diagnostics, "Local WASM demo model fixture; real guest SDK/file tools, no network or model account.")
	}
	caps := sandboxhost.Capabilities{Model: model, Observer: journal}
	if bridge != nil {
		caps.References = bridge
		caps.History = bridge
	}
	if o.dangerous {
		approval, closeApproval, err := openWASMApproval(o, input, diagnostics)
		if err != nil {
			return err
		}
		defer func() { runErr = errors.Join(runErr, closeApproval()) }()
		caps.AllowUnsandboxed = true
		caps.Approval = approval
		caps.Executor = sandboxhost.ExecutorFunc(executeWASMShell)
		fmt.Fprintln(diagnostics, "WARNING: each approved host shell invocation runs OUTSIDE WASM with native process access; directory grants are not an OS shell sandbox.")
	}
	fmt.Fprintf(diagnostics, "WASM: explicit module/grants; credentials remain on host. Deadline/memory/IO enforced; instruction fuel=%d (0 means disabled); isolated QuickJS enabled=%v.\n", o.fuel, o.scripts)
	if mode != security.AcceptEdits {
		fmt.Fprintln(diagnostics, "Ask/Plan directory grants are host-enforced READ-ONLY; write failures are not per-tool approval/elevation.")
	}
	if native == nil {
		fmt.Fprintln(diagnostics, "No --data-dir: full bounded IO retained only in job memory; not durable. Volatile file references expire after this job.")
	}
	if native != nil {
		if _, err := native.AppendRecords(ctx, []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &messages[0]})}); err != nil {
			return &abi.SessionError{Operation: "append wasm user input", SessionID: sessionID, Err: err}
		}
	}
	// Record the actual explicit policy (no host paths or credential-bearing
	// provider configuration), not an implication that a network call occurred.
	limits := sandboxhost.Limits{Fuel: o.fuel, WallClock: o.deadline, MemoryBytes: o.memoryMiB << 20, OutputBytes: 1 << 20, MaxModelCalls: 128, MaxFetchCalls: 256, MaxEscalations: 16, MaxExchanges: 8192, MaxIOBytes: 64 << 20, MaxOutputTokens: 16384}
	type guestGrant struct {
		Guest  string             `json:"guest"`
		Access sandboxhost.Access `json:"access"`
	}
	grantIndex := make([]guestGrant, len(grants))
	for i, g := range grants {
		grantIndex[i] = guestGrant{g.Guest, g.Access}
	}
	digest := sha256.Sum256(module)
	intent, err := journal.record(ctx, "job_intent", struct {
		JobID               string             `json:"job_id"`
		ModuleSHA256        string             `json:"module_sha256"`
		GuestCwd            string             `json:"guest_cwd"`
		Grants              []guestGrant       `json:"grants"`
		Policy              sandboxabi.Policy  `json:"guest_policy"`
		ScriptEnabled       bool               `json:"isolated_script_enabled"`
		AllowedFetchDomains []string           `json:"allowed_fetch_domains"`
		Limits              sandboxhost.Limits `json:"host_limits"`
	}{jobID, hex.EncodeToString(digest[:]), guestCwd, grantIndex, config.Policy, o.scripts, append([]string(nil), o.domains...), limits})
	if err != nil {
		return err
	}
	if intent != nil {
		journal.parent = intent
	}
	outcome, executionErr := sandboxhost.Run(ctx, sandboxhost.Request{JobID: jobID, Module: module, Grants: grants, AllowedDomains: append([]string(nil), o.domains...), Context: config, Limits: limits}, caps)
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	journalErr := journal.finish(cleanup)
	runErr = errors.Join(executionErr, journalErr)
	if runErr == nil && outcome.ExitCode != 0 {
		runErr = fmt.Errorf("WASM worker exited with code %d; inspect the job IO references", outcome.ExitCode)
	}
	if runErr == nil {
		if !utf8.Valid(outcome.Stdout) {
			runErr = errors.New("worker result is not valid UTF-8")
		} else {
			final := wire.TextMessage(wire.RoleAssistant, strings.TrimSuffix(string(outcome.Stdout), "\n"))
			if native != nil {
				_, err := native.AppendRecords(cleanup, []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &final})})
				if err != nil {
					runErr = &abi.SessionError{Operation: "append wasm final response", SessionID: sessionID, Err: err}
				}
			}
		}
	}
	status := "succeeded"
	if runErr != nil {
		status = "failed"
		if abi.OnlyCancellation(runErr) {
			status = "cancelled"
		}
	}
	_, settleErr := journal.record(cleanup, "job_outcome", struct {
		JobID, Status                                               string
		ExitCode                                                    uint32
		ModelCalls, ScriptCalls, FetchCalls, Escalations, Exchanges int
		FuelLimit, FuelRemaining                                    uint64
		FuelExhausted                                               bool
		IOBytes                                                     uint64
		ElapsedMS                                                   int64
		ExecutionFailed, JournalFailed                              bool
	}{JobID: jobID, Status: status, ExitCode: outcome.ExitCode, ModelCalls: outcome.ModelCalls, ScriptCalls: outcome.ScriptCalls, FetchCalls: outcome.FetchCalls, Escalations: outcome.Escalations, Exchanges: outcome.Exchanges, FuelLimit: outcome.FuelLimit, FuelRemaining: outcome.FuelRemaining, FuelExhausted: outcome.FuelExhausted, IOBytes: outcome.IOBytes, ElapsedMS: outcome.Elapsed.Milliseconds(), ExecutionFailed: executionErr != nil, JournalFailed: journalErr != nil})
	runErr = errors.Join(runErr, settleErr)
	if native != nil {
		if err := native.Flush(cleanup); err != nil {
			runErr = errors.Join(runErr, &abi.SessionError{Operation: "flush wasm job", SessionID: sessionID, Err: err})
		}
	}
	if runErr == nil {
		answer := string(outcome.Stdout)
		if !strings.HasSuffix(answer, "\n") {
			answer += "\n"
		}
		runErr = writeString(output, answer)
	}
	if runErr != nil && status == "succeeded" {
		status = "failed"
	}
	fmt.Fprintln(diagnostics, "WASM job:", jobID, "status:", status, "model calls:", outcome.ModelCalls, "(no automatic replay)")
	return runErr
}

// wasmNativeReader never accepts an owner or target session from the guest.
type wasmNativeReader struct {
	repository storage.Repository
	sessionID  string
	backend    agentcontext.ReferenceBackend
}

func (r *wasmNativeReader) Read(ctx context.Context, q sandboxabi.ReferenceRead) (sandboxabi.TextPage, error) {
	if err := q.Validate(); err != nil {
		return sandboxabi.TextPage{}, err
	}
	p, err := (agentcontext.ReferenceReader{Backend: r.backend}).Read(ctx, q.URI, q.Offset, q.Limit)
	return sandboxabi.TextPage{URI: p.URI, Text: p.Text, Offset: p.Offset, NextOffset: p.NextOffset, TotalBytes: p.TotalBytes, Truncated: p.Truncated}, err
}
func (r *wasmNativeReader) Search(ctx context.Context, q sandboxabi.ReferenceSearch) ([]sandboxabi.TextMatch, error) {
	if err := q.Validate(); err != nil {
		return nil, err
	}
	hits, err := (agentcontext.ReferenceReader{Backend: r.backend}).Search(ctx, q.URI, q.Query, q.Limit)
	if err != nil {
		return nil, err
	}
	out := make([]sandboxabi.TextMatch, len(hits))
	for i, h := range hits {
		out[i] = sandboxabi.TextMatch{URI: h.URI, Text: h.Text, Offset: h.Offset}
	}
	return out, nil
}
func (r *wasmNativeReader) Page(ctx context.Context, q sandboxabi.HistoryRequest) (sandboxabi.HistoryPage, error) {
	page := sandboxabi.HistoryPage{Version: sandboxabi.Version}
	if err := q.Validate(); err != nil {
		return page, err
	}
	entry := func(e storage.RecordEntry, snippet string) sandboxabi.HistoryEntry {
		return sandboxabi.HistoryEntry{Sequence: e.Sequence, Namespace: e.Namespace, Type: e.Type, Summary: e.Summary, Value: e.Value, Snippet: snippet}
	}
	if q.Query != "" {
		p, err := r.repository.Search(ctx, cliOwner, r.sessionID, q.Query, q.After, q.Limit, q.HighWater)
		if err != nil {
			return page, err
		}
		page.Next = p.Next
		page.HighWater = p.HighWater
		page.HasMore = p.HasMore
		for _, m := range p.Matches {
			page.Entries = append(page.Entries, entry(m.Entry, m.Snippet))
		}
	} else {
		p, err := r.repository.Page(ctx, cliOwner, r.sessionID, q.After, q.Limit, q.HighWater)
		if err != nil {
			return page, err
		}
		page.Next = p.Next
		page.HighWater = p.HighWater
		page.HasMore = p.HasMore
		for _, e := range p.Entries {
			page.Entries = append(page.Entries, entry(e, ""))
		}
	}
	return page, page.Validate(q)
}

// Complete request/response frames are sanitized as frames. Stdio is assembled
// into bounded LF/EOF frames before known-marker redaction, not per Read chunk.
type wasmJournal struct {
	jobID       string
	repository  storage.Repository
	native      storage.Session
	parent      *abi.RecordReference
	pending     map[string][]byte
	memory      [][]byte
	memoryBytes int
	sequence    uint64
}

func (j *wasmJournal) Observe(ctx context.Context, e sandboxhost.IOEvent) error {
	if strings.HasPrefix(e.Kind, "stdout") || strings.HasPrefix(e.Kind, "stderr") {
		kind := strings.Split(e.Kind, "_prefix_limit")[0]
		data := append(j.pending[kind], e.Data...)
		if len(data) > 1<<20 {
			return errors.New("WASM journal stdio frame limit exceeded")
		}
		for {
			index := bytes.IndexByte(data, '\n')
			if index < 0 {
				break
			}
			if err := j.frame(ctx, e.Kind, e.Operation, data[:index+1], "complete"); err != nil {
				return err
			}
			data = data[index+1:]
		}
		j.pending[kind] = append([]byte(nil), data...)
		if strings.HasSuffix(e.Kind, "_prefix_limit") && len(data) > 0 {
			delete(j.pending, kind)
			return j.frame(ctx, e.Kind, e.Operation, data, "truncated_prefix_limit")
		}
		return nil
	}
	return j.frame(ctx, e.Kind, e.Operation, e.Data, "complete")
}
func (j *wasmJournal) finish(ctx context.Context) error {
	var errs []error
	for _, kind := range []string{"stdout", "stderr"} {
		if data := j.pending[kind]; len(data) > 0 {
			errs = append(errs, j.frame(ctx, kind, 0, data, "eof_partial"))
			delete(j.pending, kind)
		}
	}
	return errors.Join(errs...)
}
func (j *wasmJournal) frame(ctx context.Context, kind string, op uint32, data []byte, stage string) error {
	j.sequence++
	frame, err := json.Marshal(struct {
		JobID, Kind, Stage string
		Operation          uint32
		Sequence           uint64
		Body               string `json:"base64"`
	}{j.jobID, kind, stage, op, j.sequence, base64.StdEncoding.EncodeToString(data)})
	if err != nil {
		return err
	}
	frame, err = redaction.RedactStreamFrame(frame)
	if err != nil {
		return err
	}
	if j.native == nil {
		if len(frame) > 64<<20-j.memoryBytes {
			return errors.New("volatile WASM journal budget exceeded")
		}
		j.memory = append(j.memory, frame)
		j.memoryBytes += len(frame)
		return nil
	}
	var refs []wire.Reference
	for len(frame) > 0 {
		size := min(1<<20, len(frame))
		for size > 0 && size < len(frame) && !utf8.RuneStart(frame[size]) {
			size--
		}
		info, err := j.repository.PutArtifact(ctx, cliOwner, j.native.ID(), storage.ArtifactInput{Kind: "wasm-io-frame", Name: fmt.Sprintf("%s-%d-%d", j.jobID, j.sequence, len(refs)), MediaType: "text/plain; charset=utf-8", Text: true, Processing: "Ordered UTF-8 segment of captured JSON IO frame (may not be valid standalone JSON); known credential fields/markers sanitized before segmentation, no arbitrary-secret detection; not automatically sent to model", Data: frame[:size]})
		if err != nil {
			return &abi.SessionError{Operation: "store wasm IO", SessionID: j.native.ID(), Err: err}
		}
		refs = append(refs, info.Reference())
		frame = frame[size:]
	}
	_, err = j.record(ctx, "io_frame", struct {
		JobID, Kind, Stage string
		Sequence           uint64
		Operation          uint32
		Parts              []wire.Reference
	}{j.jobID, kind, stage, j.sequence, op, refs})
	return err
}
func (j *wasmJournal) record(ctx context.Context, kind string, value any) (*abi.RecordReference, error) {
	if j.native == nil {
		return nil, nil
	}
	data, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	records, err := j.native.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.sandbox", Type: kind, SchemaVersion: 1, Parent: j.parent, Payload: data}})
	if err != nil {
		return nil, &abi.SessionError{Operation: "append wasm " + kind, SessionID: j.native.ID(), Err: err}
	}
	r := records[0]
	return &abi.RecordReference{SessionID: j.native.ID(), RecordID: r.ID, Sequence: r.Sequence}, nil
}

type wasmDemoModel struct{ round int }

func (m *wasmDemoModel) Stream(ctx context.Context, q abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	m.round++
	if m.round == 1 {
		call := wire.ToolCall{ID: "demo-list", Name: "list_directory", Arguments: json.RawMessage(`{"path":"."}`)}
		if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &call}); err != nil {
			return err
		}
	} else {
		if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "WASM guest SDK completed a real bounded directory-tool roundtrip."}); err != nil {
			return err
		}
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: "stop"})
}
