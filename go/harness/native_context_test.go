// INPUT: context, filepath, strings, testing, storage/session, agent/context, kernel/abi, llm/wire
// OUTPUT: Native Source and explicit ContextSystem co-assembly integration regression
// POS: [L5 harness testing] Proves source selection does not replace independent working-set lifecycle policies.
package harness

import (
	"context"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/platform/storage/session"
	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type nativeContextObserver struct {
	t     *testing.T
	calls int
}

func (*nativeContextObserver) Name() string { return "native-context-observer" }
func (observer *nativeContextObserver) BeforeModel(_ context.Context, _ abi.RuntimeContext, request *abi.ModelRequest) error {
	observer.calls++
	if len(request.Messages) > 75 {
		observer.t.Fatal("native source did not bound context", len(request.Messages))
	}
	var constant, notice bool
	for _, message := range request.Messages {
		constant = constant || strings.Contains(message.Text(), "always policy")
		notice = notice || strings.Contains(message.Text(), "not a summary")
	}
	if !constant || !notice {
		observer.t.Fatal("source replaced explicit context policy", constant, notice)
	}
	return nil
}
func TestNativeSourceCoexistsWithExplicitContextLifecycle(t *testing.T) {
	repo, err := session.Open(t.Context(), filepath.Join(t.TempDir(), "facts"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	log, err := repo.Create(t.Context(), "owner", "native-context-policy", "")
	if err != nil {
		t.Fatal(err)
	}
	var batch []abi.SessionRecord
	for i := 0; i < 128; i++ {
		message := wire.TextMessage(wire.RoleUser, "older retained fact")
		batch = append(batch, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message}))
	}
	if _, err := log.AppendRecords(t.Context(), batch); err != nil {
		t.Fatal(err)
	}
	plugins, err := NativeSessionPlugins(t.Context(), repo, "owner", log.ID())
	if err != nil {
		t.Fatal(err)
	}
	system := agentcontext.DefaultSystem(0, 0)
	constant := wire.TextMessage(wire.RoleSystem, "always policy")
	constant.ID = "always"
	if err := system.Handle.InjectMessage(log.ID(), abi.AlwaysPresent, constant); err != nil {
		t.Fatal(err)
	}
	observer := &nativeContextObserver{t: t}
	builder := NewDemoBuilder(log).Workspace(t.TempDir()).ContextSystem(system).Middleware(observer)
	for _, plugin := range plugins {
		builder.Plugin(plugin)
	}
	agent, err := builder.Build(t.Context())
	if err != nil {
		t.Fatal("source conflicted with explicit lifecycle", err)
	}
	if err := agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "latest")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if observer.calls != 2 {
		t.Fatal("did not execute native model/tool/model", observer.calls)
	}
}
