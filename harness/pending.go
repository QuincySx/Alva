// INPUT: context, crypto/rand, encoding/hex, sync, time, agent/core, kernel/abi/bus, llm/wire.
// OUTPUT: PendingPlugin, PendingService, PendingMessage and bounded steering queue APIs.
// POS: [L5 harness] Optional harness interjections commit atomically at the next model boundary, never inside the kernel loop.
package harness

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

var ErrPendingInactive = errors.New("pending messages require an active bound run")
var ErrPendingFull = errors.New("pending message count or byte budget exceeded")
var ErrPendingCommitting = errors.New("pending message is being committed; it cannot be retracted")

type PendingStatus string

const (
	PendingUnread     PendingStatus = "unread"
	PendingCommitting PendingStatus = "committing"
	PendingCommitted  PendingStatus = "committed"
	PendingUnknown    PendingStatus = "commit_unknown"
)

type PendingMessage struct {
	ID          string        `json:"id"`
	Text        string        `json:"text"`
	RunID       string        `json:"run_id"`
	CreatedAt   time.Time     `json:"created_at"`
	CommittedAt *time.Time    `json:"committed_at,omitempty"`
	Status      PendingStatus `json:"status"`
	Notice      string        `json:"notice,omitempty"`
}
type PendingService struct {
	mu                   sync.Mutex
	sessionID, activeRun string
	messages             []PendingMessage
	bytes                int
}
type PendingPlugin struct{ service *PendingService }

func NewPendingPlugin(actualSessionID string) *PendingPlugin {
	return &PendingPlugin{service: &PendingService{sessionID: actualSessionID}}
}
func (*PendingPlugin) Name() string               { return "pending_messages" }
func (p *PendingPlugin) Service() *PendingService { return p.service }
func (p *PendingPlugin) Register(_ context.Context, r *core.Registrar) error {
	if p.service.sessionID == "" {
		return errors.New("pending plugin requires actual session binding")
	}
	if err := bus.Provide(r.BusWriter(), p.service); err != nil {
		return err
	}
	// Admit pending input before ordinary model-request projections.
	return r.Middleware(p, -500)
}
func (s *PendingService) Enqueue(ctx context.Context, text string) (PendingMessage, error) {
	if err := ctx.Err(); err != nil {
		return PendingMessage{}, err
	}
	if text == "" || len(text) > 16<<10 || !utf8.ValidString(text) {
		return PendingMessage{}, errors.New("pending input must contain 1..16384 UTF-8 bytes")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.activeRun == "" {
		return PendingMessage{}, ErrPendingInactive
	}
	if len(s.messages) >= 128 || len(text) > (1<<20)-s.bytes {
		return PendingMessage{}, ErrPendingFull
	}
	var key [16]byte
	if _, err := rand.Read(key[:]); err != nil {
		return PendingMessage{}, err
	}
	message := PendingMessage{ID: "pending:" + hex.EncodeToString(key[:]), Text: text, RunID: s.activeRun, CreatedAt: time.Now().UTC(), Status: PendingUnread}
	s.messages = append(s.messages, message)
	s.bytes += len(text)
	return message, nil
}
func (s *PendingService) Cancel(ctx context.Context, id string) (bool, error) {
	if err := ctx.Err(); err != nil {
		return false, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	for index, message := range s.messages {
		if message.ID != id {
			continue
		}
		if message.Status == PendingCommitting {
			return false, ErrPendingCommitting
		}
		if message.Status != PendingUnread {
			return false, nil
		}
		s.bytes -= len(message.Text)
		s.messages = append(s.messages[:index], s.messages[index+1:]...)
		return true, nil
	}
	return false, nil
}
func (s *PendingService) Snapshot(unreadOnly bool) []PendingMessage {
	s.mu.Lock()
	defer s.mu.Unlock()
	messages := make([]PendingMessage, 0, len(s.messages))
	for _, message := range s.messages {
		if unreadOnly && message.Status != PendingUnread {
			continue
		}
		if message.CommittedAt != nil {
			value := *message.CommittedAt
			message.CommittedAt = &value
		}
		messages = append(messages, message)
	}
	return messages
}
func (p *PendingPlugin) OnRunStart(ctx context.Context, runtime abi.RuntimeContext) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if runtime.State == nil || runtime.State.Session().ID() != p.service.sessionID || runtime.RunID == "" {
		return errors.New("pending actual session/run mismatch")
	}
	p.service.mu.Lock()
	defer p.service.mu.Unlock()
	if p.service.activeRun != "" {
		return errors.New("pending service already active")
	}
	p.service.activeRun = runtime.RunID
	return nil
}
func (p *PendingPlugin) OnRunEnd(_ context.Context, runtime abi.RuntimeContext, _ error) error {
	p.service.mu.Lock()
	defer p.service.mu.Unlock()
	if p.service.activeRun == runtime.RunID {
		p.service.activeRun = ""
	}
	return nil
}
func (p *PendingPlugin) BeforeModel(ctx context.Context, runtime abi.RuntimeContext, request *abi.ModelRequest) error {
	if runtime.State == nil || runtime.State.Session().ID() != p.service.sessionID {
		return errors.New("pending actual runtime session mismatch")
	}
	p.service.mu.Lock()
	if p.service.activeRun != runtime.RunID {
		p.service.mu.Unlock()
		return ErrPendingInactive
	}
	var batch []abi.SessionRecord
	selected := map[string]bool{}
	for index := range p.service.messages {
		message := &p.service.messages[index]
		if message.Status != PendingUnread {
			continue
		}
		message.Status = PendingCommitting
		selected[message.ID] = true
		input := wire.TextMessage(wire.RoleUser, message.Text)
		input.ID = message.ID
		batch = append(batch, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &input}))
	}
	p.service.mu.Unlock()
	if len(batch) == 0 {
		return nil
	}
	committed, err := runtime.State.Session().AppendRecords(ctx, batch)
	if err == nil {
		if len(committed) != len(batch) {
			err = errors.New("pending session violated atomic append receipt")
		} else {
			for index, record := range committed {
				if record.Kind != abi.RecordConversation || record.Message == nil || record.Message.ID != batch[index].Message.ID {
					err = errors.New("pending session returned mismatched message receipt")
					break
				}
			}
		}
	}
	p.service.mu.Lock()
	defer p.service.mu.Unlock()
	now := time.Now().UTC()
	for index := range p.service.messages {
		message := &p.service.messages[index]
		if !selected[message.ID] {
			continue
		}
		if err != nil {
			message.Status = PendingUnknown
			message.Notice = "Session commit failed or is uncertain; inspect history before explicitly resending. No automatic retry."
		} else {
			message.Status = PendingCommitted
			message.CommittedAt = &now
		}
	}
	if err != nil {
		return err
	}
	request.Messages = append(request.Messages, wire.TextMessage(wire.RoleSystem, "The user interjected while work was running. Following user messages were committed at this model boundary; this does not prove model acceptance."))
	for _, record := range committed {
		request.Messages = append(request.Messages, wire.CloneMessages([]wire.Message{*record.Message})[0])
	}
	return nil
}
