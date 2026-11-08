// INPUT: Codex app-server item, turn and server-request frames
// OUTPUT: Bounded engine events, exact native interaction correlation, responses and the relayed resolution (offered choices only) on resolved events
// POS: [integration / native external-engine adapter] Native facts remain distinct from product ownership and human permission policy
package process

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/redaction"
)

type codexPending struct {
	id       json.RawMessage
	kind     string
	question engine.UserInputRequest
	accept   bool
	decline  bool
}

type codexRun struct {
	rpc        *codexRPC
	cancel     context.CancelFunc
	done       chan struct{}
	cleanupErr error
	session    string
	journalURI string
	mu         sync.Mutex
	turnID     string
	pending    map[string]codexPending
	seen       map[string]string
	cleared    map[string]bool
	tools      map[string]string
	items      map[string]string
	// displays keeps each open tool's display for the permission request that
	// asks to run it; resolutions keeps the decision or answers this adapter
	// relayed until the native resolved notification reports them.
	displays    map[string]*engine.ToolDisplay
	resolutions map[string]*engine.InteractionResolution
	finished    bool
	outcome     engine.Outcome
	usage       engine.Usage
	nativeErr   bool
	emit        func(engine.Event) error
	nativeStream
}

func (r *codexRun) reference(id string) *engine.Reference {
	return &engine.Reference{SessionID: r.session, RecordID: id, URI: r.journalURI}
}

func (r *codexRun) event(event engine.Event) error {
	event.SessionID = r.session
	r.rpc.journal.mu.Lock()
	private := r.rpc.journal.privateOutput
	secrets := append([]string(nil), r.rpc.journal.secrets...)
	r.rpc.journal.mu.Unlock()
	if private {
		// Incremental frames can expose a secret as separately harmless pieces.
		// Keep full completed messages useful, but never publish post-secret
		// streaming fragments into the product journal or transcript builder.
		if event.Kind == engine.MessageDelta || event.Kind == engine.ReasoningDelta {
			r.clearStreams()
			return nil
		}
	}
	if !private && len(secrets) != 0 && (event.Kind == engine.MessageDelta || event.Kind == engine.ReasoningDelta) {
		clean, err := r.filterDelta(event, secrets)
		if err != nil {
			return err
		}
		if clean == "" {
			return nil
		}
		event.Text = clean
		return r.publish(event)
	}
	{
		// Only question strings enter the credential-field redactor. Typed
		// controls such as is_secret must retain their boolean protocol meaning.
		type questionText struct {
			ID       string                   `json:"id"`
			Header   string                   `json:"header"`
			Question string                   `json:"question"`
			Options  []engine.UserInputOption `json:"options,omitempty"`
		}
		payload := struct {
			Text      string          `json:"text"`
			Input     json.RawMessage `json:"input,omitempty"`
			Questions []questionText  `json:"questions,omitempty"`
		}{Text: event.Text, Input: event.Input}
		if event.Question != nil {
			for _, question := range event.Question.Questions {
				payload.Questions = append(payload.Questions, questionText{question.ID, question.Header, question.Question, question.Options})
			}
		}
		data, err := json.Marshal(payload)
		if err != nil {
			return errors.New("could not encode private native event")
		}
		data, err = redaction.RedactJSON(data, secrets...)
		if err != nil {
			return errors.New("could not redact private native event")
		}
		// Decode into fresh storage: question options and RawMessage buffers
		// still belong to the native pending request, not this projection.
		payload.Questions, payload.Input = nil, nil
		if err := json.Unmarshal(data, &payload); err != nil {
			return errors.New("could not decode private native event")
		}
		if event.Question != nil {
			questionRequest := &engine.UserInputRequest{Questions: make([]engine.UserInputQuestion, len(event.Question.Questions))}
			for i, question := range event.Question.Questions {
				clean := payload.Questions[i]
				if clean.ID != question.ID {
					return errors.New("private native question identity cannot be published")
				}
				for j, option := range question.Options {
					if clean.Options[j].Label != option.Label {
						return errors.New("private native question choice cannot be published")
					}
				}
				question.Header, question.Question, question.Options = clean.Header, clean.Question, clean.Options
				questionRequest.Questions[i] = question
			}
			event.Question = questionRequest
		}
		event.Text, event.Input = payload.Text, payload.Input
		event.Display = finishDisplay(event.Display, secrets)
		// Relayed answers hold only offered labels (relayedAnswers); each is
		// redacted on its own and a failure degrades to no resolution.
		event.Resolution = publishedResolution(event.Resolution, event.Name, secrets, true)
	}
	return r.publish(event)
}

func (r *codexRun) publish(event engine.Event) error {
	if event.Kind == engine.Message && len(event.Text) > codexMessagePrefix {
		// A final answer keeps a readable bounded prefix; the native journal
		// record behind the reference holds the complete text.
		event.Text = utf8Prefix(event.Text, codexMessagePrefix)
		event.Truncated = true
		event.Reference = r.reference(event.ID)
	}
	if len(event.Text) > 64<<10 || len(event.Input) > 16<<10 {
		event.Text, event.Input = "", nil
		event.Reference = r.reference(event.ID)
	}
	return r.emit(event)
}

const codexMessagePrefix = 60 << 10

// utf8Prefix cuts at most limit bytes without splitting a rune.
func utf8Prefix(text string, limit int) string {
	if len(text) <= limit {
		return text
	}
	end := limit
	for end > 0 && !utf8.RuneStart(text[end]) {
		end--
	}
	return text[:end]
}

func (a *Adapter) codexRun(session string) *codexRun {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.codexRuns[session]
}

func (a *Adapter) respondCodexPermission(ctx context.Context, session, id string, decision engine.PermissionDecision) (dispatchErr error) {
	if err := ctx.Err(); err != nil {
		return err
	}
	if a.config.Transport != "app-server" {
		return engine.ErrUnsupported
	}
	if len(decision.UpdatedInput) != 0 {
		return fmt.Errorf("%w: Codex approval input cannot be rewritten", engine.ErrUnsupported)
	}
	run := a.codexRun(session)
	if run == nil {
		return engine.ErrSessionNotFound
	}
	run.mu.Lock()
	pending, ok := run.pending[id]
	if !ok || pending.kind != "permission" {
		run.mu.Unlock()
		return engine.ErrSessionNotFound
	}
	if decision.Allow && !pending.accept || !decision.Allow && !pending.decline {
		run.mu.Unlock()
		return fmt.Errorf("%w: native approval does not offer this decision", engine.ErrUnsupported)
	}
	delete(run.pending, id)
	allowed := decision.Allow
	run.relayed(id, &engine.InteractionResolution{Allowed: &allowed})
	run.mu.Unlock()
	defer func() {
		if dispatchErr != nil {
			run.forget(id)
			run.rpc.fail(dispatchErr)
			run.rpc.proc.BeginClose()
		}
	}()
	choice := "decline"
	if decision.Allow {
		choice = "accept"
	}
	return run.rpc.send(ctx, map[string]any{"id": pending.id, "result": map[string]string{"decision": choice}}, false)
}

func (a *Adapter) RespondUserInput(ctx context.Context, session, id string, response engine.UserInputResponse) (dispatchErr error) {
	if a.config.Transport == "rpc" {
		return a.respondPiInput(ctx, session, id, response)
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if a.config.Transport != "app-server" {
		return engine.ErrUnsupported
	}
	run := a.codexRun(session)
	if run == nil {
		return engine.ErrSessionNotFound
	}
	run.mu.Lock()
	pending, ok := run.pending[id]
	if !ok || pending.kind != "user_input" {
		run.mu.Unlock()
		return engine.ErrSessionNotFound
	}
	if err := response.ValidateFor(pending.question); err != nil {
		run.mu.Unlock()
		return err
	}
	delete(run.pending, id)
	// Only offered choices are kept; free-text and secret answers are kept
	// as answered without their body, as the journal omits them.
	run.relayed(id, &engine.InteractionResolution{Answers: relayedAnswers(pending.question, response)})
	run.mu.Unlock()
	defer func() {
		if dispatchErr != nil {
			run.forget(id)
			run.rpc.fail(dispatchErr)
			run.rpc.proc.BeginClose()
		}
	}()
	// Register explicitly secret answers before writing them to the native CLI.
	// Echoes in later stdout/stderr frames must also be redacted, even when the
	// question ID is an arbitrary name rather than a known credential field.
	secretQuestions := make(map[string]bool)
	for _, question := range pending.question.Questions {
		secretQuestions[question.ID] = question.IsSecret
	}
	var privateAnswers []string
	for questionID, values := range response.Answers {
		if secretQuestions[questionID] {
			privateAnswers = append(privateAnswers, values...)
		}
	}
	if err := run.rpc.journal.registerPrivateAnswers(privateAnswers); err != nil {
		return err
	}
	answers := make(map[string]any)
	for key, values := range response.Answers {
		answers[key] = map[string]any{"answers": values}
	}
	// Answers may be secrets even when the sender did not mark them as such.
	return run.rpc.send(ctx, map[string]any{"id": pending.id, "result": map[string]any{"answers": answers}}, true)
}

// relayed records what this adapter sent for a request, before sending it, so
// the native resolved notification can report it even if it arrives first.
// Callers hold r.mu; the map is bounded by the pending-request bound.
func (r *codexRun) relayed(id string, resolution *engine.InteractionResolution) {
	if r.resolutions == nil {
		r.resolutions = make(map[string]*engine.InteractionResolution)
	}
	if len(r.resolutions) < 64 {
		r.resolutions[id] = resolution
	}
}

// forget drops a relayed record whose dispatch failed.
func (r *codexRun) forget(id string) {
	r.mu.Lock()
	delete(r.resolutions, id)
	r.mu.Unlock()
}

func (j *journal) registerPrivateAnswers(values []string) error {
	if len(values) == 0 {
		return nil
	}
	j.mu.Lock()
	defer j.mu.Unlock()
	known := make(map[string]bool, len(j.secrets))
	total := 0
	for _, value := range j.secrets {
		if !known[value] {
			known[value] = true
			total += len(value)
		}
	}
	var added []string
	for _, value := range values {
		if known[value] {
			continue
		}
		known[value] = true
		total += len(value)
		added = append(added, value)
		if len(known) > 256 || total > 1<<20 {
			return errors.New("private native answer redaction budget exceeded")
		}
	}
	j.secrets = append(j.secrets, added...)
	j.privateOutput = true
	return nil
}

func (r *codexRun) handle(ctx context.Context, frame codexFrame) error {
	var params struct {
		ThreadID  string          `json:"threadId"`
		TurnID    string          `json:"turnId"`
		ItemID    string          `json:"itemId"`
		Delta     string          `json:"delta"`
		RequestID json.RawMessage `json:"requestId"`
		Thread    struct {
			ID string `json:"id"`
		} `json:"thread"`
		Turn struct {
			ID     string          `json:"id"`
			Status string          `json:"status"`
			Error  json.RawMessage `json:"error"`
		} `json:"turn"`
		Item       json.RawMessage `json:"item"`
		WillRetry  bool            `json:"willRetry"`
		TokenUsage struct {
			Last engine.Usage `json:"last"`
		} `json:"tokenUsage"`
	}
	paramsJSON := frame.Params
	if len(paramsJSON) == 0 || string(paramsJSON) == "null" {
		paramsJSON = json.RawMessage(`{}`)
	}
	if json.Unmarshal(paramsJSON, &params) != nil {
		return errors.New("invalid Codex notification parameters")
	}
	if params.Thread.ID != "" && params.Thread.ID != r.session || params.ThreadID != "" && params.ThreadID != r.session {
		if len(frame.ID) != 0 {
			return r.rpc.reject(ctx, frame.ID, -32602, "Request does not belong to the bound thread")
		}
		// App-server can multiplex descendant threads. Their notifications remain
		// in the native journal; they cannot change this root's identity, tools
		// or terminal state, nor confer typed child authority on the product.
		return nil
	}
	r.mu.Lock()
	turnID := r.turnID
	r.mu.Unlock()
	if params.TurnID != "" && params.TurnID != turnID || params.Turn.ID != "" && params.Turn.ID != turnID {
		// Resume can publish the bound thread's previous-turn usage snapshot
		// before turn/start replies. Retain that native fact only in the journal;
		// it must neither fail this turn nor replace its measured usage.
		if len(frame.ID) == 0 && frame.Method == "thread/tokenUsage/updated" && params.ThreadID == r.session && params.TurnID != "" && params.TurnID != turnID && params.Turn.ID == "" {
			return nil
		}
		return errors.New("Codex frame changed the active turn identity")
	}
	if len(frame.ID) != 0 {
		if params.ThreadID != r.session || params.TurnID != turnID || r.finished {
			return r.rpc.reject(ctx, frame.ID, -32602, "Request does not belong to the active turn")
		}
		return r.serverRequest(ctx, frame)
	}
	switch frame.Method {
	case "serverRequest/resolved":
		id, err := codexRequestID(params.RequestID)
		if err != nil {
			return err
		}
		r.mu.Lock()
		kind := r.seen[id]
		already := r.cleared[id]
		resolution := r.resolutions[id]
		delete(r.pending, id)
		delete(r.resolutions, id)
		if kind != "" {
			r.cleared[id] = true
		}
		r.mu.Unlock()
		if kind != "" && !already {
			return r.event(engine.Event{Kind: engine.InteractionResolved, ID: id, Name: kind, Resolution: resolution})
		}
		return nil
	case "thread/tokenUsage/updated":
		var usage struct {
			TokenUsage struct {
				Last struct {
					Input  uint64  `json:"inputTokens"`
					Cached *uint64 `json:"cachedInputTokens"`
					Output uint64  `json:"outputTokens"`
				} `json:"last"`
			} `json:"tokenUsage"`
		}
		if json.Unmarshal(frame.Params, &usage) != nil {
			return errors.New("invalid Codex usage")
		}
		// Codex counts cached input inside inputTokens.
		r.usage = engine.Usage{InputTokens: usage.TokenUsage.Last.Input, OutputTokens: usage.TokenUsage.Last.Output, CachedInputTokens: usage.TokenUsage.Last.Cached}
		if r.usage.Validate() != nil {
			r.usage.CachedInputTokens = nil
		}
		return nil
	case "turn/started":
		if r.finished || params.Turn.Status != "inProgress" {
			return errors.New("invalid Codex turn start")
		}
		return nil
	case "turn/completed":
		if r.finished || params.ThreadID != r.session || params.Turn.ID != turnID {
			return errors.New("invalid or repeated Codex turn completion")
		}
		if err := r.flushStreams("", false); err != nil {
			return err
		}
		r.finished = true
		switch params.Turn.Status {
		case "completed":
			r.outcome = engine.Succeeded
		case "interrupted":
			r.outcome = engine.Interrupted
		case "failed":
			r.outcome = engine.Failed
			r.nativeErr = true
		default:
			return errors.New("unknown Codex turn completion status")
		}
		if len(r.tools) != 0 {
			return errors.New("Codex completed while tools remained open")
		}
		return nil
	case "error":
		if r.finished {
			return errors.New("Codex error arrived after completion")
		}
		if params.WillRetry {
			return r.event(engine.Event{Kind: engine.Error, Text: "Codex reported a recoverable error", Recoverable: true, Reference: r.reference("")})
		}
		r.nativeErr = true
		return nil
	case "item/agentMessage/delta", "item/reasoning/summaryTextDelta", "item/reasoning/textDelta", "item/plan/delta":
		if r.finished || !nativeID.MatchString(params.ItemID) || r.items[params.ItemID] == "" || r.items[params.ItemID] == "completed" {
			return errors.New("Codex delta does not identify an active item")
		}
		kind := engine.MessageDelta
		if strings.HasPrefix(frame.Method, "item/reasoning/") {
			kind = engine.ReasoningDelta
		}
		return r.event(engine.Event{Kind: kind, ID: params.ItemID, Text: params.Delta})
	case "item/started", "item/completed":
		if r.finished || params.ThreadID != r.session || params.TurnID != turnID {
			return errors.New("Codex item is outside the active turn")
		}
		return r.item(frame.Method == "item/completed", params.Item)
	default:
		// Unknown notifications are bounded and journaled; they cannot execute work
		// or synthesize a completion. Known execution output after completion fails.
		if r.finished && (strings.HasPrefix(frame.Method, "item/") || strings.HasPrefix(frame.Method, "turn/")) {
			return errors.New("Codex execution output arrived after completion")
		}
		return nil
	}
}

func (r *codexRun) item(completed bool, raw json.RawMessage) error {
	var item struct {
		ID               string          `json:"id"`
		Type             string          `json:"type"`
		Text             string          `json:"text"`
		Status           string          `json:"status"`
		AggregatedOutput string          `json:"aggregatedOutput"`
		ExitCode         *int            `json:"exitCode"`
		Success          *bool           `json:"success"`
		Error            json.RawMessage `json:"error"`
	}
	if json.Unmarshal(raw, &item) != nil || !nativeID.MatchString(item.ID) || item.Type == "" {
		return errors.New("invalid Codex item")
	}
	previous := r.items[item.ID]
	if !completed {
		if previous != "" {
			return errors.New("Codex repeated an item start")
		}
		r.items[item.ID] = item.Type
	} else {
		if previous != item.Type {
			return errors.New("Codex item completed without its matching start")
		}
		if err := r.flushStreams(item.ID, (item.Type == "agentMessage" || item.Type == "plan") && item.Text != ""); err != nil {
			return err
		}
		r.items[item.ID] = "completed"
	}
	if len(r.items) > 8192 {
		return errors.New("Codex turn exceeds 8192 items")
	}
	switch item.Type {
	case "agentMessage", "plan":
		if completed {
			return r.event(engine.Event{Kind: engine.Message, ID: item.ID, Text: item.Text})
		}
	case "commandExecution", "fileChange", "mcpToolCall", "dynamicToolCall", "collabToolCall", "webSearch", "imageView":
		if !completed {
			r.tools[item.ID] = item.Type
			display := codexItemDisplay(item.Type, raw)
			if r.displays == nil {
				r.displays = make(map[string]*engine.ToolDisplay)
			}
			r.displays[item.ID] = display
			effect := engine.EffectUnknown
			switch item.Type {
			case "commandExecution":
				effect = engine.EffectExecute
			case "fileChange":
				effect = engine.EffectWrite
			case "webSearch":
				effect = engine.EffectNetwork
			case "imageView":
				effect = engine.EffectRead
			}
			return r.event(engine.Event{Kind: engine.ToolStart, ID: item.ID, Name: item.Type, Input: raw, Effect: effect, Display: display})
		}
		delete(r.tools, item.ID)
		delete(r.displays, item.ID)
		failed := item.Status == "failed" || item.Status == "declined" || item.Status == "cancelled" || item.ExitCode != nil && *item.ExitCode != 0 || item.Success != nil && !*item.Success || len(item.Error) > 0 && string(item.Error) != "null"
		return r.event(engine.Event{Kind: engine.ToolEnd, ID: item.ID, Name: item.Type, Text: item.AggregatedOutput, Input: raw, IsError: failed, Reference: r.reference(item.ID), Display: codexItemDisplay(item.Type, raw)})
	}
	return nil
}

func (r *codexRun) serverRequest(ctx context.Context, frame codexFrame) error {
	id, err := codexRequestID(frame.ID)
	if err != nil {
		return err
	}
	pending := codexPending{id: append(json.RawMessage(nil), frame.ID...)}
	event := engine.Event{ID: id, Reference: r.reference(id)}
	switch frame.Method {
	case "item/commandExecution/requestApproval", "item/fileChange/requestApproval":
		var params struct {
			ItemID    string            `json:"itemId"`
			Reason    string            `json:"reason"`
			Command   string            `json:"command"`
			Cwd       string            `json:"cwd"`
			Available []json.RawMessage `json:"availableDecisions"`
			Network   json.RawMessage   `json:"networkApprovalContext"`
		}
		expectedTool := "commandExecution"
		if frame.Method == "item/fileChange/requestApproval" {
			expectedTool = "fileChange"
		}
		if json.Unmarshal(frame.Params, &params) != nil || r.tools[params.ItemID] != expectedTool {
			return errors.New("Codex approval does not identify an open tool")
		}
		pending.kind, pending.accept, pending.decline = "permission", true, true
		if params.Available != nil {
			pending.accept, pending.decline = false, false
			for _, raw := range params.Available {
				var choice string
				if json.Unmarshal(raw, &choice) == nil {
					pending.accept = pending.accept || choice == "accept"
					pending.decline = pending.decline || choice == "decline"
				}
			}
		}
		event.Kind, event.Name, event.Text, event.Input = engine.PermissionRequest, r.tools[params.ItemID], params.Reason, frame.Params
		// The request asks to run the open item, so it shows that item.
		event.Display = engine.CloneToolDisplay(r.displays[params.ItemID])
		if event.Name == "commandExecution" {
			event.Effect = engine.EffectExecute
		} else {
			event.Effect = engine.EffectWrite
		}
		if len(params.Network) > 0 && string(params.Network) != "null" {
			event.Name, event.Effect = "networkAccess", engine.EffectNetwork
		}
	case "item/tool/requestUserInput":
		var params struct {
			Questions []struct {
				ID       string                   `json:"id"`
				Header   string                   `json:"header"`
				Question string                   `json:"question"`
				IsOther  bool                     `json:"isOther"`
				IsSecret bool                     `json:"isSecret"`
				Options  []engine.UserInputOption `json:"options"`
			} `json:"questions"`
		}
		if json.Unmarshal(frame.Params, &params) != nil {
			return errors.New("invalid Codex human question")
		}
		pending.kind = "user_input"
		for _, q := range params.Questions {
			pending.question.Questions = append(pending.question.Questions, engine.UserInputQuestion{ID: q.ID, Header: q.Header, Question: q.Question, IsOther: q.IsOther, IsSecret: q.IsSecret, Options: q.Options})
		}
		if err := pending.question.Validate(); err != nil {
			return err
		}
		event.Kind, event.Name, event.Question = engine.UserInputRequested, "requestUserInput", &pending.question
	default:
		return r.rpc.reject(ctx, frame.ID, -32601, "Unsupported server request")
	}
	r.mu.Lock()
	if r.seen[id] != "" || len(r.pending) >= 32 || len(r.seen) >= 8192 {
		r.mu.Unlock()
		return errors.New("Codex repeated or exceeded bounded server requests")
	}
	r.seen[id] = pending.kind
	r.pending[id] = pending
	r.mu.Unlock()
	return r.event(event)
}

func validateCodexTransport(config *Config) error {
	if config.Transport != "" && config.Transport != "exec" && config.Transport != "app-server" && config.Transport != "rpc" {
		return errors.New("unknown external engine transport")
	}
	if config.Transport == "rpc" && config.Vendor != "pi" {
		return errors.New("RPC transport requires Pi")
	}
	if config.Transport != "app-server" {
		if config.CodexSandbox != "" || config.CodexApprovalPolicy != "" {
			return errors.New("Codex native policies require app-server transport")
		}
		return nil
	}
	if config.Vendor != "codex" {
		return errors.New("app-server transport requires Codex")
	}
	if config.CodexSandbox == "" {
		config.CodexSandbox = "readOnly"
	}
	if config.CodexApprovalPolicy == "" {
		config.CodexApprovalPolicy = "onRequest"
	}
	if config.CodexSandbox != "readOnly" && config.CodexSandbox != "workspaceWrite" {
		return errors.New("unsupported Codex sandbox policy")
	}
	if config.CodexApprovalPolicy != "onRequest" && config.CodexApprovalPolicy != "unlessTrusted" && config.CodexApprovalPolicy != "never" {
		return errors.New("unsupported Codex approval policy")
	}
	return nil
}

func codexThreadCwdMatches(got, want string) bool {
	if got == "" {
		return true
	}
	canonical, err := filepath.EvalSymlinks(got)
	return err == nil && canonical == want
}
