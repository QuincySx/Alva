// INPUT: Product MCP server snapshots, protocol/mcp transports, mcpoauth flows and a keychain Vault
// OUTPUT: mcpConnector (product.MCPConnector), per-server protocol configs and the non-interactive runtime Authorizer
// POS: [L7 Hub assembly] The only place that joins MCP transport, OAuth and secret storage; tokens never reach product or disk
package bootstrap

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	mcpprotocol "github.com/QuincySx/Alva/integration/protocol/mcp"
	mcpoauth "github.com/QuincySx/Alva/integration/protocol/mcp/oauth"
	"github.com/QuincySx/Alva/platform/storage/redaction"
	"github.com/QuincySx/Alva/platform/system/keychain"
	ownedprocess "github.com/QuincySx/Alva/platform/system/process"
)

// mcpKeychainService names the keychain items holding MCP authorizations.
const mcpKeychainService = "dev.alva.mcp-oauth"

type mcpConnector struct {
	once     sync.Once
	vault    keychain.Vault
	vaultErr error
	open     func() (keychain.Vault, error)
	http     *http.Client
	mu       sync.Mutex
	locks    map[string]*sync.Mutex
}

func newMCPConnector(vault keychain.Vault) *mcpConnector {
	connector := &mcpConnector{locks: map[string]*sync.Mutex{}}
	connector.open = func() (keychain.Vault, error) {
		if vault != nil {
			return vault, nil
		}
		return keychain.System(mcpKeychainService)
	}
	return connector
}

// keychain opens the vault on first OAuth use only.
func (m *mcpConnector) keychain() (keychain.Vault, error) {
	m.once.Do(func() { m.vault, m.vaultErr = m.open() })
	return m.vault, m.vaultErr
}

func (m *mcpConnector) lock(account string) *sync.Mutex {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.locks[account] == nil {
		m.locks[account] = &sync.Mutex{}
	}
	return m.locks[account]
}

// credentialAccount derives a stable keychain account from the canonical
// resource URL; the same server shares one authorization across sessions.
func credentialAccount(serverURL string) string {
	address, err := url.Parse(serverURL)
	if err == nil {
		address.Scheme, address.Host, address.Fragment = strings.ToLower(address.Scheme), strings.ToLower(address.Host), ""
		serverURL = address.String()
	}
	sum := sha256.Sum256([]byte(serverURL))
	return "mcp-oauth:" + hex.EncodeToString(sum[:16])
}

// protocolConfig turns one approved product snapshot into a transport config.
func (m *mcpConnector) protocolConfig(server hub.MCPServer, workspace string) mcpprotocol.Config {
	if server.Remote() {
		config := mcpprotocol.Config{ID: server.ID, HTTP: &mcpprotocol.HTTPConfig{URL: server.URL, Headers: server.Headers, Client: m.http}}
		if server.Auth == hub.MCPAuthOAuth {
			config.HTTP.Auth = &mcpAuthorizer{connector: m, url: server.URL}
		}
		return config
	}
	return mcpprotocol.Config{ID: server.ID, Process: ownedprocess.Config{Command: server.Command, Args: server.Args, Env: server.Env, Cwd: workspace}}
}

// stderrTail keeps the last bytes of a server's stderr for the probe report.
type stderrTail struct {
	mu   sync.Mutex
	text []byte
}

func (s *stderrTail) Record(_ context.Context, event mcpprotocol.IOEvent) (string, error) {
	if event.Direction != "stderr" {
		return "", nil
	}
	var line struct {
		Text string `json:"text"`
	}
	if json.Unmarshal(event.Frame, &line) == nil && line.Text != "" {
		s.mu.Lock()
		s.text = append(append(s.text, line.Text...), '\n')
		if len(s.text) > 4<<10 {
			s.text = s.text[len(s.text)-4<<10:]
		}
		s.mu.Unlock()
	}
	return "", nil
}

func secretsOf(server hub.MCPServer) []string {
	return append(redaction.EnvironmentSecrets(server.Env), redaction.EnvironmentSecrets(server.Headers)...)
}

func redactedText(text string, server hub.MCPServer) string {
	redactor, err := redaction.NewRedactor(secretsOf(server)...)
	if err == nil {
		text = redactor.Text(text)
	}
	return strings.ToValidUTF8(text, "")
}

func shorten(text string, max int) string {
	text = strings.TrimSpace(text)
	if utf8.RuneCountInString(text) <= max {
		return text
	}
	runes := []rune(text)
	return string(runes[:max]) + "…"
}

// Probe starts the server once, lists its tools and closes it.
func (m *mcpConnector) Probe(ctx context.Context, workspace string, server hub.MCPServer) product.MCPProbe {
	tail := &stderrTail{}
	result := product.MCPProbe{State: product.MCPProbeFailed, CheckedAt: time.Now().UnixMilli()}
	config := m.protocolConfig(server, workspace)
	config.ConnectTimeout, config.RequestTimeout = 30*time.Second, 30*time.Second
	transport, err := mcpprotocol.Connect(ctx, config, tail)
	var tools []mcpprotocol.Tool
	if err == nil {
		tools, err = transport.ListTools(ctx)
		cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		err = errors.Join(err, transport.Close(cleanup))
		cancel()
	}
	if err != nil {
		switch {
		case errors.Is(err, mcpprotocol.ErrAuthorizationRequired):
			result.State, result.Problem = product.MCPProbeNeedsAuthorization, "服务器要求授权"
			if server.Auth == hub.MCPAuthOAuth {
				result.Problem = "请在下方完成 OAuth 授权，或重新授权"
			} else {
				result.Problem += "；可以把认证方式改为 OAuth，或在请求头里填写凭据"
			}
		case errors.Is(err, mcpprotocol.ErrLegacyTransport):
			result.Problem = "服务器只支持已弃用的 HTTP+SSE 传输，暂不支持"
		default:
			result.Problem = shorten(redactedText(err.Error(), server), 300)
		}
		tail.mu.Lock()
		if len(tail.text) > 0 {
			result.Problem += "\n" + shorten(redactedText(string(tail.text), server), 600)
		}
		tail.mu.Unlock()
		return result
	}
	result.State, result.TotalTools = product.MCPProbeConnected, len(tools)
	for _, tool := range tools {
		if len(result.Tools) == product.MCPProbeToolLimit {
			break
		}
		result.Tools = append(result.Tools, product.MCPToolSummary{Name: tool.Name, Description: shorten(tool.Description, 200)})
	}
	return result
}

func (m *mcpConnector) CredentialStatus(ctx context.Context, server hub.MCPServer) product.MCPCredential {
	vault, err := m.keychain()
	if err != nil {
		return product.MCPCredential{State: product.MCPCredentialUnsupported, Problem: "系统钥匙串不可用"}
	}
	exists, err := vault.Exists(ctx, credentialAccount(server.URL))
	switch {
	case err != nil:
		return product.MCPCredential{State: product.MCPCredentialNone, Problem: "无法读取钥匙串"}
	case exists:
		return product.MCPCredential{State: product.MCPCredentialAuthorized}
	}
	return product.MCPCredential{State: product.MCPCredentialNone}
}

func credentialOf(grant mcpoauth.Grant) product.MCPCredential {
	credential := product.MCPCredential{State: product.MCPCredentialAuthorized, Scopes: grant.Token.Scopes}
	if issuer, err := url.Parse(grant.Metadata.Issuer); err == nil {
		credential.Issuer = issuer.Host
	}
	if !grant.Token.Expiry.IsZero() {
		credential.ExpiresAt = grant.Token.Expiry.UnixMilli()
	}
	return credential
}

func oauthProblem(err error) string {
	switch {
	case errors.Is(err, context.DeadlineExceeded):
		return "等待浏览器授权超时，请重试"
	case errors.Is(err, context.Canceled):
		return "授权已取消"
	case errors.Is(err, mcpoauth.ErrNotRequired):
		return "这个服务器不需要授权，可以把认证方式改为“无”"
	case errors.Is(err, mcpoauth.ErrRegistrationRequired):
		return "服务器不支持自动注册，请在编辑页填写 Client ID"
	case errors.Is(err, mcpoauth.ErrPKCEUnsupported):
		return "授权服务器不支持 PKCE（S256），为安全起见已停止"
	}
	return "授权失败：" + shorten(err.Error(), 200)
}

func (m *mcpConnector) Authorize(ctx context.Context, server hub.MCPServer, open func(string) error) (product.MCPCredential, error) {
	vault, err := m.keychain()
	if err != nil {
		return product.MCPCredential{State: product.MCPCredentialUnsupported}, errors.New("此平台无法安全保存令牌，OAuth 不可用")
	}
	grant, err := mcpoauth.Authorize(ctx, mcpoauth.AuthorizeOptions{ServerURL: server.URL, ClientID: server.OAuthClientID, HTTP: m.http, Open: func(address string) error {
		if err := mcpoauth.SecureEndpoint(address); err != nil {
			return err // Only an https (or loopback http) page reaches the system browser.
		}
		return open(address)
	}})
	if err != nil {
		return m.CredentialStatus(context.WithoutCancel(ctx), server), errors.New(oauthProblem(err))
	}
	if err = m.store(ctx, vault, server.URL, grant); err != nil {
		return product.MCPCredential{State: product.MCPCredentialNone}, err
	}
	return credentialOf(grant), nil
}

func (m *mcpConnector) store(ctx context.Context, vault keychain.Vault, serverURL string, grant mcpoauth.Grant) error {
	data, err := json.Marshal(grant)
	if err != nil || len(data) > keychain.MaxSecret {
		return errors.New("授权数据过大，无法保存")
	}
	if err = vault.Put(ctx, credentialAccount(serverURL), data); err != nil {
		return errors.New("无法写入系统钥匙串：" + shorten(err.Error(), 120))
	}
	return nil
}

func (m *mcpConnector) load(ctx context.Context, vault keychain.Vault, serverURL string) (mcpoauth.Grant, error) {
	data, err := vault.Get(ctx, credentialAccount(serverURL))
	if err != nil {
		return mcpoauth.Grant{}, err
	}
	var grant mcpoauth.Grant
	if json.Unmarshal(data, &grant) != nil || grant.Validate() != nil {
		return mcpoauth.Grant{}, errors.New("saved MCP authorization is corrupt")
	}
	return grant, nil
}

func (m *mcpConnector) Revoke(ctx context.Context, server hub.MCPServer) (product.MCPCredential, error) {
	vault, err := m.keychain()
	if err != nil {
		return product.MCPCredential{State: product.MCPCredentialUnsupported}, errors.New("系统钥匙串不可用")
	}
	account := credentialAccount(server.URL)
	lock := m.lock(account)
	lock.Lock()
	defer lock.Unlock()
	grant, err := m.load(ctx, vault, server.URL)
	var remote error
	switch {
	case errors.Is(err, keychain.ErrNotFound):
		return product.MCPCredential{State: product.MCPCredentialNone}, nil
	case err == nil:
		remote = mcpoauth.Revoke(ctx, m.http, grant)
	}
	// The local copy is always removed, even when the server cannot revoke.
	if err := vault.Delete(context.WithoutCancel(ctx), account); err != nil {
		return product.MCPCredential{State: product.MCPCredentialAuthorized}, errors.New("无法从钥匙串删除授权")
	}
	credential := product.MCPCredential{State: product.MCPCredentialNone}
	switch {
	case errors.Is(remote, mcpoauth.ErrRevocationUnsupported):
		credential.Problem = "服务器不支持撤销，已删除本地令牌"
	case remote != nil:
		credential.Problem = "已删除本地令牌，但服务器撤销失败：" + shorten(remote.Error(), 120)
	}
	return credential, nil
}

// mcpAuthorizer serves one connection. It refreshes from the keychain at most
// once per challenge and never starts an interactive login.
type mcpAuthorizer struct {
	connector *mcpConnector
	url       string
	mu        sync.Mutex
	grant     *mcpoauth.Grant
	refreshed bool
}

// errReauthorize is an authorization requirement, so probes and runs report
// "needs authorization" instead of a generic failure.
var errReauthorize = fmt.Errorf("%w: the saved authorization is missing or expired; authorize again in Settings", mcpprotocol.ErrAuthorizationRequired)

func (a *mcpAuthorizer) current(ctx context.Context, force bool) (mcpoauth.Grant, error) {
	vault, err := a.connector.keychain()
	if err != nil {
		return mcpoauth.Grant{}, errReauthorize
	}
	account := credentialAccount(a.url)
	lock := a.connector.lock(account)
	lock.Lock()
	defer lock.Unlock()
	// Another connection may have rotated the refresh token meanwhile.
	grant, err := a.connector.load(ctx, vault, a.url)
	if err != nil {
		return mcpoauth.Grant{}, errReauthorize
	}
	if a.grant != nil && grant.Token.AccessToken != a.grant.Token.AccessToken {
		force = false
	}
	if force || grant.Expired(time.Now()) {
		refreshed, err := mcpoauth.Refresh(ctx, a.connector.http, grant)
		if err != nil {
			return mcpoauth.Grant{}, errors.Join(errReauthorize, err)
		}
		if err = a.connector.store(ctx, vault, a.url, refreshed); err != nil {
			return mcpoauth.Grant{}, err
		}
		grant = refreshed
	}
	return grant, nil
}

func (a *mcpAuthorizer) Header(ctx context.Context) (string, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.grant == nil || a.grant.Expired(time.Now()) {
		grant, err := a.current(ctx, false)
		if err != nil {
			return "", err
		}
		a.grant = &grant
	}
	return "Bearer " + a.grant.Token.AccessToken, nil
}

func (a *mcpAuthorizer) Unauthorized(ctx context.Context, status int, _ []string) error {
	a.mu.Lock()
	defer a.mu.Unlock()
	if status != http.StatusUnauthorized || a.refreshed {
		return errReauthorize
	}
	a.refreshed = true
	grant, err := a.current(ctx, true)
	if err != nil {
		return err
	}
	a.grant = &grant
	return nil
}
