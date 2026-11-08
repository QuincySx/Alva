// INPUT: net/http, go-sdk oauthex/auth discovery and registration helpers, an explicit MCP endpoint and browser opener
// OUTPUT: Metadata, Client, Token, Grant, Discover, Register, Receiver, AuthorizationURL, Exchange, Refresh, Revoke and Authorize
// POS: [integration / external protocol] MCP authorization (OAuth 2.1 + PKCE S256, RFC 9728/8414/7591/8707/9207/7009); stores nothing, opens nothing itself
package mcpoauth

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode/utf8"

	"github.com/modelcontextprotocol/go-sdk/auth"
	"github.com/modelcontextprotocol/go-sdk/oauthex"

	"github.com/QuincySx/Alva/platform/system/loopback"
)

var (
	ErrNotRequired           = errors.New("MCP server accepted the request without authorization")
	ErrRegistrationRequired  = errors.New("authorization server offers no dynamic client registration; enter a client ID")
	ErrRevocationUnsupported = errors.New("authorization server does not advertise token revocation")
	ErrPKCEUnsupported       = errors.New("authorization server does not support PKCE S256")
)

// Metadata is what discovery established for one MCP resource.
type Metadata struct {
	Resource              string   `json:"resource"`
	Issuer                string   `json:"issuer"`
	AuthorizationEndpoint string   `json:"authorization_endpoint"`
	TokenEndpoint         string   `json:"token_endpoint"`
	RegistrationEndpoint  string   `json:"registration_endpoint,omitempty"`
	RevocationEndpoint    string   `json:"revocation_endpoint,omitempty"`
	Scopes                []string `json:"scopes,omitempty"`
	IssParameterSupported bool     `json:"iss_parameter_supported,omitempty"`
}

// Client is the registered OAuth client. Secret is empty for public clients.
type Client struct {
	ID         string `json:"client_id"`
	Secret     string `json:"client_secret,omitempty"`
	AuthMethod string `json:"auth_method"`
}

type Token struct {
	AccessToken  string    `json:"access_token"`
	RefreshToken string    `json:"refresh_token,omitempty"`
	Expiry       time.Time `json:"expiry,omitzero"`
	Scopes       []string  `json:"scopes,omitempty"`
}

// Grant is everything needed to use, refresh and revoke one authorization.
// It is secret as a whole and belongs in the OS keychain.
type Grant struct {
	Version  int      `json:"version"`
	Metadata Metadata `json:"metadata"`
	Client   Client   `json:"client"`
	Token    Token    `json:"token"`
}

func (g Grant) String() string   { return "[private MCP authorization]" }
func (g Grant) GoString() string { return "[private MCP authorization]" }

func (g Grant) Validate() error {
	if g.Version != 1 || g.Token.AccessToken == "" || g.Client.ID == "" || g.Metadata.TokenEndpoint == "" || g.Metadata.Resource == "" {
		return errors.New("incomplete MCP authorization")
	}
	return nil
}

// Expired reports a token at or within one minute of its expiry.
func (g Grant) Expired(now time.Time) bool {
	return !g.Token.Expiry.IsZero() && !now.Add(time.Minute).Before(g.Token.Expiry)
}

// HTTPClient follows no redirects on token/registration calls and ignores
// environment proxies.
func HTTPClient() *http.Client {
	return &http.Client{Timeout: 30 * time.Second, Transport: &http.Transport{Proxy: nil, TLSHandshakeTimeout: 15 * time.Second},
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
}

func loopbackHost(host string) bool {
	if strings.EqualFold(host, "localhost") {
		return true
	}
	ip := net.ParseIP(strings.Trim(host, "[]"))
	return ip != nil && ip.IsLoopback()
}

// SecureEndpoint accepts https, or http only on a loopback host (MCP
// authorization requires TLS for every authorization server endpoint).
func SecureEndpoint(raw string) error {
	address, err := url.Parse(raw)
	if err != nil || address.Host == "" || address.User != nil || address.Fragment != "" || !utf8.ValidString(raw) || strings.ContainsAny(raw, "\x00\r\n\t ") {
		return errors.New("invalid authorization endpoint")
	}
	if address.Scheme != "https" && !(address.Scheme == "http" && loopbackHost(address.Hostname())) {
		return errors.New("authorization endpoint must use HTTPS")
	}
	return nil
}

func origin(raw string) (string, error) {
	address, err := url.Parse(raw)
	if err != nil || address.Host == "" {
		return "", errors.New("invalid MCP server address")
	}
	return address.Scheme + "://" + address.Host, nil
}

// probe sends one unauthenticated initialize and returns the 401 challenge.
func probe(ctx context.Context, client *http.Client, serverURL string) ([]oauthex.Challenge, error) {
	body := []byte(`{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"alva","version":"go-v1"}}}`)
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, serverURL, bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Accept", "application/json, text/event-stream")
	response, err := client.Do(request)
	if err != nil {
		return nil, errors.New("MCP server is unreachable")
	}
	defer response.Body.Close()
	io.Copy(io.Discard, io.LimitReader(response.Body, 64<<10))
	switch {
	case response.StatusCode >= 200 && response.StatusCode < 300:
		return nil, ErrNotRequired
	case response.StatusCode != http.StatusUnauthorized:
		return nil, fmt.Errorf("MCP server answered HTTP %d instead of asking for authorization", response.StatusCode)
	}
	challenges, err := oauthex.ParseWWWAuthenticate(response.Header.Values("WWW-Authenticate"))
	if err != nil {
		return nil, errors.New("invalid WWW-Authenticate challenge")
	}
	return challenges, nil
}

func challengeParam(challenges []oauthex.Challenge, name string) string {
	for _, challenge := range challenges {
		if challenge.Scheme == "bearer" && challenge.Params[name] != "" {
			return challenge.Params[name]
		}
	}
	for _, challenge := range challenges {
		if challenge.Params[name] != "" {
			return challenge.Params[name]
		}
	}
	return ""
}

// Discover finds the protected-resource and authorization-server metadata.
func Discover(ctx context.Context, client *http.Client, serverURL string) (Metadata, error) {
	if client == nil {
		client = HTTPClient()
	}
	challenges, err := probe(ctx, client, serverURL)
	if err != nil {
		return Metadata{}, err
	}
	root, err := origin(serverURL)
	if err != nil {
		return Metadata{}, err
	}
	address, _ := url.Parse(serverURL)
	type candidate struct{ metadata, resource string }
	var candidates []candidate
	if hinted := challengeParam(challenges, "resource_metadata"); hinted != "" {
		candidates = append(candidates, candidate{hinted, serverURL})
	}
	if path := strings.TrimLeft(address.Path, "/"); path != "" {
		candidates = append(candidates, candidate{root + "/.well-known/oauth-protected-resource/" + path, serverURL})
	}
	candidates = append(candidates, candidate{root + "/.well-known/oauth-protected-resource", root})
	md := Metadata{Resource: serverURL, Issuer: root}
	var scopes []string
	for _, item := range candidates {
		prm, prmErr := oauthex.GetProtectedResourceMetadata(ctx, item.metadata, item.resource, client)
		if prmErr != nil || prm == nil {
			continue
		}
		if len(prm.AuthorizationServers) == 0 {
			return Metadata{}, errors.New("protected resource metadata names no authorization server")
		}
		md.Resource, md.Issuer, scopes = prm.Resource, prm.AuthorizationServers[0], prm.ScopesSupported
		break
	}
	if scope := challengeParam(challenges, "scope"); scope != "" {
		scopes = strings.Fields(scope)
	}
	server, err := auth.GetAuthServerMetadata(ctx, md.Issuer, client)
	if err != nil {
		return Metadata{}, errors.New("authorization server metadata is invalid: " + truncate(err.Error(), 200))
	}
	if server == nil {
		return Metadata{}, errors.New("authorization server publishes no metadata; PKCE support cannot be verified")
	}
	if !slices.Contains(server.CodeChallengeMethodsSupported, "S256") {
		return Metadata{}, ErrPKCEUnsupported
	}
	md.Issuer = server.Issuer
	md.AuthorizationEndpoint, md.TokenEndpoint = server.AuthorizationEndpoint, server.TokenEndpoint
	md.RegistrationEndpoint, md.RevocationEndpoint = server.RegistrationEndpoint, server.RevocationEndpoint
	md.IssParameterSupported = server.AuthorizationResponseIssParameterSupported
	for _, endpoint := range []string{md.AuthorizationEndpoint, md.TokenEndpoint} {
		if err := SecureEndpoint(endpoint); err != nil {
			return Metadata{}, err
		}
	}
	for _, endpoint := range []string{md.RegistrationEndpoint, md.RevocationEndpoint} {
		if endpoint != "" {
			if err := SecureEndpoint(endpoint); err != nil {
				return Metadata{}, err
			}
		}
	}
	if slices.Contains(server.ScopesSupported, "offline_access") && !slices.Contains(scopes, "offline_access") {
		scopes = append(scopes, "offline_access")
	}
	if len(scopes) > 64 {
		return Metadata{}, errors.New("authorization requests too many scopes")
	}
	md.Scopes = scopes
	return md, nil
}

// Register returns the preregistered public client, or registers one
// dynamically with exactly this loopback redirect URI.
func Register(ctx context.Context, client *http.Client, md Metadata, redirectURI, clientID string) (Client, error) {
	if clientID != "" {
		return Client{ID: clientID, AuthMethod: "none"}, nil
	}
	if md.RegistrationEndpoint == "" {
		return Client{}, ErrRegistrationRequired
	}
	if client == nil {
		client = HTTPClient()
	}
	response, err := oauthex.RegisterClient(ctx, md.RegistrationEndpoint, &oauthex.ClientRegistrationMetadata{RedirectURIs: []string{redirectURI}, TokenEndpointAuthMethod: "none",
		GrantTypes: []string{"authorization_code", "refresh_token"}, ResponseTypes: []string{"code"}, ClientName: "Alva", ApplicationType: "native"}, client)
	if err != nil {
		return Client{}, errors.New("dynamic client registration failed: " + truncate(err.Error(), 200))
	}
	if len(response.RedirectURIs) != 0 && !slices.Contains(response.RedirectURIs, redirectURI) {
		return Client{}, errors.New("registered client does not allow this loopback redirect URI")
	}
	method := response.TokenEndpointAuthMethod
	switch method {
	case "", "none":
		method = "none"
		if response.ClientSecret != "" {
			method = "client_secret_basic"
		}
	case "client_secret_basic", "client_secret_post":
	default:
		return Client{}, errors.New("unsupported token endpoint authentication method")
	}
	return Client{ID: response.ClientID, Secret: response.ClientSecret, AuthMethod: method}, nil
}

func randomText() (string, error) {
	var data [32]byte
	if _, err := rand.Read(data[:]); err != nil {
		return "", err
	}
	return base64.RawURLEncoding.EncodeToString(data[:]), nil
}

// Challenge derives the S256 code challenge of a verifier.
func Challenge(verifier string) string {
	sum := sha256.Sum256([]byte(verifier))
	return base64.RawURLEncoding.EncodeToString(sum[:])
}

// AuthorizationURL builds the browser URL with PKCE S256, state and resource.
func AuthorizationURL(md Metadata, client Client, redirectURI, state, verifier string) (string, error) {
	if err := SecureEndpoint(md.AuthorizationEndpoint); err != nil {
		return "", err
	}
	address, err := url.Parse(md.AuthorizationEndpoint)
	if err != nil {
		return "", err
	}
	query := address.Query()
	query.Set("response_type", "code")
	query.Set("client_id", client.ID)
	query.Set("redirect_uri", redirectURI)
	query.Set("state", state)
	query.Set("code_challenge", Challenge(verifier))
	query.Set("code_challenge_method", "S256")
	query.Set("resource", md.Resource)
	if len(md.Scopes) > 0 {
		query.Set("scope", strings.Join(md.Scopes, " "))
	}
	address.RawQuery = query.Encode()
	return address.String(), nil
}

type callback struct {
	code, iss string
	err       error
}

// Receiver is the single-use loopback redirect endpoint (127.0.0.1 only).
type Receiver struct {
	listener net.Listener
	server   *http.Server
	host     string
	state    string
	results  chan callback
	once     sync.Once
	rejected atomic.Int32
}

// Listen binds 127.0.0.1 on an ephemeral port and expects exactly state.
func Listen(state string) (*Receiver, error) {
	if len(state) < 32 {
		return nil, errors.New("authorization state too short")
	}
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		return nil, err
	}
	r := &Receiver{listener: listener, host: listener.Addr().String(), state: state, results: make(chan callback, 1)}
	r.server = &http.Server{Handler: r, ReadHeaderTimeout: 10 * time.Second, MaxHeaderBytes: 16 << 10}
	go func() { _ = r.server.Serve(listener) }()
	return r, nil
}

func (r *Receiver) RedirectURI() string { return "http://" + r.host + "/callback" }

func (r *Receiver) deliver(value callback) { r.once.Do(func() { r.results <- value }) }

func page(w http.ResponseWriter, status int, message string) {
	loopback.SecureHeaders(w)
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Header().Set("Content-Security-Policy", "default-src 'none'")
	w.WriteHeader(status)
	fmt.Fprintf(w, "<!doctype html><meta charset=utf-8><title>Alva</title><p>%s</p>", message)
}

func (r *Receiver) ServeHTTP(w http.ResponseWriter, request *http.Request) {
	if request.Method != http.MethodGet || request.URL.Path != "/callback" || request.Host != r.host {
		page(w, http.StatusNotFound, "Not found")
		return
	}
	query := request.URL.Query()
	if subtle.ConstantTimeCompare([]byte(query.Get("state")), []byte(r.state)) != 1 {
		if r.rejected.Add(1) > 8 {
			r.deliver(callback{err: errors.New("too many authorization callbacks with a wrong state")})
		}
		page(w, http.StatusBadRequest, "授权状态不匹配，请回到 Alva 重新开始。")
		return
	}
	if code := query.Get("error"); code != "" {
		r.deliver(callback{err: errors.New("authorization server refused: " + truncate(sanitize(code), 64))})
		page(w, http.StatusOK, "授权没有完成，可以回到 Alva。")
		return
	}
	code := query.Get("code")
	if code == "" || len(code) > 4096 {
		page(w, http.StatusBadRequest, "授权回调缺少授权码。")
		return
	}
	r.deliver(callback{code: code, iss: query.Get("iss")})
	page(w, http.StatusOK, "授权完成，可以回到 Alva。")
}

// Wait returns the code and iss of the first valid callback.
func (r *Receiver) Wait(ctx context.Context) (string, string, error) {
	select {
	case value := <-r.results:
		return value.code, value.iss, value.err
	case <-ctx.Done():
		return "", "", ctx.Err()
	}
}

func (r *Receiver) Close() error {
	shutdown, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	return r.server.Shutdown(shutdown)
}

// TokenError is an OAuth error response from the token endpoint.
type TokenError struct{ Code string }

func (e *TokenError) Error() string { return "token endpoint refused: " + e.Code }

func tokenRequest(ctx context.Context, client *http.Client, endpoint string, c Client, form url.Values) (Token, error) {
	if err := SecureEndpoint(endpoint); err != nil {
		return Token{}, err
	}
	if client == nil {
		client = HTTPClient()
	}
	request, err := clientRequest(ctx, endpoint, c, form)
	if err != nil {
		return Token{}, err
	}
	response, err := client.Do(request)
	if err != nil {
		return Token{}, errors.New("token endpoint is unreachable")
	}
	defer response.Body.Close()
	data, err := io.ReadAll(io.LimitReader(response.Body, 64<<10+1))
	if err != nil || len(data) > 64<<10 {
		return Token{}, errors.New("token response is invalid or too large")
	}
	if response.StatusCode != http.StatusOK {
		var failure struct {
			Error string `json:"error"`
		}
		if json.Unmarshal(data, &failure) == nil && failure.Error != "" {
			return Token{}, &TokenError{Code: truncate(sanitize(failure.Error), 64)}
		}
		return Token{}, fmt.Errorf("token endpoint returned HTTP %d", response.StatusCode)
	}
	var value struct {
		AccessToken  string          `json:"access_token"`
		TokenType    string          `json:"token_type"`
		ExpiresIn    json.RawMessage `json:"expires_in"`
		RefreshToken string          `json:"refresh_token"`
		Scope        string          `json:"scope"`
	}
	if err = json.Unmarshal(data, &value); err != nil || value.AccessToken == "" || len(value.AccessToken) > 8192 || len(value.RefreshToken) > 8192 || !strings.EqualFold(value.TokenType, "bearer") {
		return Token{}, errors.New("token response lacks a bearer access token")
	}
	if strings.ContainsAny(value.AccessToken+value.RefreshToken, "\x00\r\n ") {
		return Token{}, errors.New("token response carries invalid characters")
	}
	token := Token{AccessToken: value.AccessToken, RefreshToken: value.RefreshToken, Scopes: strings.Fields(value.Scope)}
	if len(value.ExpiresIn) > 0 && string(value.ExpiresIn) != "null" {
		seconds, err := strconv.ParseInt(strings.Trim(string(value.ExpiresIn), `"`), 10, 64)
		if err != nil || seconds < 0 || seconds > 10*365*24*3600 {
			return Token{}, errors.New("token response has an invalid lifetime")
		}
		if seconds > 0 {
			token.Expiry = time.Now().Add(time.Duration(seconds) * time.Second).UTC()
		}
	}
	return token, nil
}

func clientRequest(ctx context.Context, endpoint string, c Client, form url.Values) (*http.Request, error) {
	switch c.AuthMethod {
	case "client_secret_post":
		form.Set("client_id", c.ID)
		form.Set("client_secret", c.Secret)
	case "client_secret_basic":
	default:
		form.Set("client_id", c.ID)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, strings.NewReader(form.Encode()))
	if err != nil {
		return nil, err
	}
	request.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	request.Header.Set("Accept", "application/json")
	if c.AuthMethod == "client_secret_basic" {
		request.SetBasicAuth(url.QueryEscape(c.ID), url.QueryEscape(c.Secret))
	}
	return request, nil
}

// Exchange redeems the authorization code with the PKCE verifier.
func Exchange(ctx context.Context, client *http.Client, md Metadata, c Client, redirectURI, code, verifier string) (Token, error) {
	return tokenRequest(ctx, client, md.TokenEndpoint, c, url.Values{"grant_type": {"authorization_code"}, "code": {code}, "redirect_uri": {redirectURI}, "code_verifier": {verifier}, "resource": {md.Resource}})
}

// Refresh returns the grant with a new access token; a rotated refresh token
// replaces the old one, otherwise the old one is kept.
func Refresh(ctx context.Context, client *http.Client, grant Grant) (Grant, error) {
	if grant.Token.RefreshToken == "" {
		return Grant{}, errors.New("authorization has no refresh token")
	}
	token, err := tokenRequest(ctx, client, grant.Metadata.TokenEndpoint, grant.Client, url.Values{"grant_type": {"refresh_token"}, "refresh_token": {grant.Token.RefreshToken}, "resource": {grant.Metadata.Resource}})
	if err != nil {
		return Grant{}, err
	}
	if token.RefreshToken == "" {
		token.RefreshToken = grant.Token.RefreshToken
	}
	if len(token.Scopes) == 0 {
		token.Scopes = grant.Token.Scopes
	}
	grant.Token = token
	return grant, nil
}

// Revoke revokes the refresh token, then the access token (RFC 7009).
func Revoke(ctx context.Context, client *http.Client, grant Grant) error {
	if grant.Metadata.RevocationEndpoint == "" {
		return ErrRevocationUnsupported
	}
	if err := SecureEndpoint(grant.Metadata.RevocationEndpoint); err != nil {
		return err
	}
	if client == nil {
		client = HTTPClient()
	}
	var result error
	for _, token := range []struct{ value, hint string }{{grant.Token.RefreshToken, "refresh_token"}, {grant.Token.AccessToken, "access_token"}} {
		if token.value == "" {
			continue
		}
		request, err := clientRequest(ctx, grant.Metadata.RevocationEndpoint, grant.Client, url.Values{"token": {token.value}, "token_type_hint": {token.hint}})
		if err != nil {
			return err
		}
		response, err := client.Do(request)
		if err != nil {
			result = errors.Join(result, errors.New("revocation endpoint is unreachable"))
			continue
		}
		io.Copy(io.Discard, io.LimitReader(response.Body, 64<<10))
		response.Body.Close()
		if response.StatusCode != http.StatusOK {
			result = errors.Join(result, fmt.Errorf("revocation endpoint returned HTTP %d", response.StatusCode))
		}
	}
	return result
}

// AuthorizeOptions configure one interactive authorization.
type AuthorizeOptions struct {
	ServerURL string
	ClientID  string
	HTTP      *http.Client
	Open      func(string) error
	Timeout   time.Duration
}

// Authorize runs discovery, registration, the browser step on a loopback
// redirect, iss/state checks and the code exchange.
func Authorize(ctx context.Context, options AuthorizeOptions) (Grant, error) {
	if options.Open == nil {
		return Grant{}, errors.New("authorization requires a browser opener")
	}
	if options.Timeout == 0 {
		options.Timeout = 5 * time.Minute
	}
	md, err := Discover(ctx, options.HTTP, options.ServerURL)
	if err != nil {
		return Grant{}, err
	}
	state, err := randomText()
	if err != nil {
		return Grant{}, err
	}
	verifier, err := randomText()
	if err != nil {
		return Grant{}, err
	}
	receiver, err := Listen(state)
	if err != nil {
		return Grant{}, err
	}
	defer receiver.Close()
	client, err := Register(ctx, options.HTTP, md, receiver.RedirectURI(), options.ClientID)
	if err != nil {
		return Grant{}, err
	}
	address, err := AuthorizationURL(md, client, receiver.RedirectURI(), state, verifier)
	if err != nil {
		return Grant{}, err
	}
	if err = options.Open(address); err != nil {
		return Grant{}, errors.New("could not open the system browser")
	}
	wait, cancel := context.WithTimeout(ctx, options.Timeout)
	defer cancel()
	code, iss, err := receiver.Wait(wait)
	if err != nil {
		return Grant{}, err
	}
	if md.IssParameterSupported && iss == "" || iss != "" && iss != md.Issuer {
		return Grant{}, errors.New("authorization response issuer does not match the authorization server")
	}
	token, err := Exchange(ctx, options.HTTP, md, client, receiver.RedirectURI(), code, verifier)
	if err != nil {
		return Grant{}, err
	}
	return Grant{Version: 1, Metadata: md, Client: client, Token: token}, nil
}

func sanitize(value string) string {
	return strings.Map(func(r rune) rune {
		if r < 0x20 || r == 0x7f || r == '<' || r == '>' || r == '&' {
			return -1
		}
		return r
	}, value)
}

func truncate(value string, max int) string {
	if len(value) <= max {
		return value
	}
	cut := max
	for cut > 0 && !utf8.RuneStart(value[cut]) {
		cut--
	}
	return value[:cut]
}
