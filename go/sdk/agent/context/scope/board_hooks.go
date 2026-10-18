// INPUT: bytes, context, encoding/json, errors, fmt, io, sync, agent/abi, kernel/abi, llm/wire
// OUTPUT: BlackboardHooks, BlackboardHooksConfig, BlackboardCommunication
// POS: [L3 capability library] Opt-in context lifecycle bridge and spawn-time scoped board attachment, never a kernel policy.
package scope

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	agentabi "github.com/QuincySx/Alva/go/sdk/agent/abi"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"io"
	"sync"
)

type BlackboardHooksConfig struct {
	MaxMessagesInContext           int
	AutoPostStatus, AutoPostOutput bool
}
type boardAgentState struct{ latest string }
type BlackboardHooks struct {
	profile AgentProfile
	board   *Blackboard
	config  BlackboardHooksConfig
	mu      sync.Mutex
	agents  map[string]boardAgentState
}

func NewBlackboardHooks(profile AgentProfile, board *Blackboard, config BlackboardHooksConfig) (*BlackboardHooks, error) {
	if board == nil || profile.ID == "" || profile.Role == "" || config.MaxMessagesInContext < 0 || config.MaxMessagesInContext > 1000 {
		return nil, errors.New("invalid board hook configuration")
	}
	return &BlackboardHooks{profile: cloneProfile(profile), board: board, config: config, agents: make(map[string]boardAgentState)}, nil
}
func (*BlackboardHooks) Name() string { return "blackboard" }
func (hooks *BlackboardHooks) Bootstrap(ctx context.Context, handle abi.ContextHandle, agentID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := hooks.board.Register(hooks.profile); err != nil {
		return err
	}
	message := wire.TextMessage(wire.RoleSystem, hooks.profile.TeamPrompt(hooks.board.Profiles()))
	message.ID = "board/team/" + hooks.profile.ID
	if err := handle.InjectMessage(agentID, abi.AlwaysPresent, message); err != nil {
		return err
	}
	intro, err := NewBoardMessage(hooks.profile.ID, "Joined team: "+hooks.profile.Role)
	if err != nil {
		return err
	}
	intro.Kind = Introduction
	hooks.mu.Lock()
	if _, exists := hooks.agents[agentID]; !exists && len(hooks.agents) >= 256 {
		hooks.mu.Unlock()
		return errors.New("board hook agent capacity exceeded")
	}
	hooks.agents[agentID] = boardAgentState{}
	hooks.mu.Unlock()
	return hooks.board.Post(intro)
}
func (hooks *BlackboardHooks) OnMessage(ctx context.Context, _ abi.ContextHandle, agentID string, message wire.Message) ([]abi.Injection, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if message.Role == wire.RoleAssistant && message.Text() != "" {
		hooks.mu.Lock()
		state, exists := hooks.agents[agentID]
		if !exists {
			hooks.mu.Unlock()
			return nil, errors.New("board hooks were not bootstrapped for agent")
		}
		state.latest = message.Text()
		hooks.agents[agentID] = state
		hooks.mu.Unlock()
	}
	return nil, nil
}
func (hooks *BlackboardHooks) Assemble(ctx context.Context, _ abi.ContextHandle, _ string, entries []abi.ContextEntry, _ int) ([]abi.ContextEntry, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	text, total := hooks.board.RenderChatLogFor(hooks.profile.ID, hooks.config.MaxMessagesInContext)
	if text == "" {
		return entries, nil
	}
	message := wire.TextMessage(wire.RoleUser, "<team_chat>\n"+text+"</team_chat>")
	message.ID = fmt.Sprintf("board/chat/%s/%d", hooks.profile.ID, total)
	return append(entries, abi.ContextEntry{ID: message.ID, Message: message, Metadata: abi.ContextMetadata{Layer: abi.OnDemand, Priority: abi.Normal, Origin: "blackboard"}}), nil
}
func (hooks *BlackboardHooks) AfterTurn(ctx context.Context, _ abi.ContextHandle, agentID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	hooks.mu.Lock()
	state, exists := hooks.agents[agentID]
	if !exists {
		hooks.mu.Unlock()
		return errors.New("board hooks were not bootstrapped for agent")
	}
	stateCopy := state
	state.latest = ""
	hooks.agents[agentID] = state
	hooks.mu.Unlock()
	if hooks.config.AutoPostOutput && stateCopy.latest != "" {
		message, err := NewBoardMessage(hooks.profile.ID, stateCopy.latest)
		if err != nil {
			return err
		}
		message.Kind = Artifact
		message.ArtifactName = hooks.profile.ID + "-output"
		message.Mentions = append([]string(nil), hooks.profile.ProvidesTo...)
		if err := hooks.board.Post(message); err != nil {
			return err
		}
	}
	if hooks.config.AutoPostStatus {
		message, err := NewBoardMessage(hooks.profile.ID, "Turn completed")
		if err != nil {
			return err
		}
		message.Kind = Status
		message.Status = &TaskPhase{Kind: TaskInProgress}
		return hooks.board.Post(message)
	}
	return nil
}

type BlackboardCommunication struct {
	Registry *BoardRegistry
	Config   BlackboardHooksConfig
}

func (BlackboardCommunication) Kind() string { return "blackboard" }
func (BlackboardCommunication) Description() string {
	return "Shared board within the direct parent scope; attach with a board_id"
}
func (BlackboardCommunication) ConfigSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","properties":{"board_id":{"type":"string"}},"required":["board_id"],"additionalProperties":false}`)
}
func (communication BlackboardCommunication) Attach(ctx context.Context, runtime agentabi.SpawnCommContext, raw json.RawMessage) (agentabi.SpawnCommHandle, error) {
	if err := ctx.Err(); err != nil {
		return agentabi.SpawnCommHandle{}, err
	}
	if communication.Registry == nil || runtime.ParentScopeID == "" || runtime.ChildScopeID == "" || runtime.ChildSessionID == "" || runtime.ParentScopeID == runtime.ChildScopeID || runtime.Role == "" {
		return agentabi.SpawnCommHandle{}, errors.New("actual parent/child identity and board registry required")
	}
	var config struct {
		BoardID string `json:"board_id"`
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&config); err != nil {
		return agentabi.SpawnCommHandle{}, err
	}
	if decoder.Decode(new(any)) != io.EOF || config.BoardID == "" {
		return agentabi.SpawnCommHandle{}, errors.New("invalid board config")
	}
	board, err := communication.Registry.GetOrCreate(runtime.ParentScopeID, config.BoardID)
	if err != nil {
		return agentabi.SpawnCommHandle{}, err
	}
	profile := AgentProfile{ID: runtime.ChildScopeID, Role: runtime.Role}
	hooks, err := NewBlackboardHooks(profile, board, communication.Config)
	if err != nil {
		return agentabi.SpawnCommHandle{}, err
	}
	callback := agentabi.ChildCompleteFunc(func(ctx context.Context, result agentabi.SpawnResult) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		message, err := NewBoardMessage(profile.ID, result.Text)
		if err != nil {
			return err
		}
		message.Kind = Artifact
		message.ArtifactName = runtime.Role + "-output"
		if result.IsError {
			message.Content += "\nChild failed: " + result.Error
		}
		return board.Post(message)
	})
	return agentabi.SpawnCommHandle{Hooks: []abi.ContextHooks{hooks}, OnComplete: callback}, nil
}
