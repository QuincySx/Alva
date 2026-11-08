// INPUT: A local fake MCP resource + OAuth authorization server and a scripted browser
// OUTPUT: Discovery, DCR, PKCE S256, state, iss, resource, redirect, refresh rotation and revocation regressions
// POS: [integration / external protocol] OAuth acceptance on 127.0.0.1 only; never contacts the network
package mcpoauth

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"testing"
	"time"
)

type fakeAuthServer struct {
	mu          sync.Mutex
	base        string
	pkce        bool
	issBroken   bool
	echoWrong   bool
	challenge   string
	redirect    string
	resource    []string
	codes       int
	tokenCalls  int
	revoked     []string
	refreshSeen string
}

func (f *fakeAuthServer) handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/mcp", func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") == "Bearer at-1" || r.Header.Get("Authorization") == "Bearer at-2" {
			w.Header().Set("Content-Type", "application/json")
			w.Write([]byte(`{"jsonrpc":"2.0","id":1,"result":{}}`))
			return
		}
		w.Header().Set("WWW-Authenticate", `Bearer resource_metadata="`+f.base+`/.well-known/oauth-protected-resource/mcp", scope="files:read"`)
		w.WriteHeader(http.StatusUnauthorized)
	})
	mux.HandleFunc("/.well-known/oauth-protected-resource/mcp", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]any{"resource": f.base + "/mcp", "authorization_servers": []string{f.base}})
	})
	mux.HandleFunc("/.well-known/oauth-authorization-server", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		methods := []string{"S256"}
		if !f.pkce {
			methods = []string{"plain"}
		}
		json.NewEncoder(w).Encode(map[string]any{"issuer": f.base, "authorization_endpoint": f.base + "/authorize", "token_endpoint": f.base + "/token", "registration_endpoint": f.base + "/register",
			"revocation_endpoint": f.base + "/revoke", "code_challenge_methods_supported": methods, "scopes_supported": []string{"files:read", "offline_access"}, "authorization_response_iss_parameter_supported": true, "response_types_supported": []string{"code"}})
	})
	mux.HandleFunc("/register", func(w http.ResponseWriter, r *http.Request) {
		var request map[string]any
		json.NewDecoder(r.Body).Decode(&request)
		uris, _ := request["redirect_uris"].([]any)
		if f.echoWrong {
			uris = []any{"http://127.0.0.1:1/elsewhere"}
		}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusCreated)
		json.NewEncoder(w).Encode(map[string]any{"client_id": "client-1", "redirect_uris": uris, "token_endpoint_auth_method": "none"})
	})
	mux.HandleFunc("/authorize", func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query()
		f.mu.Lock()
		f.challenge, f.redirect = q.Get("code_challenge"), q.Get("redirect_uri")
		f.resource = append(f.resource, q.Get("resource"))
		f.codes++
		f.mu.Unlock()
		if q.Get("code_challenge_method") != "S256" || q.Get("response_type") != "code" || q.Get("client_id") != "client-1" {
			http.Error(w, "bad request", http.StatusBadRequest)
			return
		}
		iss := f.base
		if f.issBroken {
			iss = "https://attacker.example.test"
		}
		target := q.Get("redirect_uri") + "?" + url.Values{"code": {"code-1"}, "state": {q.Get("state")}, "iss": {iss}}.Encode()
		http.Redirect(w, r, target, http.StatusFound)
	})
	mux.HandleFunc("/token", func(w http.ResponseWriter, r *http.Request) {
		r.ParseForm()
		f.mu.Lock()
		defer f.mu.Unlock()
		f.tokenCalls++
		f.resource = append(f.resource, r.PostForm.Get("resource"))
		w.Header().Set("Content-Type", "application/json")
		switch r.PostForm.Get("grant_type") {
		case "authorization_code":
			if r.PostForm.Get("code") != "code-1" || r.PostForm.Get("redirect_uri") != f.redirect || Challenge(r.PostForm.Get("code_verifier")) != f.challenge || r.PostForm.Get("client_id") != "client-1" {
				w.WriteHeader(http.StatusBadRequest)
				w.Write([]byte(`{"error":"invalid_grant"}`))
				return
			}
			w.Write([]byte(`{"access_token":"at-1","token_type":"Bearer","expires_in":3600,"refresh_token":"rt-1","scope":"files:read offline_access"}`))
		case "refresh_token":
			f.refreshSeen = r.PostForm.Get("refresh_token")
			if f.refreshSeen != "rt-1" {
				w.WriteHeader(http.StatusBadRequest)
				w.Write([]byte(`{"error":"invalid_grant"}`))
				return
			}
			w.Write([]byte(`{"access_token":"at-2","token_type":"bearer","expires_in":60,"refresh_token":"rt-2"}`))
		default:
			w.WriteHeader(http.StatusBadRequest)
		}
	})
	mux.HandleFunc("/revoke", func(w http.ResponseWriter, r *http.Request) {
		r.ParseForm()
		f.mu.Lock()
		f.revoked = append(f.revoked, r.PostForm.Get("token_type_hint")+":"+r.PostForm.Get("token"))
		f.mu.Unlock()
		w.WriteHeader(http.StatusOK)
	})
	return mux
}

func newFakeAuth(t *testing.T) (*fakeAuthServer, *httptest.Server) {
	t.Helper()
	fake := &fakeAuthServer{pkce: true}
	server := httptest.NewServer(fake.handler())
	t.Cleanup(server.Close)
	fake.base = server.URL
	return fake, server
}

// browser follows the authorization redirect once to the loopback callback,
// as the system browser would after the user consents.
func browser(t *testing.T, tamper func(*url.URL)) func(string) error {
	return func(address string) error {
		go func() {
			client := &http.Client{CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
			response, err := client.Get(address)
			if err != nil {
				t.Error(err)
				return
			}
			response.Body.Close()
			location, err := url.Parse(response.Header.Get("Location"))
			if err != nil {
				t.Error(err)
				return
			}
			if tamper != nil {
				tamper(location)
			}
			if callback, err := http.Get(location.String()); err == nil {
				callback.Body.Close()
			}
		}()
		return nil
	}
}

func TestAuthorizeRunsTheFullFlowOnLoopback(t *testing.T) {
	fake, server := newFakeAuth(t)
	var opened string
	open := browser(t, nil)
	grant, err := Authorize(context.Background(), AuthorizeOptions{ServerURL: server.URL + "/mcp", Open: func(address string) error { opened = address; return open(address) }, Timeout: 5 * time.Second})
	if err != nil {
		t.Fatal(err)
	}
	if grant.Validate() != nil || grant.Token.AccessToken != "at-1" || grant.Token.RefreshToken != "rt-1" || grant.Client.ID != "client-1" || grant.Metadata.Resource != server.URL+"/mcp" {
		t.Fatal("grant incomplete", grant.Token.AccessToken, grant.Client.ID)
	}
	if !strings.HasPrefix(fake.redirect, "http://127.0.0.1:") || !strings.HasSuffix(fake.redirect, "/callback") {
		t.Fatal("redirect URI is not a 127.0.0.1 loopback", fake.redirect)
	}
	query, _ := url.Parse(opened)
	if query.Query().Get("code_challenge_method") != "S256" || len(query.Query().Get("state")) < 43 || !strings.Contains(query.Query().Get("scope"), "offline_access") {
		t.Fatal("authorization URL lacks PKCE, state or offline access", opened)
	}
	for _, resource := range fake.resource {
		if resource != server.URL+"/mcp" {
			t.Fatal("resource indicator missing from a request", fake.resource)
		}
	}
	if strings.Contains(grant.GoString(), "at-1") || strings.Contains(grant.String(), "rt-1") {
		t.Fatal("grant formatting leaks tokens")
	}
	refreshed, err := Refresh(context.Background(), nil, grant)
	if err != nil || refreshed.Token.AccessToken != "at-2" || refreshed.Token.RefreshToken != "rt-2" || fake.refreshSeen != "rt-1" {
		t.Fatal("refresh did not rotate tokens", err)
	}
	if !refreshed.Expired(time.Now().Add(30*time.Second)) || refreshed.Expired(time.Now().Add(-time.Hour)) {
		t.Fatal("expiry margin wrong")
	}
	if err = Revoke(context.Background(), nil, refreshed); err != nil || len(fake.revoked) != 2 || fake.revoked[0] != "refresh_token:rt-2" || fake.revoked[1] != "access_token:at-2" {
		t.Fatal("revocation did not revoke both tokens", err, fake.revoked)
	}
}

func TestAuthorizeRejectsTamperedState(t *testing.T) {
	fake, server := newFakeAuth(t)
	_, err := Authorize(context.Background(), AuthorizeOptions{ServerURL: server.URL + "/mcp", Timeout: 500 * time.Millisecond, Open: browser(t, func(location *url.URL) {
		q := location.Query()
		q.Set("state", strings.Repeat("A", 43))
		location.RawQuery = q.Encode()
	})})
	if !errors.Is(err, context.DeadlineExceeded) || fake.tokenCalls != 0 {
		t.Fatal("callback with a forged state was accepted", err, fake.tokenCalls)
	}
}

func TestAuthorizeRejectsMismatchedIssuer(t *testing.T) {
	fake, server := newFakeAuth(t)
	fake.issBroken = true
	_, err := Authorize(context.Background(), AuthorizeOptions{ServerURL: server.URL + "/mcp", Timeout: 5 * time.Second, Open: browser(t, nil)})
	if err == nil || !strings.Contains(err.Error(), "issuer") || fake.tokenCalls != 0 {
		t.Fatal("mix-up issuer accepted", err)
	}
}

func TestAuthorizeRequiresPKCES256(t *testing.T) {
	fake, server := newFakeAuth(t)
	fake.pkce = false
	if _, err := Authorize(context.Background(), AuthorizeOptions{ServerURL: server.URL + "/mcp", Open: browser(t, nil)}); !errors.Is(err, ErrPKCEUnsupported) || fake.codes != 0 {
		t.Fatal("server without S256 was used", err)
	}
}

func TestRegisterRejectsAForeignRedirect(t *testing.T) {
	fake, server := newFakeAuth(t)
	fake.echoWrong = true
	if _, err := Authorize(context.Background(), AuthorizeOptions{ServerURL: server.URL + "/mcp", Open: browser(t, nil)}); err == nil || !strings.Contains(err.Error(), "redirect") || fake.codes != 0 {
		t.Fatal("registration with a foreign redirect URI was used", err)
	}
}

func TestPKCEVerifierMustMatch(t *testing.T) {
	fake, server := newFakeAuth(t)
	md, err := Discover(context.Background(), nil, server.URL+"/mcp")
	if err != nil {
		t.Fatal(err)
	}
	fake.redirect = "http://127.0.0.1:1/callback"
	fake.challenge = Challenge("the-real-verifier")
	if _, err = Exchange(context.Background(), nil, md, Client{ID: "client-1", AuthMethod: "none"}, fake.redirect, "code-1", "another-verifier"); err == nil {
		t.Fatal("code exchanged with a different verifier")
	}
	if token, err := Exchange(context.Background(), nil, md, Client{ID: "client-1", AuthMethod: "none"}, fake.redirect, "code-1", "the-real-verifier"); err != nil || token.AccessToken != "at-1" {
		t.Fatal("correct verifier rejected", err)
	}
}

func TestReceiverAcceptsOnlyItsExactCallback(t *testing.T) {
	state := strings.Repeat("s", 43)
	receiver, err := Listen(state)
	if err != nil {
		t.Fatal(err)
	}
	defer receiver.Close()
	redirect, _ := url.Parse(receiver.RedirectURI())
	if redirect.Hostname() != "127.0.0.1" {
		t.Fatal("callback not bound to 127.0.0.1", redirect)
	}
	for _, path := range []string{"/other?state=" + state + "&code=x", "/callback?code=x&state=wrong"} {
		response, err := http.Get("http://" + redirect.Host + path)
		if err != nil {
			t.Fatal(err)
		}
		response.Body.Close()
		if response.StatusCode == http.StatusOK {
			t.Fatal("unexpected callback accepted", path)
		}
	}
	// A rebinding name pointing at loopback carries a different Host header.
	request, _ := http.NewRequest(http.MethodGet, receiver.RedirectURI()+"?state="+state+"&code=x", nil)
	request.Host = "attacker.example.test"
	if response, err := http.DefaultClient.Do(request); err != nil || response.StatusCode != http.StatusNotFound {
		t.Fatal("callback accepted a foreign Host header", err)
	}
	response, err := http.Get(receiver.RedirectURI() + "?state=" + state + "&code=good&iss=https://as.example.test")
	if err != nil || response.StatusCode != http.StatusOK || response.Header.Get("Cache-Control") == "" {
		t.Fatal("valid callback rejected", err)
	}
	code, iss, err := receiver.Wait(context.Background())
	if err != nil || code != "good" || iss != "https://as.example.test" {
		t.Fatal("callback result lost", code, iss, err)
	}
}

func TestDiscoverReportsServersWithoutAuthorization(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { w.Write([]byte(`{}`)) }))
	defer server.Close()
	if _, err := Discover(context.Background(), nil, server.URL+"/mcp"); !errors.Is(err, ErrNotRequired) {
		t.Fatal("open server not reported", err)
	}
}

func TestSecureEndpointRequiresTLSExceptLoopback(t *testing.T) {
	for address, ok := range map[string]bool{"https://as.example.test/token": true, "http://127.0.0.1:9/token": true, "http://localhost:9/token": true, "http://as.example.test/token": false, "javascript:alert(1)": false} {
		if (SecureEndpoint(address) == nil) != ok {
			t.Fatal("secure endpoint policy wrong", address)
		}
	}
}
