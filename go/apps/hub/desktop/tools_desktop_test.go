// INPUT: MyGo Tester, real product settings and an owned human-question broker
// OUTPUT: Actual settings controls and clarification answers remain separate from permission approval
// POS: [L7 Hub product] Headless native control acceptance; no claim of OS window or accessibility verification.
//go:build desktop

package hubdesktop

import (
	"context"
	"github.com/QuincySx/Alva/go/extension/interaction"
	"github.com/QuincySx/Alva/go/platform/host"
	"github.com/egoist/mygo/ui"
	"testing"
	"time"
)

func TestNativeOptionalToolsAndHumanClarification(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	product, err := s.Create(ctx, "tools", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.selected = product.ID
	p.detail, err = s.Store.Detail(ctx, product.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	tester := ui.NewTester(p.Build, 1400, 1600)
	if err = tester.Click("Session settings"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if err = tester.Click("Optional tools and human questions"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if err = tester.Click("Enable Human questions next turn"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if !p.settings.Tools.Interaction || s.Runtime.HasActive() {
		t.Fatal("opt-in did not persist cold")
	}
	broker, err := host.NewQuestionBroker(8)
	if err != nil {
		t.Fatal(err)
	}
	defer broker.Close()
	s.Questions = broker
	native := "question-native"
	if _, err = s.Repository.Create(ctx, product.ID, native, ""); err != nil {
		t.Fatal(err)
	}
	if err = s.Store.BindNative(ctx, product.ID, p.detail.Session.ActiveBranchID, native); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Store.BeginTurn(ctx, product.ID, "actual question owner"); err != nil {
		t.Fatal(err)
	}
	q := interaction.Question{ID: "question-fixture", SessionID: native, RunID: "run-fixture", CallID: "call-fixture", Prompt: "Pick one real answer", Options: []interaction.Choice{{Label: "First choice"}, {Label: "Second choice"}}}
	answered := make(chan interaction.Answer, 1)
	wait, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	go func() { a, _ := broker.Ask(wait, q); answered <- a }()
	deadline := time.Now().Add(time.Second)
	for len(broker.PendingQuestionsSnapshot()) == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	p.questions, err = s.PendingQuestions(ctx, product.ID)
	if err != nil || len(p.questions) != 1 {
		t.Fatal("question not product-owned", err)
	}
	tester.Frame()
	if err = tester.Click("2. Second choice"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	select {
	case a := <-answered:
		if len(a.Selected) != 1 || a.Selected[0] != 2 {
			t.Fatal("actual broker answer lost", a)
		}
	case <-wait.Done():
		t.Fatal("UI never answered actual question")
	}
	approvals, err := s.PendingApprovals(ctx, product.ID)
	if err != nil || len(approvals) != 0 {
		t.Fatal("clarification created permission authority")
	}
}
