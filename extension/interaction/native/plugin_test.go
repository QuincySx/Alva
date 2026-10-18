// INPUT: context, encoding/json, errors, filepath, strings, testing/time, SDK/security, host QuestionBroker and native facts.
// OUTPUT: Real question/answer references, structured selection, cancellation and no-handler/no-authority regressions.
// POS: [L4 extension] The actual host broker is a tested consumer, with no fake stdin wait or approval implication.
package native_test

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/extension/interaction"
	interactionnative "github.com/QuincySx/Alva/extension/interaction/native"
	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type model struct{}

func (model) Stream(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error {
	return nil
}
func TestRealHumanHandlerAnswerIsDurableReferenceNotInlineOrApproval(t *testing.T) {
	ctx := context.Background()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	log, err := repo.Create(ctx, "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	cwd := t.TempDir()
	guard, _ := security.NewGuard(cwd, nil, nil, security.NewPermissionManager(security.Ask, nil))
	broker, _ := host.NewQuestionBroker(4)
	defer broker.Close()
	p, err := interactionnative.New(ctx, interactionnative.Config{Repository: repo, Owner: "owner", SessionID: "native", Cwd: cwd, Guard: guard, Handler: broker})
	if err != nil {
		t.Fatal(err)
	}
	tool := p.Tools()[0]
	state, _ := abi.NewAgentState(model{}, log, p.Tools())
	runtime := abi.RuntimeContext{RunID: "run", State: state, Workspace: cwd}
	if err = p.OnRunStart(ctx, runtime); err != nil {
		t.Fatal(err)
	}
	e := abi.ToolExecutionContext{ActualTool: tool, Session: log, SessionID: "native", RunID: "run", CallID: "call", Workspace: cwd}
	call := wire.ToolCall{ID: "call", Name: "ask_human", Arguments: json.RawMessage(`{"question":"Which actual answer?","options":[{"label":"A"},{"label":"B"}]}`)}
	type outcome struct {
		result wire.ToolResult
		err    error
	}
	done := make(chan outcome, 1)
	go func() { result, err := tool.Execute(ctx, e, call); done <- outcome{result, err} }()
	var question interaction.Question
	until := time.NewTimer(time.Second)
	defer until.Stop()
	tick := time.NewTicker(time.Millisecond)
	defer tick.Stop()
	for question.ID == "" {
		items := broker.PendingQuestionsSnapshot()
		if len(items) > 0 {
			question = items[0]
			break
		}
		select {
		case <-until.C:
			t.Fatal("no actual pending question")
		case <-tick.C:
		}
	}
	answer := strings.Repeat("human-source-only🙂", 2000)
	if err = broker.RespondQuestion("native", question.ID, interaction.Answer{Text: &answer}); err != nil {
		t.Fatal(err)
	}
	out := <-done
	if out.err != nil {
		t.Fatal(out.err)
	}
	if strings.Contains(out.result.Content[0].Text, "human-source-only") {
		t.Fatal("large raw answer automatically fed to model")
	}
	var receipt struct {
		Answer wire.Reference `json:"answer"`
	}
	if err = json.Unmarshal([]byte(out.result.Content[0].Text), &receipt); err != nil {
		t.Fatal(err)
	}
	_, raw, err := repo.ReadArtifactFull(ctx, "owner", "native", receipt.Answer.URI, 128<<10)
	if err != nil || !strings.Contains(string(raw), answer) {
		t.Fatal("full actual answer missing", err)
	}
	if err = p.OnRunEnd(ctx, runtime, nil); err != nil {
		t.Fatal(err)
	}
	q := interaction.Question{ID: "q", SessionID: "s", RunID: "r", CallID: "c", Prompt: "q", Options: []interaction.Choice{{Label: "A"}, {Label: "B"}}, MultiSelect: true}
	resolved, err := q.Resolve(interaction.Answer{Selected: []int{2, 1}})
	if err != nil || resolved.Text != "B, A" {
		t.Fatal("actual selection mapping failed", err)
	}
	if _, err = q.Resolve(interaction.Answer{Selected: []int{1, 1}}); err == nil {
		t.Fatal("duplicate option accepted")
	}
	var absent interaction.HandlerFunc
	if _, err = interactionnative.New(ctx, interactionnative.Config{Repository: repo, Owner: "owner", SessionID: "native", Cwd: cwd, Guard: guard, Handler: absent}); err == nil {
		t.Fatal("typed nil handler accepted")
	}
	_, err = tool.Execute(ctx, e, call)
	if err == nil || errors.Is(err, security.ErrApprovalRequired) {
		t.Fatal("ended run accepted or clarification confused with approval", err)
	}
}
