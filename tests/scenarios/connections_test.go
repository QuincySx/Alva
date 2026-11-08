// INPUT: hubFixture real assembly with saved model connections, fakeModel catalog/stream endpoints and an httptest TLS server
// OUTPUT: Scenarios for adding http/https connections, reading the model catalog, first message, wrong key, rate limit and deleting a connection in use
// POS: [scenario] Connections from the settings page to the first real request and back.
package scenarios

import (
	"context"
	"errors"
	"io"
	"log"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func connectionDraft(name, protocol, endpoint, key string, models ...string) hub.ConnectionUpdate {
	u := hub.ConnectionUpdate{Name: name, Protocol: protocol, Endpoint: endpoint, CatalogURL: hub.DefaultCatalogURL(endpoint), Auth: "none", Enabled: true}
	if key != "" {
		u.Auth, u.APIKey = "api_key", key
	}
	for _, id := range models {
		u.Models = append(u.Models, hub.ConnectionModel{ID: id, Enabled: true})
	}
	return u
}

func targetOf(t *testing.T, h *hubFixture, c hub.ModelConnection, model string) hub.Target {
	t.Helper()
	for _, m := range c.Models {
		if m.ID == model {
			target := c.Target(m)
			if product.ValidateConfiguredTarget(target, h.service().AvailableTargets()) != nil {
				t.Fatalf("model %s is not selectable", model)
			}
			return target
		}
	}
	t.Fatalf("connection has no model %s", model)
	return hub.Target{}
}

func TestUserAddsConnectionsReadsCatalogAndSendsFirstMessage(t *testing.T) {
	for _, tc := range []struct {
		protocol string
		kind     provider.Kind
		header   string
	}{
		{"openai_chat", provider.OpenAIChatKind, "Authorization"},
		{"anthropic", provider.AnthropicKind, "X-Api-Key"},
	} {
		t.Run(tc.protocol, func(t *testing.T) {
			model := newFakeModel(t, tc.kind)
			model.catalog = []string{"model-b", "model-a"}
			model.next = func(r modelRequest) reply { return text("hello from ", "the connection") }
			h := openHub(t, demoConfig(t))
			ctx := context.Background()
			draft := connectionDraft("Local gateway", tc.protocol, model.endpoint(), "secret-key-1")
			if !strings.HasPrefix(draft.Endpoint, "http://") {
				t.Fatal("fixture is plain http")
			}
			catalog, err := h.service().Connections.FetchModels(ctx, draft)
			if err != nil {
				t.Fatal("fetch catalog:", err)
			}
			if len(catalog.Models) != 2 || catalog.Models[0].ID != "model-a" || catalog.Models[1].ID != "model-b" {
				t.Fatalf("catalog %+v", catalog.Models)
			}
			probe, err := h.service().Connections.TestConnection(ctx, draft)
			if err != nil || probe.ModelCount != 2 {
				t.Fatalf("connection test %+v %v", probe, err)
			}
			if model.calls() != 0 {
				t.Fatal("reading the catalog sent a generation request")
			}
			draft.Models = []hub.ConnectionModel{{ID: "model-b", Enabled: true}}
			saved, err := h.service().Connections.Save(ctx, draft)
			if err != nil {
				t.Fatal(err)
			}
			s := h.create("first message", "")
			if _, err := h.service().SwitchTarget(ctx, s.ID, targetOf(t, h, saved, "model-b"), "user picked a model"); err != nil {
				t.Fatal(err)
			}
			h.send(s.ID, "hi")
			h.expect(s.ID, engine.Succeeded)
			first := model.request(1)
			want := "secret-key-1"
			if tc.header == "Authorization" {
				want = "Bearer secret-key-1"
			}
			if first.Header.Get(tc.header) != want || !first.has(`"model-b"`) {
				t.Fatalf("request auth %q / body %s", first.Header.Get(tc.header), first.Body)
			}
			if r := lastReply(t, h.blocks(s.ID)); r.Text != "hello from the connection" {
				t.Fatalf("reply %+v", r)
			}
			if strings.Contains(h.dump(s.ID), "secret-key-1") {
				t.Fatal("API key entered product history")
			}
		})
	}
}

// An https connection is accepted and reaches the TLS server, but the catalog
// client trusts only the system roots: a self-signed local server is refused.
func TestHTTPSConnectionUsesTLSAndRefusesUntrustedCertificates(t *testing.T) {
	var reached bool
	server := httptest.NewUnstartedServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { reached = true }))
	server.Config.ErrorLog = log.New(io.Discard, "", 0)
	server.StartTLS()
	defer server.Close()
	h := openHub(t, demoConfig(t))
	draft := connectionDraft("TLS gateway", "openai_chat", server.URL+"/v1/chat/completions", "tls-secret", "tls-model")
	if !strings.HasPrefix(draft.Endpoint, "https://") {
		t.Fatal("fixture is https")
	}
	_, err := h.service().Connections.FetchModels(context.Background(), draft)
	if err == nil || reached || strings.Contains(err.Error(), "tls-secret") {
		t.Fatalf("untrusted certificate result: reached=%v err=%v", reached, err)
	}
	saved, err := h.service().Connections.Save(context.Background(), draft)
	if err != nil || !strings.HasPrefix(saved.Endpoint, "https://") {
		t.Fatalf("https connection not saved: %+v %v", saved, err)
	}
}

func TestWrongKeyFailsAsAuthenticationAndFixingItNeedsReselectingTheModel(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind)
	model.next = func(r modelRequest) reply {
		if r.Header.Get("Authorization") != "Bearer right-key" {
			return reply{Status: 401}
		}
		if r.typed("hit the limit") {
			return reply{Status: 429}
		}
		return text("authorized")
	}
	h := openHub(t, demoConfig(t))
	ctx := context.Background()
	saved, err := h.service().Connections.Save(ctx, connectionDraft("Keyed", "openai_chat", model.endpoint(), "wrong-key", "keyed-model"))
	if err != nil {
		t.Fatal(err)
	}
	s := h.create("keys", "")
	if _, err := h.service().SwitchTarget(ctx, s.ID, targetOf(t, h, saved, "keyed-model"), "pick"); err != nil {
		t.Fatal(err)
	}
	h.send(s.ID, "hello")
	h.expect(s.ID, engine.Failed)
	if errs := errorEvents(h.history(s.ID)); len(errs) != 1 || errs[0].Failure != engine.FailureAuth {
		t.Fatalf("wrong key failure %+v", errs)
	}
	// The user fixes the key. The edit creates a new connection revision, so
	// the conversation's saved route is refused until a model is picked again.
	update := connectionDraft("Keyed", "openai_chat", model.endpoint(), "right-key", "keyed-model")
	update.ID, update.ExpectedRevision = saved.ID, saved.Revision
	fixed, err := h.service().Connections.Save(ctx, update)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := h.service().Submit(ctx, s.ID, "after fixing the key", nil); !errors.Is(err, product.ErrRouteUnavailable) {
		t.Fatalf("submit on the edited connection's old revision: %v", err)
	}
	if _, err := h.service().SwitchTarget(ctx, s.ID, targetOf(t, h, fixed, "keyed-model"), "re-pick after key change"); err != nil {
		t.Fatal(err)
	}
	h.send(s.ID, "after fixing the key")
	h.expect(s.ID, engine.Succeeded)
	if branches := h.detail(s.ID).Branches; len(branches) != 2 {
		t.Logf("branches after re-pick: %d", len(branches))
	}
	// A rate limit is a typed, retryable failure; sending again works.
	h.send(s.ID, "hit the limit")
	h.expect(s.ID, engine.Failed)
	errs := errorEvents(h.history(s.ID))
	if last := errs[len(errs)-1]; last.Failure != engine.FailureRateLimit {
		t.Fatalf("rate limit failure %+v", last)
	}
	h.send(s.ID, "try again")
	h.expect(s.ID, engine.Succeeded)
}

func TestDeletingAConnectionInUseFinishesTheTurnThenAsksForAnotherModel(t *testing.T) {
	hold, reached := make(chan struct{}), make(chan struct{})
	model := newFakeModel(t, provider.OpenAIChatKind, reply{Text: []string{"still "}, Hold: hold, Reached: reached, After: []string{"answering"}})
	h := openHub(t, demoConfig(t))
	ctx := context.Background()
	saved, err := h.service().Connections.Save(ctx, connectionDraft("Doomed", "openai_chat", model.endpoint(), "", "doomed-model"))
	if err != nil {
		t.Fatal(err)
	}
	s := h.create("in use", "")
	if _, err := h.service().SwitchTarget(ctx, s.ID, targetOf(t, h, saved, "doomed-model"), "pick"); err != nil {
		t.Fatal(err)
	}
	h.send(s.ID, "long answer please")
	received(t, reached, "the running request")
	if err := h.service().Connections.Delete(ctx, saved.ID, saved.Revision); err != nil {
		t.Fatalf("delete while in use: %v", err)
	}
	close(hold)
	h.expect(s.ID, engine.Succeeded)
	if r := lastReply(t, h.blocks(s.ID)); r.Text != "still answering" {
		t.Fatalf("running Turn changed by the deletion: %+v", r)
	}
	if _, err := h.service().Submit(ctx, s.ID, "next", nil); !errors.Is(err, product.ErrRouteUnavailable) {
		t.Fatalf("submit on a deleted connection: %v", err)
	}
	// History stays readable; the user picks the configured model and continues.
	if len(blocksOf(h.blocks(s.ID), product.BlockKindReply)) != 1 {
		t.Fatal("history lost after deleting the connection")
	}
	if _, err := h.service().SwitchTarget(ctx, s.ID, h.service().DefaultTarget, "connection deleted"); err != nil {
		t.Fatal(err)
	}
	h.send(s.ID, "continue elsewhere")
	h.expect(s.ID, engine.Succeeded)
	handoff := false
	for _, e := range h.history(s.ID) {
		if e.Timeline != nil && e.Timeline.Kind == "handoff" {
			handoff = true
		}
	}
	if !handoff {
		t.Fatalf("switching away from the deleted connection carried no handoff: %s", h.dump(s.ID))
	}
	h.reopen()
	if _, err := h.service().Submit(ctx, s.ID, "after restart", nil); err != nil {
		t.Fatal(err)
	}
	h.expect(s.ID, engine.Succeeded)
}
