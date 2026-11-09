// INPUT: Real Hub assembly with an in-memory vault, same-binary stdio MCP child and a local fake OAuth/MCP server
// OUTPUT: Probe, application-scope assembly, OAuth authorize/refresh/revoke and no-plaintext-on-disk regressions
// POS: [L7 Hub assembly] OAuth runs only against 127.0.0.1 fixtures; tokens live in the vault, never the data directory
package bootstrap

import (
	"bytes"
	"context"
	"encoding/json"
	"io/fs"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/llm/provider"
	mcpoauth "github.com/QuincySx/Alva/integration/protocol/mcp/oauth"
	"github.com/QuincySx/Alva/platform/system/keychain"
)

func connectorApp(t *testing.T, vault keychain.Vault) (*Application, string) {
	t.Helper()
	directory := filepath.Join(t.TempDir(), "private")
	app, err := Open(context.Background(), Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: directory, Demo: true, MCPVault: vault})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { closeApp(t, app) })
	if app.Service.Settings == nil {
		t.Fatal("assembly did not install the settings service")
	}
	return app, directory
}

func saveApplicationMCP(t *testing.T, app *Application, scope product.SettingsScope, servers ...hub.MCPServer) {
	t.Helper()
	view, err := app.Service.Settings.Load(context.Background(), scope)
	if err != nil {
		t.Fatal(err)
	}
	desired := view.Capabilities
	desired.MCP = servers
	if _, err = app.Service.Settings.Save(context.Background(), scope, product.SectionMCP, view.Fingerprints[product.SectionMCP], desired); err != nil {
		t.Fatal(err)
	}
}

func TestMCPProbeListsToolsOnlyForApprovedWorkspace(t *testing.T) {
	app, _ := connectorApp(t, keychain.NewMemory())
	ctx := context.Background()
	session, err := app.Service.Create(ctx, "probe", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(t.TempDir(), "started")
	scope := product.SettingsScope{App: true, SessionID: session.ID}
	saveApplicationMCP(t, app, scope, hub.MCPServer{ID: "fixture", Command: exe, Args: []string{"-test.run=^TestNativeMCPResourceHelper$", "--"}, Env: map[string]string{"ALVA_MCP_RESOURCE_MARKER": marker}},
		// The broken server prints its secret to stderr; the probe report must redact it.
		hub.MCPServer{ID: "broken", Command: "/bin/sh", Args: []string{"-c", `echo "login failed for $TOKEN" >&2; exit 3`}, Env: map[string]string{"TOKEN": "probe-secret-value"}})
	if _, err = app.Service.Settings.ProbeMCP(ctx, scope, "fixture"); err == nil {
		t.Fatal("unapproved server was started")
	}
	if _, err = os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("probe ran an unapproved command", err)
	}
	for _, id := range []string{"fixture", "broken"} {
		if _, err = app.Service.Settings.SetEnabled(ctx, scope, hub.CapabilityMCP, id, true); err != nil {
			t.Fatal(err)
		}
	}
	probe, err := app.Service.Settings.ProbeMCP(ctx, scope, "fixture")
	if err != nil || probe.State != product.MCPProbeConnected || probe.TotalTools != 1 || probe.Tools[0].Name != "echo" {
		t.Fatal("approved probe did not list tools", probe, err)
	}
	if cached, ok := app.Service.Settings.Probe(scope, "fixture"); !ok || cached.TotalTools != 1 {
		t.Fatal("probe result not kept for the form")
	}
	failed, err := app.Service.Settings.ProbeMCP(ctx, scope, "broken")
	if err != nil || failed.State != product.MCPProbeFailed || !strings.Contains(failed.Problem, "login failed") || strings.Contains(failed.Problem, "probe-secret-value") {
		t.Fatal("failed probe missing or leaked a secret", failed, err)
	}
}

func TestApplicationMCPReachesTheAttemptOnlyWithWorkspaceApproval(t *testing.T) {
	ctx := context.Background()
	var app *Application
	var calls int
	var mu sync.Mutex
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		calls++
		first := calls%2 == 1
		mu.Unlock()
		delta := map[string]any{"content": "done"}
		finish := "stop"
		if first {
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "mcp-call", "type": "function", "function": map[string]string{"name": "mcp_runtime", "arguments": `{"action":"call_tool","server_id":"fixture","tool_name":"echo","arguments":{}}`}}}}
			finish = "tool_calls"
		}
		data, _ := json.Marshal(map[string]any{"id": "fixture", "choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
		w.Header().Set("Content-Type", "text/event-stream")
		w.Write([]byte("data: " + string(data) + "\n\ndata: [DONE]\n\n"))
	}))
	defer server.Close()
	var err error
	app, err = Open(ctx, Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "fixture"}, MCPVault: keychain.NewMemory()})
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, app)
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(t.TempDir(), "started")
	approved, err := createLive(app.Service, ctx, "approved workspace", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	other, err := createLive(app.Service, ctx, "other workspace", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	saveApplicationMCP(t, app, product.SettingsScope{App: true}, hub.MCPServer{ID: "fixture", Command: exe, Args: []string{"-test.run=^TestNativeMCPResourceHelper$", "--"}, Env: map[string]string{"ALVA_MCP_RESOURCE_MARKER": marker}})
	if _, err = app.Service.Settings.SetEnabled(ctx, product.SettingsScope{App: true, SessionID: approved.ID}, hub.CapabilityMCP, "fixture", true); err != nil {
		t.Fatal(err)
	}
	// The other workspace has the same application intent but no approval:
	// the attempt has no MCP gateway, so the model's call cannot start it.
	if _, err = app.Service.Submit(ctx, other.ID, "call the fixture", nil); err != nil {
		t.Fatal(err)
	}
	awaitTerminal(t, app, other.ID)
	if _, err = os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("unapproved workspace started the application MCP server", err)
	}
	if _, err = app.Service.Submit(ctx, approved.ID, "call the fixture", nil); err != nil {
		t.Fatal(err)
	}
	awaitTerminal(t, app, approved.ID)
	if _, err = os.Stat(marker); err != nil {
		t.Fatal("approved workspace did not run the application MCP server", err)
	}
}

type fakeOAuthMCP struct {
	mu      sync.Mutex
	base    string
	revoked []string
	access  map[string]bool
	refresh map[string]string
	issued  int
}

func newFakeOAuthMCP(t *testing.T) (*fakeOAuthMCP, *httptest.Server) {
	t.Helper()
	fake := &fakeOAuthMCP{access: map[string]bool{}, refresh: map[string]string{}}
	mux := http.NewServeMux()
	jsonReply := func(w http.ResponseWriter, value any) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(value)
	}
	var challenge, redirect string
	mux.HandleFunc("/mcp", func(w http.ResponseWriter, r *http.Request) {
		fake.mu.Lock()
		ok := fake.access[strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")]
		fake.mu.Unlock()
		if !ok {
			w.Header().Set("WWW-Authenticate", `Bearer resource_metadata="`+fake.base+`/.well-known/oauth-protected-resource/mcp"`)
			w.WriteHeader(http.StatusUnauthorized)
			return
		}
		var message struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
		}
		json.NewDecoder(r.Body).Decode(&message)
		if message.Method == "notifications/initialized" || r.Method == http.MethodDelete {
			w.WriteHeader(http.StatusAccepted)
			return
		}
		result := map[string]any{"tools": []any{map[string]any{"name": "remote_tool", "inputSchema": map[string]string{"type": "object"}}}}
		if message.Method == "initialize" {
			result = map[string]any{"protocolVersion": "2025-11-25", "capabilities": map[string]any{"tools": map[string]any{}}, "serverInfo": map[string]string{"name": "oauth-fixture", "version": "1"}}
		}
		jsonReply(w, map[string]any{"jsonrpc": "2.0", "id": message.ID, "result": result})
	})
	mux.HandleFunc("/.well-known/oauth-protected-resource/mcp", func(w http.ResponseWriter, r *http.Request) {
		jsonReply(w, map[string]any{"resource": fake.base + "/mcp", "authorization_servers": []string{fake.base}})
	})
	mux.HandleFunc("/.well-known/oauth-authorization-server", func(w http.ResponseWriter, r *http.Request) {
		jsonReply(w, map[string]any{"issuer": fake.base, "authorization_endpoint": fake.base + "/authorize", "token_endpoint": fake.base + "/token", "registration_endpoint": fake.base + "/register", "revocation_endpoint": fake.base + "/revoke", "code_challenge_methods_supported": []string{"S256"}, "response_types_supported": []string{"code"}})
	})
	mux.HandleFunc("/register", func(w http.ResponseWriter, r *http.Request) {
		var request map[string]any
		json.NewDecoder(r.Body).Decode(&request)
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusCreated)
		json.NewEncoder(w).Encode(map[string]any{"client_id": "alva-client", "redirect_uris": request["redirect_uris"], "token_endpoint_auth_method": "none"})
	})
	mux.HandleFunc("/authorize", func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query()
		challenge, redirect = q.Get("code_challenge"), q.Get("redirect_uri")
		http.Redirect(w, r, redirect+"?"+url.Values{"code": {"the-code"}, "state": {q.Get("state")}}.Encode(), http.StatusFound)
	})
	mux.HandleFunc("/token", func(w http.ResponseWriter, r *http.Request) {
		r.ParseForm()
		fake.mu.Lock()
		defer fake.mu.Unlock()
		form := r.PostForm
		valid := form.Get("grant_type") == "authorization_code" && form.Get("code") == "the-code" && mcpoauth.Challenge(form.Get("code_verifier")) == challenge && form.Get("redirect_uri") == redirect
		if form.Get("grant_type") == "refresh_token" {
			_, valid = fake.refresh[form.Get("refresh_token")]
			delete(fake.refresh, form.Get("refresh_token"))
		}
		if !valid {
			w.WriteHeader(http.StatusBadRequest)
			jsonReply(w, map[string]string{"error": "invalid_grant"})
			return
		}
		fake.issued++
		access, refresh := "access-token-"+string(rune('0'+fake.issued)), "refresh-token-"+string(rune('0'+fake.issued))
		fake.access[access], fake.refresh[refresh] = true, access
		jsonReply(w, map[string]any{"access_token": access, "token_type": "Bearer", "expires_in": 3600, "refresh_token": refresh})
	})
	mux.HandleFunc("/revoke", func(w http.ResponseWriter, r *http.Request) {
		r.ParseForm()
		fake.mu.Lock()
		fake.revoked = append(fake.revoked, r.PostForm.Get("token"))
		delete(fake.access, r.PostForm.Get("token"))
		fake.mu.Unlock()
	})
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)
	fake.base = server.URL
	return fake, server
}

func followBrowser(t *testing.T) func(string) error {
	return func(address string) error {
		go func() {
			client := &http.Client{CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
			response, err := client.Get(address)
			if err != nil {
				t.Error(err)
				return
			}
			response.Body.Close()
			if callback, err := http.Get(response.Header.Get("Location")); err == nil {
				callback.Body.Close()
			}
		}()
		return nil
	}
}

func TestMCPOAuthTokensStayInTheVaultAndRefreshAndRevoke(t *testing.T) {
	vault := keychain.NewMemory()
	app, directory := connectorApp(t, vault)
	fake, server := newFakeOAuthMCP(t)
	ctx := context.Background()
	session, err := app.Service.Create(ctx, "oauth", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	scope := product.SettingsScope{App: true, SessionID: session.ID}
	remote := hub.MCPServer{ID: "remote", Transport: hub.MCPTransportHTTP, URL: server.URL + "/mcp", Auth: hub.MCPAuthOAuth}
	saveApplicationMCP(t, app, scope, remote)
	if _, err = app.Service.Settings.SetEnabled(ctx, scope, hub.CapabilityMCP, "remote", true); err != nil {
		t.Fatal(err)
	}
	if status, err := app.Service.Settings.MCPCredential(ctx, scope, "remote"); err != nil || status.State != product.MCPCredentialNone {
		t.Fatal("fresh server reported an authorization", status, err)
	}
	probe, err := app.Service.Settings.ProbeMCP(ctx, scope, "remote")
	if err != nil || probe.State != product.MCPProbeNeedsAuthorization {
		t.Fatal("unauthorized probe did not ask for authorization", probe, err)
	}
	opened := 0
	browser := followBrowser(t)
	credential, err := app.Service.Settings.AuthorizeMCP(ctx, scope, "remote", func(address string) error { opened++; return browser(address) })
	if err != nil || credential.State != product.MCPCredentialAuthorized || opened != 1 {
		t.Fatal("authorization failed", credential, err)
	}
	probe, err = app.Service.Settings.ProbeMCP(ctx, scope, "remote")
	if err != nil || probe.State != product.MCPProbeConnected || probe.TotalTools != 1 {
		t.Fatal("authorized probe failed", probe, err)
	}
	// Nothing token-shaped may be written to the product data directory.
	err = filepath.WalkDir(directory, func(path string, entry fs.DirEntry, err error) error {
		if err != nil || entry.IsDir() {
			return err
		}
		data, readErr := os.ReadFile(path)
		if readErr == nil && (bytes.Contains(data, []byte("access-token-")) || bytes.Contains(data, []byte("refresh-token-"))) {
			t.Errorf("token bytes found on disk in %s", path)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	// An expired token refreshes from the vault and the rotated grant is saved.
	account := credentialAccount(remote.URL)
	data, err := vault.Get(ctx, account)
	if err != nil {
		t.Fatal(err)
	}
	var grant mcpoauth.Grant
	if err = json.Unmarshal(data, &grant); err != nil {
		t.Fatal(err)
	}
	grant.Token.Expiry = time.Now().Add(-time.Minute)
	expired, _ := json.Marshal(grant)
	if err = vault.Put(ctx, account, expired); err != nil {
		t.Fatal(err)
	}
	if probe, err = app.Service.Settings.ProbeMCP(ctx, scope, "remote"); err != nil || probe.State != product.MCPProbeConnected {
		t.Fatal("expired token was not refreshed", probe, err)
	}
	data, _ = vault.Get(ctx, account)
	var rotated mcpoauth.Grant
	if json.Unmarshal(data, &rotated) != nil || rotated.Token.RefreshToken == grant.Token.RefreshToken || rotated.Token.AccessToken == grant.Token.AccessToken {
		t.Fatal("rotated grant was not persisted")
	}
	credential, err = app.Service.Settings.RevokeMCP(ctx, scope, "remote")
	if err != nil || credential.State != product.MCPCredentialNone {
		t.Fatal("revocation failed", credential, err)
	}
	if ok, _ := vault.Exists(ctx, account); ok || len(fake.revoked) != 2 {
		t.Fatal("revocation kept the local grant or skipped the server", fake.revoked)
	}
	// Without a grant the runtime never opens a browser; it fails closed.
	if probe, err = app.Service.Settings.ProbeMCP(ctx, scope, "remote"); err != nil || probe.State != product.MCPProbeNeedsAuthorization || opened != 1 {
		t.Fatal("missing grant did not fail closed", probe, err)
	}
}

func TestMCPOAuthRefusesWithoutAKeychain(t *testing.T) {
	connector := newMCPConnector(nil)
	connector.open = func() (keychain.Vault, error) { return nil, keychain.ErrUnsupported }
	server := hub.MCPServer{ID: "remote", Transport: hub.MCPTransportHTTP, URL: "https://mcp.example.test/mcp", Auth: hub.MCPAuthOAuth}
	if status := connector.CredentialStatus(context.Background(), server); status.State != product.MCPCredentialUnsupported {
		t.Fatal("missing keychain not reported", status)
	}
	if _, err := connector.Authorize(context.Background(), server, func(string) error { t.Fatal("browser opened without a keychain"); return nil }); err == nil {
		t.Fatal("authorization proceeded without secure storage")
	}
}
