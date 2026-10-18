// INPUT: encoding/json, errors, fmt, math, strings, sync, time, unicode/utf8, kernel/bus
// OUTPUT: Blackboard, BoardReader, BoardMessage, AgentProfile, MessageKind, TaskPhase, BoardLimits, NewBlackboard
// POS: [L3 capability library] Bounded append-only collaboration board with typed notifications and immutable parent views.
package scope

import (
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"math"
	"strings"
	"sync"
	"time"
	"unicode/utf8"
)

type MessageKind string

const (
	Introduction MessageKind = "introduction"
	Chat         MessageKind = "chat"
	Artifact     MessageKind = "artifact"
	Question     MessageKind = "question"
	Answer       MessageKind = "answer"
	Status       MessageKind = "status"
)

type TaskPhaseKind string

const (
	TaskStarted    TaskPhaseKind = "started"
	TaskInProgress TaskPhaseKind = "in_progress"
	TaskBlocked    TaskPhaseKind = "blocked"
	TaskCompleted  TaskPhaseKind = "completed"
	TaskFailed     TaskPhaseKind = "failed"
)

type TaskPhase struct {
	Kind    TaskPhaseKind `json:"kind"`
	Percent *float64      `json:"percent,omitempty"`
	Reason  string        `json:"reason,omitempty"`
	Error   string        `json:"error,omitempty"`
}
type BoardMessage struct {
	ID           string            `json:"id"`
	Time         time.Time         `json:"time"`
	From         string            `json:"from"`
	Mentions     []string          `json:"mentions,omitempty"`
	Kind         MessageKind       `json:"kind"`
	Content      string            `json:"content"`
	ArtifactName string            `json:"artifact_name,omitempty"`
	QuestionID   string            `json:"question_id,omitempty"`
	Status       *TaskPhase        `json:"status,omitempty"`
	Attachments  []json.RawMessage `json:"attachments,omitempty"`
}

func NewBoardMessage(from, content string) (BoardMessage, error) {
	id, err := identity()
	if err != nil {
		return BoardMessage{}, err
	}
	return BoardMessage{ID: id, Time: time.Now().UTC(), From: from, Content: content, Kind: Chat}, nil
}
func (message BoardMessage) ChatLine() string {
	tag := ""
	switch message.Kind {
	case Introduction:
		tag = " [intro]"
	case Artifact:
		tag = " [artifact: " + message.ArtifactName + "]"
	case Question:
		tag = " [question]"
	case Answer:
		tag = " [answer]"
	case Status:
		if message.Status != nil {
			tag = " [" + string(message.Status.Kind) + "]"
		}
	}
	line := "[" + message.From + "]" + tag + ": " + message.Content
	for _, mention := range message.Mentions {
		line += " @" + mention
	}
	return line
}
func (message BoardMessage) Validate() error {
	if message.ID == "" || message.From == "" || message.Time.IsZero() || !utf8.ValidString(message.Content) {
		return errors.New("invalid board message identity/text")
	}
	switch message.Kind {
	case Introduction, Chat:
	case Artifact:
		if message.ArtifactName == "" {
			return errors.New("artifact name missing")
		}
	case Question, Answer:
		if message.QuestionID == "" {
			return errors.New("question identity missing")
		}
	case Status:
		if message.Status == nil {
			return errors.New("task status missing")
		}
		switch message.Status.Kind {
		case TaskStarted, TaskCompleted:
		case TaskInProgress:
			if message.Status.Percent != nil && (math.IsNaN(*message.Status.Percent) || math.IsInf(*message.Status.Percent, 0) || *message.Status.Percent < 0 || *message.Status.Percent > 1) {
				return errors.New("invalid progress fraction")
			}
		case TaskBlocked:
			if message.Status.Reason == "" {
				return errors.New("blocked reason missing")
			}
		case TaskFailed:
			if message.Status.Error == "" {
				return errors.New("failure detail missing")
			}
		default:
			return errors.New("invalid task phase")
		}
	default:
		return errors.New("invalid board message kind")
	}
	for _, attachment := range message.Attachments {
		if !json.Valid(attachment) {
			return errors.New("invalid board attachment")
		}
	}
	return nil
}
func cloneBoardMessage(message BoardMessage) BoardMessage {
	message.Mentions = append([]string(nil), message.Mentions...)
	message.Attachments = append([]json.RawMessage(nil), message.Attachments...)
	for i := range message.Attachments {
		message.Attachments[i] = append(json.RawMessage(nil), message.Attachments[i]...)
	}
	if message.Status != nil {
		phase := *message.Status
		if phase.Percent != nil {
			value := *phase.Percent
			phase.Percent = &value
		}
		message.Status = &phase
	}
	return message
}

type AgentProfile struct {
	ID, Role                            string
	Capabilities, DependsOn, ProvidesTo []string
}

func cloneProfile(profile AgentProfile) AgentProfile {
	profile.Capabilities = append([]string(nil), profile.Capabilities...)
	profile.DependsOn = append([]string(nil), profile.DependsOn...)
	profile.ProvidesTo = append([]string(nil), profile.ProvidesTo...)
	return profile
}
func (profile AgentProfile) TeamPrompt(all []AgentProfile) string {
	var builder strings.Builder
	fmt.Fprintf(&builder, "## Team\nYou are %s, responsible for: %s.\n", profile.ID, profile.Role)
	if len(profile.Capabilities) > 0 {
		fmt.Fprintf(&builder, "Capabilities: %s.\n", strings.Join(profile.Capabilities, ", "))
	}
	for _, peer := range all {
		if peer.ID == profile.ID {
			continue
		}
		relation := ""
		if contains(profile.DependsOn, peer.ID) {
			relation += "; you depend on their output"
		}
		if contains(profile.ProvidesTo, peer.ID) {
			relation += "; they depend on your output"
		}
		fmt.Fprintf(&builder, "- %s (%s)%s\n", peer.ID, peer.Role, relation)
	}
	fmt.Fprintf(&builder, "Respond to @%s mentions; post artifacts/status for dependents.\n", profile.ID)
	return builder.String()
}
func contains(values []string, target string) bool {
	for _, value := range values {
		if value == target {
			return true
		}
	}
	return false
}

type BoardLimits struct{ MaxProfiles, MaxMessages, MaxBytes, MaxMessageBytes, NotificationCapacity int }
type Blackboard struct {
	mu       sync.RWMutex
	profiles []AgentProfile
	messages []BoardMessage
	ids      map[string]bool
	bytes    int
	limits   BoardLimits
	topic    *bus.Topic[BoardMessage]
}

func NewBlackboard(limits BoardLimits) (*Blackboard, error) {
	if limits.MaxProfiles < 1 || limits.MaxMessages < 1 || limits.MaxBytes < 1 || limits.MaxMessageBytes < 1 || limits.MaxMessageBytes > limits.MaxBytes {
		return nil, errors.New("positive finite board budgets required")
	}
	topic, err := bus.NewTopic(limits.NotificationCapacity, cloneBoardMessage)
	if err != nil {
		return nil, err
	}
	return &Blackboard{limits: limits, topic: topic, ids: make(map[string]bool)}, nil
}
func (board *Blackboard) Register(profile AgentProfile) error {
	if profile.ID == "" || profile.Role == "" {
		return errors.New("invalid board profile")
	}
	encoded, _ := json.Marshal(profile)
	if len(encoded) > board.limits.MaxMessageBytes {
		return errors.New("profile byte limit exceeded")
	}
	board.mu.Lock()
	defer board.mu.Unlock()
	for i, current := range board.profiles {
		if current.ID == profile.ID {
			board.profiles[i] = cloneProfile(profile)
			return nil
		}
	}
	if len(board.profiles) >= board.limits.MaxProfiles {
		return errors.New("board profile capacity exceeded")
	}
	board.profiles = append(board.profiles, cloneProfile(profile))
	return nil
}
func (board *Blackboard) Profiles() []AgentProfile {
	board.mu.RLock()
	defer board.mu.RUnlock()
	out := make([]AgentProfile, len(board.profiles))
	for i, profile := range board.profiles {
		out[i] = cloneProfile(profile)
	}
	return out
}
func (board *Blackboard) Post(message BoardMessage) error {
	if err := message.Validate(); err != nil {
		return err
	}
	encoded, err := json.Marshal(message)
	if err != nil {
		return err
	}
	board.mu.Lock()
	defer board.mu.Unlock()
	if board.ids[message.ID] {
		return errors.New("duplicate board message")
	}
	if len(encoded) > board.limits.MaxMessageBytes || len(board.messages) >= board.limits.MaxMessages || len(encoded) > board.limits.MaxBytes-board.bytes {
		return errors.New("board capacity exceeded; message was not posted")
	}
	message = cloneBoardMessage(message)
	board.messages = append(board.messages, message)
	board.ids[message.ID] = true
	board.bytes += len(encoded)
	board.topic.Publish(message)
	return nil
}
func (board *Blackboard) MessageCount() int {
	board.mu.RLock()
	defer board.mu.RUnlock()
	return len(board.messages)
}
func (board *Blackboard) MessagesSince(index int) []BoardMessage {
	board.mu.RLock()
	defer board.mu.RUnlock()
	if index < 0 {
		index = 0
	}
	if index >= len(board.messages) {
		return nil
	}
	out := make([]BoardMessage, 0, len(board.messages)-index)
	for _, message := range board.messages[index:] {
		out = append(out, cloneBoardMessage(message))
	}
	return out
}
func (board *Blackboard) MessagesFor(agentID string) []BoardMessage {
	board.mu.RLock()
	defer board.mu.RUnlock()
	var dependencies []string
	for _, profile := range board.profiles {
		if profile.ID == agentID {
			dependencies = profile.DependsOn
			break
		}
	}
	var result []BoardMessage
	for _, message := range board.messages {
		if len(message.Mentions) == 0 || contains(message.Mentions, agentID) || contains(dependencies, message.From) || message.Kind == Introduction {
			result = append(result, cloneBoardMessage(message))
		}
	}
	return result
}
func (board *Blackboard) RenderChatLogFor(agentID string, maxMessages int) (string, int) {
	messages := board.MessagesFor(agentID)
	total := len(messages)
	if maxMessages < 0 {
		maxMessages = 0
	}
	start := total - maxMessages
	if start < 0 {
		start = 0
	}
	var builder strings.Builder
	if start > 0 {
		fmt.Fprintf(&builder, "... (%d earlier relevant messages omitted)\n", start)
	}
	for _, message := range messages[start:] {
		line := message.ChatLine() + "\n"
		if builder.Len()+len(line) > 16<<10 {
			remaining := (16 << 10) - builder.Len() - len(" … [truncated]\n")
			if remaining > 0 {
				for remaining > 0 && !utf8.RuneStart(line[remaining]) {
					remaining--
				}
				builder.WriteString(line[:remaining])
				builder.WriteString(" … [truncated]\n")
			}
			break
		}
		builder.WriteString(line)
	}
	return builder.String(), total
}
func (board *Blackboard) Subscribe() *bus.Subscription[BoardMessage] { return board.topic.Subscribe() }

type BoardReader interface {
	Profiles() []AgentProfile
	MessageCount() int
	MessagesSince(int) []BoardMessage
	MessagesFor(string) []BoardMessage
	RenderChatLogFor(string, int) (string, int)
	Subscribe() *bus.Subscription[BoardMessage]
}
type boardView struct{ board *Blackboard }

func (view boardView) Profiles() []AgentProfile               { return view.board.Profiles() }
func (view boardView) MessageCount() int                      { return view.board.MessageCount() }
func (view boardView) MessagesSince(index int) []BoardMessage { return view.board.MessagesSince(index) }
func (view boardView) MessagesFor(id string) []BoardMessage   { return view.board.MessagesFor(id) }
func (view boardView) RenderChatLogFor(id string, max int) (string, int) {
	return view.board.RenderChatLogFor(id, max)
}
func (view boardView) Subscribe() *bus.Subscription[BoardMessage] { return view.board.Subscribe() }
