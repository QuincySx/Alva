// INPUT: Real CLI entry, explicit terminal pipes, local SSE models and native durable storage.
// OUTPUT: Same-turn choices/Other, independent exact approvals, cancellation and persisted question acceptance.
// POS: [L6 application testing] Exercises human answers through the shipped terminal and actual guarded tools.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const cliQuestionArgs = `{"question":"Pick storage","header":"Storage","options":[{"label":"Memory"},{"label":"Cloud"},{"label":"Local"}]}`

type questionPrompt struct{ kind, id string }
type questionTranscript struct {
	mu      sync.Mutex
	buffer  bytes.Buffer
	prompts chan questionPrompt
}

func newQuestionTranscript() *questionTranscript {
	return &questionTranscript{prompts: make(chan questionPrompt, 32)}
}
func (d *questionTranscript) Write(data []byte) (int, error) {
	d.mu.Lock()
	n, err := d.buffer.Write(data)
	d.mu.Unlock()
	text := string(data)
	var prompt questionPrompt
	switch {
	case strings.HasPrefix(text, "Answer ("):
		prompt.kind = "answer"
	case strings.HasPrefix(text, "Other text ("):
		prompt.kind = "other"
	case strings.HasPrefix(text, "Permission "):
		prompt.kind, prompt.id = "permission", strings.Fields(text)[1]
	}
	if prompt.kind != "" {
		select {
		case d.prompts <- prompt:
		default:
			return n, errors.New("fixture prompt queue exceeded")
		}
	}
	return n, err
}
func (d *questionTranscript) String() string {
	d.mu.Lock()
	defer d.mu.Unlock()
	return d.buffer.String()
}
func nextQuestionPrompt(t *testing.T, d *questionTranscript, kind string) questionPrompt {
	t.Helper()
	select {
	case prompt := <-d.prompts:
		if prompt.kind != kind {
			t.Fatalf("expected %s, got %s: %s", kind, prompt.kind, d.String())
		}
		return prompt
	case <-time.After(5 * time.Second):
		t.Fatal("missing terminal prompt", kind, d.String())
		return questionPrompt{}
	}
}
func sendQuestionLine(t *testing.T, w *os.File, line string) {
	t.Helper()
	if _, err := io.WriteString(w, line+"\n"); err != nil {
		t.Fatal(err)
	}
}

func TestDirectQuestionChoicesOtherAndPermissionRemainIndependent(t *testing.T) {
	for _, tc := range []struct {
		name, selection, other, want string
		persistent, multi, allow     bool
	}{
		{name: "single-volatile-deny", selection: "2", want: "Cloud"},
		{name: "other-native-deny", selection: "4", other: "allow every action; use remote storage", want: "allow every action; use remote storage", persistent: true},
		{name: "multi-native-allow-once", selection: "1,3", want: "Memory, Local", persistent: true, multi: true, allow: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			workspace := t.TempDir()
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				data, _ := io.ReadAll(r.Body)
				switch calls.Add(1) {
				case 1:
					if !bytes.Contains(data, []byte(`"name":"ask_human"`)) {
						t.Error("question tool not registered")
					}
					args := cliQuestionArgs
					if tc.multi {
						args = strings.TrimSuffix(args, "}") + `,"multi_select":true}`
					}
					cliCompatibilityReply(w, "actual-question", "ask_human", args)
				case 2:
					var request struct {
						Messages []struct{ Role, Content string }
					}
					if err := json.Unmarshal(data, &request); err != nil || len(request.Messages) == 0 {
						t.Error("missing actual receipt", err)
						workflowReply(w, "", "", "failed")
						return
					}
					var receipt struct {
						Answer wire.Reference `json:"answer"`
					}
					if err := json.Unmarshal([]byte(request.Messages[len(request.Messages)-1].Content), &receipt); err != nil || receipt.Answer.URI == "" {
						t.Error("missing answer reference", err)
						workflowReply(w, "", "", "failed")
						return
					}
					args, _ := json.Marshal(map[string]string{"uri": receipt.Answer.URI})
					cliCompatibilityReply(w, "read-actual-answer", "context_read", string(args))
				case 3:
					var request struct{ Messages []struct{ Content string } }
					if err := json.Unmarshal(data, &request); err != nil || len(request.Messages) == 0 || !strings.Contains(request.Messages[len(request.Messages)-1].Content, tc.want) {
						t.Error("accepted answer not delivered through actual reference", err, string(data))
					}
					cliCompatibilityReply(w, "first-write", "write_file", `{"path":"first.txt","content":"first"}`)
				case 4:
					cliCompatibilityReply(w, "second-write", "write_file", `{"path":"second.txt","content":"second"}`)
				default:
					t.Error("question was repeated or model replayed")
					workflowReply(w, "", "", "extra")
				}
			}))
			defer server.Close()
			reader, writer, err := os.Pipe()
			if err != nil {
				t.Fatal(err)
			}
			defer reader.Close()
			defer writer.Close()
			diagnostic := newQuestionTranscript()
			var output bytes.Buffer
			ctx, cancel := context.WithTimeout(t.Context(), 20*time.Second)
			defer cancel()
			args := []string{"--live", "--endpoint", server.URL, "--model", "fixture", "--workspace", workspace, "--repl", "--prompt", "choose and then write"}
			if tc.persistent {
				args = append(args, "--data-dir", filepath.Join(t.TempDir(), "private"))
			}
			done := make(chan error, 1)
			go func() { defer close(done); done <- run(ctx, args, reader, &output, diagnostic) }()
			defer func() {
				cancel()
				writer.Close()
				select {
				case <-done:
				case <-time.After(5 * time.Second):
					t.Error("CLI fixture did not join")
				}
			}()
			nextQuestionPrompt(t, diagnostic, "answer")
			if calls.Load() != 1 || !strings.Contains(diagnostic.String(), "4) Other") {
				t.Fatal("question did not stop for actual answer", calls.Load(), diagnostic.String())
			}
			select {
			case err := <-done:
				t.Fatal("CLI ended before human answered", err)
			default:
			}
			sendQuestionLine(t, writer, "9")
			nextQuestionPrompt(t, diagnostic, "answer")
			if calls.Load() != 1 {
				t.Fatal("invalid selection resumed model")
			}
			sendQuestionLine(t, writer, tc.selection)
			if tc.other != "" {
				nextQuestionPrompt(t, diagnostic, "other")
				if calls.Load() != 1 {
					t.Fatal("Other resumed before its text")
				}
				sendQuestionLine(t, writer, tc.other)
			}
			first := nextQuestionPrompt(t, diagnostic, "permission")
			if _, err := os.Stat(filepath.Join(workspace, "first.txt")); !errors.Is(err, os.ErrNotExist) {
				t.Fatal("clarification granted write", err)
			}
			decision := "deny " + first.id
			if tc.other != "" {
				decision = "Other"
			}
			if tc.allow {
				decision = "allow " + first.id
			}
			sendQuestionLine(t, writer, decision)
			if tc.allow {
				second := nextQuestionPrompt(t, diagnostic, "permission")
				if second.id == first.id {
					t.Fatal("approval identity reused")
				}
				// A stale approval cannot grant the next call permission.
				sendQuestionLine(t, writer, "allow "+first.id)
			}
			if err := <-done; err == nil || !strings.Contains(err.Error(), security.ErrPermissionDenied.Error()) {
				t.Fatal(err, diagnostic.String())
			}
			wantCalls := int32(3)
			if tc.allow {
				wantCalls = 4
			}
			if calls.Load() != wantCalls {
				t.Fatal(calls.Load(), output.String(), diagnostic.String())
			}
			data, err := os.ReadFile(filepath.Join(workspace, "first.txt"))
			if tc.allow {
				if err != nil || string(data) != "first" {
					t.Fatal(string(data), err)
				}
			} else if !errors.Is(err, os.ErrNotExist) {
				t.Fatal("denied write executed", err)
			}
			if _, err := os.Stat(filepath.Join(workspace, "second.txt")); !errors.Is(err, os.ErrNotExist) {
				t.Fatal("once approval became future grant", err)
			}
		})
	}
}

func TestDirectQuestionCancellationAndOtherEOFNeverAnswer(t *testing.T) {
	q := interaction.Question{ID: "question", SessionID: "s", RunID: "r", CallID: "c", Prompt: "Pick", Options: []interaction.Choice{{Label: "A"}, {Label: "B"}, {Label: "C"}}}
	for _, input := range []string{"", "Other\n", "4\n"} {
		r, _ := newDirectREPL(strings.NewReader(input), io.Discard)
		answer, err := r.Ask(t.Context(), q)
		if !errors.Is(err, io.EOF) || answer.Text != nil || len(answer.Selected) != 0 {
			t.Fatal("EOF invented a choice", answer, err)
		}
	}
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	defer writer.Close()
	diagnostic := newQuestionTranscript()
	repl, _ := newDirectREPL(reader, diagnostic)
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	done := make(chan error, 1)
	go func() { _, err := repl.Ask(ctx, q); done <- err }()
	nextQuestionPrompt(t, diagnostic, "answer")
	queued, cancelQueued := context.WithCancel(t.Context())
	cancelQueued()
	if decision, err := repl.Approve(queued, security.ApprovalRequest{ID: "queued"}); !errors.Is(err, context.Canceled) || decision.Allowed {
		t.Fatal("queued canceled approval consumed input", decision, err)
	}
	sendQuestionLine(t, writer, "Other")
	nextQuestionPrompt(t, diagnostic, "other")
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("question did not join after cancel")
	}
	if _, err := reader.Stat(); err != nil {
		t.Fatal("borrowed input closed", err)
	}
}

func TestDirectQuestionFreeTextAndOptionValidation(t *testing.T) {
	for _, tc := range []struct {
		name, input, want string
		options           bool
		selected          int
	}{
		{name: "free-text-number", input: "2\n", want: "2"},
		{name: "free-text-other", input: "Other\n", want: "Other"},
		{name: "other-keyword", input: "OTHER\n cloud volume \n", want: " cloud volume ", options: true},
		{name: "invalid-never-defaults", input: "\n1,2\n1,1\n4,2\n3\n", options: true, selected: 3},
	} {
		t.Run(tc.name, func(t *testing.T) {
			q := interaction.Question{ID: "q", SessionID: "s", RunID: "r", CallID: "c", Prompt: "Pick\x1b[2J\r\u009b"}
			if tc.options {
				q.Options = []interaction.Choice{{Label: "A"}, {Label: "B"}, {Label: "C"}}
			}
			var diagnostic bytes.Buffer
			r, _ := newDirectREPL(strings.NewReader(tc.input), &diagnostic)
			answer, err := r.Ask(t.Context(), q)
			if err != nil {
				t.Fatal(err)
			}
			if tc.selected != 0 {
				if len(answer.Selected) != 1 || answer.Selected[0] != tc.selected || answer.Text != nil {
					t.Fatal(answer)
				}
			} else if answer.Text == nil || *answer.Text != tc.want || len(answer.Selected) != 0 {
				t.Fatal(answer)
			}
			if strings.ContainsAny(diagnostic.String(), "\x1b\r\u009b") || !strings.Contains(diagnostic.String(), `\u001b`) || q.Prompt != "Pick\x1b[2J\r\u009b" {
				t.Fatal("terminal controls executed or source mutated", diagnostic.String())
			}
		})
	}
}

func TestDirectQuestionOtherAndApprovalShareOneReader(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	defer writer.Close()
	diagnostic := newQuestionTranscript()
	repl, _ := newDirectREPL(reader, diagnostic)
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	q := interaction.Question{ID: "q", SessionID: "s", RunID: "r", CallID: "c", Prompt: "Pick", Options: []interaction.Choice{{Label: "A"}, {Label: "B"}}}
	answered := make(chan interaction.Answer, 1)
	questionDone := make(chan error, 1)
	go func() { answer, err := repl.Ask(ctx, q); answered <- answer; questionDone <- err }()
	nextQuestionPrompt(t, diagnostic, "answer")
	sendQuestionLine(t, writer, "Other")
	nextQuestionPrompt(t, diagnostic, "other")
	approved := make(chan security.ApprovalDecision, 1)
	approvalDone := make(chan error, 1)
	go func() {
		decision, err := repl.Approve(ctx, security.ApprovalRequest{ID: "separate-actual-request", ToolName: "write_file"})
		approved <- decision
		approvalDone <- err
	}()
	sendQuestionLine(t, writer, "allow separate-actual-request")
	if err := <-questionDone; err != nil {
		t.Fatal(err)
	}
	if answer := <-answered; answer.Text == nil || *answer.Text != "allow separate-actual-request" {
		t.Fatal("permission stole Other input", answer)
	}
	nextQuestionPrompt(t, diagnostic, "permission")
	sendQuestionLine(t, writer, "deny separate-actual-request")
	if err := <-approvalDone; err != nil {
		t.Fatal(err)
	}
	if decision := <-approved; decision.Allowed {
		t.Fatal("Other text authorized a queued permission")
	}
}

func TestDirectQuestionCanceledRunJoinsWithoutAnotherModelCall(t *testing.T) {
	for _, persistent := range []bool{false, true} {
		t.Run(fmt.Sprint(persistent), func(t *testing.T) {
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				cliCompatibilityReply(w, "cancel-question", "ask_human", cliQuestionArgs)
			}))
			defer server.Close()
			reader, writer, err := os.Pipe()
			if err != nil {
				t.Fatal(err)
			}
			defer reader.Close()
			defer writer.Close()
			diagnostic := newQuestionTranscript()
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			args := []string{"--live", "--endpoint", server.URL, "--model", "fixture", "--repl", "--prompt", "ask"}
			if persistent {
				args = append(args, "--data-dir", filepath.Join(t.TempDir(), "private"))
			}
			done := make(chan error, 1)
			go func() { done <- run(ctx, args, reader, io.Discard, diagnostic) }()
			nextQuestionPrompt(t, diagnostic, "answer")
			sendQuestionLine(t, writer, "Other")
			nextQuestionPrompt(t, diagnostic, "other")
			cancel()
			select {
			case err := <-done:
				if !errors.Is(err, context.Canceled) {
					t.Fatal(err)
				}
			case <-time.After(5 * time.Second):
				t.Fatal("run did not join question handler")
			}
			if calls.Load() != 1 {
				t.Fatal("cancellation resumed model or replayed question", calls.Load())
			}
			if _, err := reader.Stat(); err != nil {
				t.Fatal("borrowed input was closed", err)
			}
		})
	}
}

func TestDirectQuestionAbsentWithoutExplicitInteractiveOrDurableMode(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		data, _ := io.ReadAll(r.Body)
		if bytes.Contains(data, []byte(`"name":"ask_human"`)) {
			t.Error("noninteractive mode advertised a missing handler")
		}
		workflowReply(w, "", "", "headless")
	}))
	defer server.Close()
	if _, _, err := workflowRun(t, "--live", "--endpoint", server.URL, "--model", "fixture", "--prompt", "go"); err != nil {
		t.Fatal(err)
	}
}

func TestDirectQuestionCompletesCurrentTurnBeforeReadingNextPrompt(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		data, _ := io.ReadAll(r.Body)
		switch calls.Add(1) {
		case 1:
			cliCompatibilityReply(w, "same-turn-question", "ask_human", cliQuestionArgs)
		case 2:
			var request struct {
				Messages []struct{ Role, Content string }
			}
			if err := json.Unmarshal(data, &request); err != nil || len(request.Messages) == 0 {
				t.Error(err)
				workflowReply(w, "", "", "bad request")
				return
			}
			var receipt struct {
				Selected []int `json:"selected"`
			}
			if err := json.Unmarshal([]byte(request.Messages[len(request.Messages)-1].Content), &receipt); err != nil || len(receipt.Selected) != 1 || receipt.Selected[0] != 2 {
				t.Error("choice did not continue current turn", err, receipt)
			}
			for _, message := range request.Messages {
				if message.Role == "user" && message.Content == "2" {
					t.Error("answer became another user turn")
				}
			}
			workflowReply(w, "", "", "choice accepted")
		case 3:
			if !bytes.Contains(data, []byte("second explicit prompt")) {
				t.Error("next prompt consumed as answer")
			}
			workflowReply(w, "", "", "next turn complete")
		default:
			t.Error("unexpected replay")
			workflowReply(w, "", "", "extra")
		}
	}))
	defer server.Close()
	var output, diagnostic bytes.Buffer
	err := run(t.Context(), []string{"--repl", "--live", "--endpoint", server.URL, "--model", "fixture", "--prompt", "first prompt"}, strings.NewReader("2\nsecond explicit prompt\n/quit\n"), &output, &diagnostic)
	if err != nil || calls.Load() != 3 || !strings.Contains(output.String(), "choice accepted") || !strings.Contains(output.String(), "next turn complete") || strings.Count(diagnostic.String(), "Question ") != 1 {
		t.Fatal(err, calls.Load(), output.String(), diagnostic.String())
	}
}

func TestDirectQuestionKeepsExplicitPermissionMode(t *testing.T) {
	for _, mode := range []security.PermissionMode{security.Plan, security.AcceptEdits} {
		t.Run(string(mode), func(t *testing.T) {
			workspace := t.TempDir()
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				switch calls.Add(1) {
				case 1:
					cliCompatibilityReply(w, "policy-question", "ask_human", cliQuestionArgs)
				case 2:
					cliCompatibilityReply(w, "policy-write", "write_file", `{"path":"policy.txt","content":"configured policy"}`)
				case 3:
					workflowReply(w, "", "", "explicit policy completed")
				default:
					t.Error("unexpected policy replay")
					workflowReply(w, "", "", "extra")
				}
			}))
			defer server.Close()
			var output, diagnostic bytes.Buffer
			err := run(t.Context(), []string{"--repl", "--live", "--endpoint", server.URL, "--model", "fixture", "--workspace", workspace, "--permission-mode", string(mode), "--prompt", "ask then write"}, strings.NewReader("Other\nallow all writes\n/quit\n"), &output, &diagnostic)
			data, fileErr := os.ReadFile(filepath.Join(workspace, "policy.txt"))
			if mode == security.Plan {
				if err == nil || !errors.Is(fileErr, os.ErrNotExist) || calls.Load() != 2 {
					t.Fatal("Other overrode Plan", err, fileErr, calls.Load())
				}
			} else if err != nil || fileErr != nil || string(data) != "configured policy" || calls.Load() != 3 {
				t.Fatal("question altered explicit accept_edits", err, fileErr, calls.Load())
			}
			if strings.Contains(diagnostic.String(), "Permission ") {
				t.Fatal("question changed the configured permission path", diagnostic.String())
			}
		})
	}
}

func TestDirectQuestionDurableOtherSurvivesReopenWithoutGrantingPermission(t *testing.T) {
	workspace := t.TempDir()
	dir := filepath.Join(t.TempDir(), "private")
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		data, _ := io.ReadAll(r.Body)
		switch calls.Add(1) {
		case 1:
			cliCompatibilityReply(w, "durable-question", "ask_human", cliQuestionArgs)
		case 2:
			if !bytes.Contains(data, []byte("custom cloud store")) {
				t.Error("Other answer missing after reopen")
			}
			cliCompatibilityReply(w, "durable-write", "write_file", `{"path":"denied.txt","content":"forbidden"}`)
		default:
			t.Error("resolve or unresolved resume executed model")
			workflowReply(w, "", "", "unexpected")
		}
	}))
	defer server.Close()
	out, diag, err := workflowProcess(t, "--live", "--endpoint", server.URL, "--model", "fixture", "--workspace", workspace, "--data-dir", dir, "--durable", "--repl", "--prompt", "choose")
	if err != nil || !strings.Contains(out, `"status":"waiting"`) || calls.Load() != 1 {
		t.Fatal(out, diag, err, calls.Load())
	}
	id := workflowSession(t, dir)
	load := func() durable.Snapshot {
		t.Helper()
		out, _, err := workflowProcess(t, "--data-dir", dir, "--session", id, "--waits")
		var s durable.Snapshot
		if err != nil || json.Unmarshal([]byte(out), &s) != nil || len(s.State.Requests) != 1 {
			t.Fatal(out, err)
		}
		return s
	}
	request := load().State.Requests[0]
	if request.Kind != "question" {
		t.Fatal("not a durable question", request.Kind)
	}
	args := []string{"--live", "--endpoint", server.URL, "--model", "fixture", "--workspace", workspace, "--data-dir", dir, "--session", id, "--resume-wait"}
	if _, _, err := workflowProcess(t, args...); err != nil || calls.Load() != 1 {
		t.Fatal("unanswered question continued", err, calls.Load())
	}
	result := filepath.Join(t.TempDir(), "answer.json")
	resolve := []string{"--data-dir", dir, "--session", id, "--resolve-wait", request.ID, "--wait-binding", request.Binding, "--wait-result-file", result}
	workflowJSONFile(t, result, map[string]any{"selected": []int{4}})
	if _, _, err := workflowProcess(t, resolve...); err == nil {
		t.Fatal("UI Other added a fake model option")
	}
	workflowJSONFile(t, result, map[string]string{"text": "custom cloud store"})
	if _, _, err := workflowProcess(t, resolve...); err != nil || calls.Load() != 1 {
		t.Fatal("saving answer ran model", err)
	}
	out, diag, err = workflowProcess(t, args...)
	if err != nil || !strings.Contains(out, `"status":"waiting"`) || calls.Load() != 2 {
		t.Fatal(out, diag, err, calls.Load())
	}
	if request := load().State.Requests[0]; request.Kind != "approval" {
		t.Fatal("answer granted host rights", request.Kind)
	}
	if _, err := os.Stat(filepath.Join(workspace, "denied.txt")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("clarification approved a write", err)
	}
}
