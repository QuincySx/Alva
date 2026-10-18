// INPUT: crypto/rand, encoding/binary, errors, math, strconv, strings, time, unicode/utf8
// OUTPUT: TaskKind, TaskStatus, Task, NewTask
// POS: [L2.3 agent contracts] Agent task identity/lifecycle/output cursors, independent of any execution manager or OS process.
package abi

import (
	"crypto/rand"
	"encoding/binary"
	"errors"
	"math"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"
)

type TaskKind string

const (
	TaskLocalBash         TaskKind = "local_bash"
	TaskLocalAgent        TaskKind = "local_agent"
	TaskRemoteAgent       TaskKind = "remote_agent"
	TaskInProcessTeammate TaskKind = "in_process_teammate"
	TaskLocalWorkflow     TaskKind = "local_workflow"
	TaskMonitorMCP        TaskKind = "monitor_mcp"
	TaskDream             TaskKind = "dream"
)

func (kind TaskKind) Prefix() (byte, error) {
	switch kind {
	case TaskLocalBash:
		return 'b', nil
	case TaskLocalAgent:
		return 'a', nil
	case TaskRemoteAgent:
		return 'r', nil
	case TaskInProcessTeammate:
		return 't', nil
	case TaskLocalWorkflow:
		return 'w', nil
	case TaskMonitorMCP:
		return 'm', nil
	case TaskDream:
		return 'd', nil
	}
	return 0, errors.New("unknown task kind")
}

type TaskStatus string

const (
	TaskPending   TaskStatus = "pending"
	TaskRunning   TaskStatus = "running"
	TaskSucceeded TaskStatus = "succeeded"
	TaskFailed    TaskStatus = "failed"
	TaskCancelled TaskStatus = "cancelled"
)

func (status TaskStatus) Terminal() bool {
	return status == TaskSucceeded || status == TaskFailed || status == TaskCancelled
}

// Task is a detached state value. The owner must synchronize mutations; path
// metadata never grants file access or means that a process has been started.
type Task struct {
	ID           string        `json:"id"`
	Kind         TaskKind      `json:"kind"`
	Title        string        `json:"title"`
	Status       TaskStatus    `json:"status"`
	ToolUseID    string        `json:"tool_use_id,omitempty"`
	CreatedAt    time.Time     `json:"created_at"`
	StartedAt    *time.Time    `json:"started_at,omitempty"`
	EndedAt      *time.Time    `json:"ended_at,omitempty"`
	TotalPaused  time.Duration `json:"total_paused_ns"`
	OutputFile   string        `json:"output_file,omitempty"`
	OutputOffset int64         `json:"output_offset"`
	Notified     bool          `json:"notified"`
}

func NewTask(kind TaskKind, title, toolUseID, outputFile string) (Task, error) {
	prefix, err := kind.Prefix()
	if err != nil {
		return Task{}, err
	}
	for _, text := range []string{title, toolUseID, outputFile} {
		if !utf8.ValidString(text) || len(text) > 16<<10 || strings.ContainsRune(text, 0) {
			return Task{}, errors.New("invalid bounded task metadata")
		}
	}
	var random [8]byte
	if _, err := rand.Read(random[:]); err != nil {
		return Task{}, err
	}
	id := strconv.FormatUint(binary.BigEndian.Uint64(random[:]), 36)
	for len(id) < 8 {
		id = "0" + id
	}
	return Task{ID: string(prefix) + id, Kind: kind, Title: title, Status: TaskPending, ToolUseID: toolUseID, CreatedAt: time.Now().UTC(), OutputFile: outputFile}, nil
}

func (task *Task) Transition(next TaskStatus) error {
	allowed := task.Status == TaskPending && (next == TaskRunning || next == TaskCancelled) || task.Status == TaskRunning && next.Terminal()
	if !allowed {
		return errors.New("invalid task state transition")
	}
	now := time.Now().UTC()
	if next == TaskRunning {
		task.StartedAt = &now
	}
	if next.Terminal() {
		task.EndedAt = &now
	}
	task.Status = next
	return nil
}

func (task *Task) AdvanceOutput(next int64) error {
	if next < task.OutputOffset || next < 0 {
		return errors.New("task output cursor cannot move backwards")
	}
	task.OutputOffset = next
	return nil
}

func (task *Task) AddPaused(duration time.Duration) error {
	if task.Status != TaskRunning || duration < 0 || task.TotalPaused > time.Duration(math.MaxInt64)-duration {
		return errors.New("invalid task paused duration")
	}
	task.TotalPaused += duration
	return nil
}
