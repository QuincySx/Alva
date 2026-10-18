// INPUT: context, encoding/json, errors, filepath, strings, sync, time, unicode/utf8, kernel/abi, llm/wire
// OUTPUT: Session, NewSession, LoadSession, Prompt, Cancel, NextUpdate and permission callback dispatch
// POS: [integration / external protocol] Explicit turn ownership, late approval rejection and peer-confirmed cancellation.
package acp

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type promptTurn struct {
	ctx        context.Context
	cancel     context.CancelFunc
	cancelSent bool
}
type Session struct {
	client   *Client
	id, cwd  string
	mu       sync.Mutex
	turn     *promptTurn
	loading  bool
	usable   bool
	updates  chan Update
	creation NewSessionResponse
}

func (s *Session) ID() string    { return s.id }
func (s *Session) Running() bool { s.mu.Lock(); defer s.mu.Unlock(); return s.turn != nil }
func validCwd(cwd string) bool {
	return filepath.IsAbs(cwd) && len(cwd) <= 8192 && utf8.ValidString(cwd) && !strings.ContainsRune(cwd, 0)
}
func (c *Client) NewSession(ctx context.Context, cwd string) (*Session, error) {
	if !validCwd(cwd) {
		return nil, errors.New("ACP absolute session cwd required")
	}
	var session *Session
	_, err := c.request(ctx, "session/new", NewSessionRequest{Cwd: cwd, MCPServers: []struct{}{}}, func(data json.RawMessage) error {
		var response NewSessionResponse
		if len(data) > 64<<10 || json.Unmarshal(data, &response) != nil || !identity(response.SessionID) {
			return errors.New("invalid ACP session/new response")
		}
		c.mu.Lock()
		defer c.mu.Unlock()
		if len(c.sessions) >= 64 || c.sessions[response.SessionID] != nil {
			return errors.New("ACP session duplicate or limit exceeded")
		}
		session = &Session{client: c, id: response.SessionID, cwd: cwd, updates: make(chan Update, 64), creation: response, usable: true}
		c.sessions[session.id] = session
		return nil
	}, nil)
	return session, err
}
func (c *Client) LoadSession(ctx context.Context, id, cwd string) (*Session, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if !identity(id) || !validCwd(cwd) || !c.Initialize().AgentCapabilities.LoadSession {
		return nil, errors.New("ACP load identity/cwd or capability missing")
	}
	s := &Session{client: c, id: id, cwd: cwd, updates: make(chan Update, 64), loading: true}
	c.mu.Lock()
	if len(c.sessions) >= 64 || c.sessions[id] != nil {
		c.mu.Unlock()
		return nil, errors.New("ACP session duplicate or limit exceeded")
	}
	c.sessions[id] = s
	c.mu.Unlock()
	_, err := c.request(ctx, "session/load", LoadSessionRequest{SessionID: id, Cwd: cwd, MCPServers: []struct{}{}}, objectResult, nil)
	s.mu.Lock()
	s.loading = false
	s.usable = err == nil
	s.mu.Unlock()
	return s, err
}
func (s *Session) Prompt(ctx context.Context, content []ContentBlock) (PromptResponse, error) {
	if err := ctx.Err(); err != nil {
		return PromptResponse{}, err
	}
	if len(content) == 0 || len(content) > 128 {
		return PromptResponse{}, errors.New("ACP prompt requires 1..128 blocks")
	}
	caps := s.client.Initialize().AgentCapabilities.PromptCapabilities
	for _, block := range content {
		if err := block.Validate(caps); err != nil {
			return PromptResponse{}, err
		}
	}
	encoded, err := json.Marshal(content)
	if err != nil || len(encoded) > s.client.config.MaxFrameBytes/2 {
		return PromptResponse{}, errors.New("ACP prompt byte budget exceeded")
	}
	var owned []ContentBlock
	_ = json.Unmarshal(encoded, &owned)
	s.mu.Lock()
	if s.turn != nil || s.loading || !s.usable {
		s.mu.Unlock()
		return PromptResponse{}, errors.New("ACP session active")
	}
	turnContext, cancel := context.WithCancel(s.client.ctx)
	turn := &promptTurn{ctx: turnContext, cancel: cancel}
	s.turn = turn
	s.mu.Unlock()
	defer func() {
		s.mu.Lock()
		if s.turn == turn {
			s.turn = nil
		}
		s.mu.Unlock()
		cancel()
	}()
	data, err := s.client.request(ctx, "session/prompt", PromptRequest{SessionID: s.id, Prompt: owned}, nil, s)
	var response PromptResponse
	if len(data) > 0 {
		if json.Unmarshal(data, &response) != nil || response.StopReason == "" {
			failure := errors.New("invalid ACP prompt completion")
			s.client.fail(failure)
			return response, errors.Join(err, failure, s.client.cleanup())
		}
		switch response.StopReason {
		case "end_turn", "max_tokens", "max_turn_requests", "refusal", "cancelled":
		default:
			failure := errors.New("unknown ACP stop reason retained")
			s.client.fail(failure)
			return response, errors.Join(err, failure, s.client.cleanup())
		}
	}
	return response, err
}
func (s *Session) Cancel(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	bounded, cancel := context.WithTimeout(ctx, s.client.config.CancelTimeout)
	defer cancel()
	ctx = bounded
	s.mu.Lock()
	turn := s.turn
	s.mu.Unlock()
	return s.cancelTurn(ctx, turn)
}
func (s *Session) cancelTurn(ctx context.Context, turn *promptTurn) error {
	s.mu.Lock()
	if turn == nil || s.turn != turn || turn.cancelSent {
		s.mu.Unlock()
		return nil
	}
	turn.cancelSent = true
	turn.cancel()
	s.mu.Unlock()
	c := s.client
	if err := c.send(ctx, struct {
		JSONRPC string             `json:"jsonrpc"`
		Method  string             `json:"method"`
		Params  CancelNotification `json:"params"`
	}{"2.0", "session/cancel", CancelNotification{SessionID: s.id}}); err != nil {
		return err
	}
	c.mu.Lock()
	var pending []*callbackRequest
	for _, request := range c.callbackIDs {
		if request.session == s && request.turn == turn {
			pending = append(pending, request)
		}
	}
	c.mu.Unlock()
	var err error
	for _, request := range pending {
		err = errors.Join(err, c.answerPermission(ctx, request, PermissionOutcome{Outcome: "cancelled"}))
	}
	return err
}
func (s *Session) NextUpdate(ctx context.Context) (Update, error) {
	read := func(update Update) (Update, error) { s.client.release(update.bytes); return update, nil }
	select {
	case update := <-s.updates:
		return read(update)
	default:
	}
	select {
	case update := <-s.updates:
		return read(update)
	case <-ctx.Done():
		return Update{}, ctx.Err()
	case <-s.client.done:
		select {
		case update := <-s.updates:
			return read(update)
		default:
			return Update{}, s.client.failureError()
		}
	}
}
func (c *Client) deliverUpdate(params json.RawMessage) error {
	var notice struct {
		SessionID string          `json:"sessionId"`
		Update    json.RawMessage `json:"update"`
	}
	if json.Unmarshal(params, &notice) != nil || !identity(notice.SessionID) || !wire.ValidJSON(notice.Update) {
		return errors.New("invalid ACP session/update")
	}
	var kind struct {
		Kind string `json:"sessionUpdate"`
	}
	if json.Unmarshal(notice.Update, &kind) != nil || !identity(kind.Kind) {
		return errors.New("ACP update discriminator required")
	}
	c.mu.Lock()
	session := c.sessions[notice.SessionID]
	c.mu.Unlock()
	if session == nil {
		return errors.New("ACP update unknown session")
	}
	update := Update{SessionID: notice.SessionID, Kind: kind.Kind, Raw: append(json.RawMessage(nil), notice.Update...), bytes: len(params)}
	switch kind.Kind {
	case "agent_message_chunk", "user_message_chunk", "agent_thought_chunk":
		var message struct {
			Content   *ContentBlock `json:"content"`
			MessageID *string       `json:"messageId"`
		}
		if json.Unmarshal(notice.Update, &message) != nil || message.Content == nil {
			return errors.New("ACP content update required")
		}
		if message.MessageID != nil && !identity(*message.MessageID) {
			return errors.New("invalid ACP message identity")
		}
		update.MessageID = message.MessageID
		switch message.Content.Type {
		case "text", "image", "audio", "resource_link", "resource":
			if err := message.Content.Validate(PromptCapabilities{Image: true, Audio: true, EmbeddedContext: true}); err != nil {
				return err
			}
			update.Content = message.Content
		default:
			// Retain future content unchanged in Raw, never treat it as text
			// or as a tool completion/authorization signal.
		}
	case "tool_call", "tool_call_update":
		var tool ToolCallUpdate
		if json.Unmarshal(notice.Update, &tool) != nil || !identity(tool.ToolCallID) {
			return errors.New("ACP tool update identity required")
		}
		if kind.Kind == "tool_call" && tool.Title == nil {
			return errors.New("initial ACP tool call title required")
		}
		update.Tool = &tool
	case "notice", "compaction_update", "compaction_summary_chunk":
		return errors.New("ACP unadvertised client extension used")
	}
	if !c.reserve(update.bytes) {
		return errors.New("ACP update byte budget exceeded")
	}
	select {
	case session.updates <- update:
		return nil
	default:
		c.release(update.bytes)
		return errors.New("ACP update queue exceeded; no silent drops")
	}
}
func (c *Client) queuePermission(id json.RawMessage, key string, params json.RawMessage) error {
	var request PermissionRequest
	if json.Unmarshal(params, &request) != nil || !identity(request.SessionID) || !identity(request.ToolCall.ToolCallID) || len(request.Options) < 1 || len(request.Options) > 32 {
		return errors.New("invalid ACP permission request")
	}
	ids := map[string]bool{}
	for _, option := range request.Options {
		if !identity(option.OptionID) || option.Name == "" || option.Kind == "" || ids[option.OptionID] {
			return errors.New("invalid ACP permission options")
		}
		ids[option.OptionID] = true
	}
	c.mu.Lock()
	session := c.sessions[request.SessionID]
	c.mu.Unlock()
	if session == nil {
		return errors.New("ACP permission unknown session")
	}
	session.mu.Lock()
	turn := session.turn
	session.mu.Unlock()
	if turn == nil {
		return errors.New("ACP permission outside prompt")
	}
	if !c.reserve(len(params)) {
		return errors.New("ACP callback byte budget exceeded")
	}
	callback := &callbackRequest{id: append(json.RawMessage(nil), id...), key: key, params: append(json.RawMessage(nil), params...), session: session, turn: turn, request: request}
	c.mu.Lock()
	existing := c.callbackIDs[key]
	if (existing != nil && !existing.committed.Load()) || (existing == nil && len(c.callbackIDs) >= 32) {
		c.mu.Unlock()
		c.release(len(params))
		return errors.New("ACP callback duplicate or active limit exceeded")
	}
	c.callbackIDs[key] = callback
	c.mu.Unlock()
	select {
	case c.callbacks <- callback:
		return nil
	default:
		c.release(len(params))
		return errors.New("ACP callback queue exceeded")
	}
}
func (c *Client) answerPermission(ctx context.Context, request *callbackRequest, outcome PermissionOutcome) error {
	request.mu.Lock()
	defer request.mu.Unlock()
	if request.responded {
		return nil
	}
	request.session.mu.Lock()
	valid := request.session.turn == request.turn && request.turn.ctx.Err() == nil
	request.session.mu.Unlock()
	if !valid {
		outcome = PermissionOutcome{Outcome: "cancelled"}
	}
	if outcome.Outcome != "cancelled" && outcome.Outcome != "selected" {
		return errors.New("invalid ACP permission outcome")
	}
	if outcome.Outcome == "selected" {
		found := false
		for _, option := range request.request.Options {
			found = found || option.OptionID == outcome.OptionID
		}
		if !found {
			return errors.New("ACP permission option not offered")
		}
	} else {
		outcome.OptionID = ""
	}
	message := struct {
		JSONRPC string             `json:"jsonrpc"`
		ID      json.RawMessage    `json:"id"`
		Result  PermissionResponse `json:"result"`
	}{"2.0", request.id, PermissionResponse{Outcome: outcome}}
	err := c.send(ctx, message, func() (any, error) {
		request.session.mu.Lock()
		valid := request.session.turn == request.turn && request.turn.ctx.Err() == nil
		request.session.mu.Unlock()
		actual := message
		if !valid {
			actual.Result.Outcome = PermissionOutcome{Outcome: "cancelled"}
		}
		request.committed.Store(true)
		return actual, nil
	})
	if err == nil {
		request.responded = true
	}
	return err
}
func (c *Client) callbackWorker() {
	defer c.wg.Done()
	for {
		select {
		case <-c.ctx.Done():
			return
		case request := <-c.callbacks:
			func() {
				defer func() {
					c.release(len(request.params))
					c.mu.Lock()
					if c.callbackIDs[request.key] == request {
						delete(c.callbackIDs, request.key)
					}
					c.mu.Unlock()
				}()
				outcome := PermissionOutcome{Outcome: "cancelled"}
				var err error
				if request.turn.ctx.Err() == nil && c.handler != nil {
					encoded, _ := json.Marshal(request.request)
					var owned PermissionRequest
					_ = json.Unmarshal(encoded, &owned)
					outcome, err = c.handler.Approve(request.turn.ctx, owned)
				}
				if request.turn.ctx.Err() != nil {
					outcome = PermissionOutcome{Outcome: "cancelled"}
					if abi.OnlyCancellation(err) {
						err = nil
					}
				}
				if err != nil {
					c.mu.Lock()
					c.handlerFailure = errors.Join(c.handlerFailure, err)
					c.mu.Unlock()
					c.fail(err)
					return
				}
				if err = c.answerPermission(c.ctx, request, outcome); err != nil {
					c.fail(err)
					return
				}
				if c.handler == nil {
					ctx, stop := context.WithTimeout(c.ctx, c.config.CancelTimeout)
					defer stop()
					if err := request.session.cancelTurn(ctx, request.turn); err != nil {
						c.fail(err)
					}
				}
			}()
		}
	}
}

// WaitIdle observes peer completion. A delivered cancellation notice alone is
// not proof that remote model/tools have stopped.
func (s *Session) WaitIdle(ctx context.Context) error {
	ticker := time.NewTicker(5 * time.Millisecond)
	defer ticker.Stop()
	for {
		if !s.Running() {
			return nil
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-s.client.done:
			return s.client.failureError()
		case <-ticker.C:
		}
	}
}
