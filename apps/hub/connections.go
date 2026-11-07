// INPUT: Explicit connection edits, exact model identities and standard value validation.
// OUTPUT: Redacted connection values, optimistic CRUD, optional request-bound creation receipts and revision-bound routes.
// POS: [L7 Hub product] Shared connection policy without provider, credential discovery or execution dependencies.
package hub

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"slices"
	"sort"
	"strings"
	"unicode/utf8"
)

var ErrConnectionConflict = errors.New("model connection changed; reload before saving")
var ErrConnectionCreationConflict = errors.New("connection creation changed or its connection was edited or deleted; resolve the original request")

// ModelConnection is safe for lists and editors: credential/header values are never returned.
type ModelConnection struct {
	ID          string            `json:"id"`
	Revision    uint64            `json:"revision"`
	Name        string            `json:"name"`
	Protocol    string            `json:"protocol"`
	Endpoint    string            `json:"endpoint"`
	CatalogURL  string            `json:"catalog_url,omitempty"`
	Auth        string            `json:"auth"`
	Enabled     bool              `json:"enabled"`
	HasAPIKey   bool              `json:"has_api_key"`
	HeaderNames []string          `json:"header_names,omitempty"`
	Models      []ConnectionModel `json:"models"`
	CreatedAt   int64             `json:"created_at"`
	UpdatedAt   int64             `json:"updated_at"`
}

type ConnectionModel struct {
	ID      string `json:"id"`
	Name    string `json:"name"`
	Enabled bool   `json:"enabled"`
	// ImageCapable is an explicit operator declaration, never inferred from a model name.
	ImageCapable bool `json:"image_capable"`
}

type ConnectionUpdate struct {
	ID               string            `json:"id"`
	ExpectedRevision uint64            `json:"expected_revision"`
	Name             string            `json:"name"`
	Protocol         string            `json:"protocol"`
	Endpoint         string            `json:"endpoint"`
	CatalogURL       string            `json:"catalog_url,omitempty"`
	Auth             string            `json:"auth"`
	Enabled          bool              `json:"enabled"`
	Models           []ConnectionModel `json:"models"`
	// Empty APIKey preserves the saved value. ClearAPIKey explicitly removes it.
	APIKey         string            `json:"-"`
	ClearAPIKey    bool              `json:"clear_api_key"`
	Headers        map[string]string `json:"-"`
	ReplaceHeaders bool              `json:"replace_headers"`
}

// ConnectionSecrets is an explicit assembly-only capability; never a list/editor DTO.
// The selected provider owns protection at rest; this contract promises no encryption.
type ConnectionSecrets struct {
	APIKey  string            `json:"-"`
	Headers map[string]string `json:"-"`
}

func (ConnectionSecrets) String() string   { return "[private model credentials]" }
func (ConnectionSecrets) GoString() string { return "[private model credentials]" }
func (ConnectionUpdate) String() string    { return "[private model connection edit]" }
func (ConnectionUpdate) GoString() string  { return "[private model connection edit]" }

type ConnectionsStore interface {
	ModelPreferencesStore
	Connections(context.Context) ([]ModelConnection, error)
	SaveConnection(context.Context, ConnectionUpdate) (ModelConnection, error)
	DeleteConnection(context.Context, string, uint64) error
	ConnectionSecrets(context.Context, string, uint64) (ConnectionSecrets, error)
}

// ConnectionCreate freezes one explicit creation identity. Its secrets retain
// ConnectionUpdate's private serialization and are never receipt metadata.
type ConnectionCreate struct {
	RequestID string           `json:"request_id"`
	Update    ConnectionUpdate `json:"update"`
}

func (r ConnectionCreate) Validate() error {
	if !connectionText(r.RequestID, 128) || r.Update.ID != "" || r.Update.ExpectedRevision != 0 || r.Update.ClearAPIKey ||
		r.Update.Auth == "none" && r.Update.APIKey != "" || !r.Update.ReplaceHeaders && len(r.Update.Headers) != 0 {
		return errors.New("connection creation requires an explicit request identity and new configuration without ignored credential edits")
	}
	check := r.Update
	check.ID = "connection-validation"
	_, _, err := (ModelConnection{ID: check.ID}).Apply(check, ConnectionSecrets{})
	return err
}

// Created is the immutable redacted save receipt. Current is nil after explicit
// deletion; an edited connection retains its identity and newer revision.
type ConnectionCreationReceipt struct {
	RequestID string           `json:"request_id"`
	Created   ModelConnection  `json:"created"`
	Current   *ModelConnection `json:"current,omitempty"`
}

func (r ConnectionCreationReceipt) Validate() error {
	if !connectionText(r.RequestID, 128) || r.Created.Revision != 1 || r.Created.Validate() != nil ||
		r.Current != nil && (r.Current.ID != r.Created.ID || r.Current.Revision < r.Created.Revision || r.Current.Validate() != nil ||
			r.Current.Revision == r.Created.Revision && !r.Current.SameConfiguration(r.Created)) {
		return errors.New("invalid connection creation receipt")
	}
	return nil
}

func (r ConnectionCreationReceipt) Clone() ConnectionCreationReceipt {
	r.Created = r.Created.Clone()
	if r.Current != nil {
		current := r.Current.Clone()
		r.Current = &current
	}
	return r
}

// ConnectionCreationStore is an optional configuration capability, independent
// of execution. Create atomically commits a RequestID and the actual connection;
// only an identical, still-unchanged creation may replay the original receipt.
// Resolve remains available after edit/delete and never recreates a connection.
type ConnectionCreationStore interface {
	CreateConnection(context.Context, ConnectionCreate) (ModelConnection, error)
	ConnectionCreation(context.Context, string) (ConnectionCreationReceipt, error)
}

func (c ModelConnection) Clone() ModelConnection {
	c.Models = append([]ConnectionModel(nil), c.Models...)
	c.HeaderNames = append([]string(nil), c.HeaderNames...)
	return c
}

// SameConfiguration compares submitted configuration, excluding storage-owned
// identity, revision and timestamps. Secret values remain a separate capability.
func (c ModelConnection) SameConfiguration(other ModelConnection) bool {
	return c.Name == other.Name && c.Protocol == other.Protocol && c.Endpoint == other.Endpoint && c.CatalogURL == other.CatalogURL &&
		c.Auth == other.Auth && c.Enabled == other.Enabled && c.HasAPIKey == other.HasAPIKey &&
		slices.Equal(c.Models, other.Models) && slices.Equal(c.HeaderNames, other.HeaderNames)
}
func connectionText(s string, max int) bool {
	return strings.TrimSpace(s) != "" && len(s) <= max && utf8.ValidString(s) && !strings.ContainsAny(s, "\x00\r\n")
}
func (c ModelConnection) Validate() error {
	if !connectionText(c.ID, 80) || c.Revision == 0 || !connectionText(c.Name, 120) {
		return errors.New("connection requires an identity, revision and bounded name")
	}
	switch c.Protocol {
	case "openai_chat", "openai_responses", "anthropic", "gemini":
	default:
		return errors.New("unsupported model protocol")
	}
	if c.Auth != "none" && c.Auth != "api_key" {
		return errors.New("choose no authentication or an explicit API key")
	}
	if c.Auth == "api_key" && !c.HasAPIKey {
		return errors.New("API key required for the selected authentication")
	}
	endpoint, err := url.Parse(c.Endpoint)
	if err != nil || len(c.Endpoint) > 256 || endpoint.Host == "" || (endpoint.Scheme != "https" && endpoint.Scheme != "http") || endpoint.User != nil || endpoint.RawQuery != "" || endpoint.Fragment != "" || endpoint.Path == "" || strings.HasSuffix(endpoint.Path, "/") {
		return errors.New("complete HTTP(S) request endpoint without credentials, query or fragment required (maximum 256 bytes)")
	}
	if endpoint.Scheme == "http" && endpoint.Hostname() != "localhost" {
		ip := net.ParseIP(endpoint.Hostname())
		if ip == nil || !ip.IsLoopback() {
			return errors.New("non-loopback model endpoints require HTTPS")
		}
	}
	if strings.Contains(c.Endpoint, "{") || strings.Contains(c.Endpoint, "}") {
		if c.Protocol != "gemini" || strings.Count(c.Endpoint, "{model}") != 1 || strings.ContainsAny(strings.ReplaceAll(c.Endpoint, "{model}", ""), "{}") {
			return errors.New("only Gemini supports an explicit {model} endpoint placeholder")
		}
	}
	if c.CatalogURL != "" {
		catalog, err := url.Parse(c.CatalogURL)
		if err != nil || len(c.CatalogURL) > 256 || catalog.Scheme != endpoint.Scheme || !strings.EqualFold(catalog.Host, endpoint.Host) || catalog.User != nil || catalog.RawQuery != "" || catalog.Fragment != "" || catalog.Path == "" || strings.ContainsAny(c.CatalogURL, "{}\r\n\x00") {
			return errors.New("catalog URL must be an explicit same-origin HTTP(S) URL without credentials, query, fragment or model placeholder")
		}
	}
	if len(c.Models) > 128 || len(c.HeaderNames) > 64 {
		return errors.New("connection exceeds model/header limits")
	}
	seen := map[string]bool{}
	for _, m := range c.Models {
		if !connectionText(m.ID, 256) || len(m.Name) > 120 || !utf8.ValidString(m.Name) || strings.ContainsAny(m.Name, "\x00\r\n") || seen[m.ID] {
			return errors.New("models require unique exact bounded IDs and bounded display names")
		}
		seen[m.ID] = true
		if err := c.Target(m).Validate(); err != nil {
			return errors.New("resolved model route exceeds identity/endpoint limits")
		}
	}
	return nil
}

// Apply is the only connection edit policy, reused inside the provider transaction.
func (c ModelConnection) Apply(u ConnectionUpdate, secret ConnectionSecrets) (ModelConnection, ConnectionSecrets, error) {
	if u.ID != c.ID || u.ExpectedRevision != c.Revision {
		return ModelConnection{}, ConnectionSecrets{}, ErrConnectionConflict
	}
	if u.ClearAPIKey && u.APIKey != "" {
		return ModelConnection{}, ConnectionSecrets{}, errors.New("cannot replace and clear an API key together")
	}
	if u.ClearAPIKey {
		secret.APIKey = ""
	} else if u.APIKey != "" {
		secret.APIKey = u.APIKey
	}
	if u.ReplaceHeaders {
		secret.Headers = cloneConnectionHeaders(u.Headers)
	} else {
		secret.Headers = cloneConnectionHeaders(secret.Headers)
	}
	if u.Auth == "none" {
		secret.APIKey = ""
	}
	if len(secret.APIKey) > 8192 || strings.ContainsAny(secret.APIKey, "\x00\r\n") {
		return ModelConnection{}, ConnectionSecrets{}, errors.New("invalid API key")
	}
	if len(secret.Headers) > 64 {
		return ModelConnection{}, ConnectionSecrets{}, errors.New("too many explicit model headers")
	}
	names := make([]string, 0, len(secret.Headers))
	seen := map[string]bool{}
	size := 0
	for k, v := range secret.Headers {
		name := http.CanonicalHeaderKey(k)
		if name == "" || len(k) > 128 || strings.ContainsAny(k, " :\r\n\x00") || strings.ContainsAny(v, "\r\n\x00") || seen[name] {
			return ModelConnection{}, ConnectionSecrets{}, errors.New("invalid or duplicate explicit model header")
		}
		for _, r := range k {
			if r > 127 || !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || strings.ContainsRune("!#$%&'*+-.^_`|~", r)) {
				return ModelConnection{}, ConnectionSecrets{}, errors.New("invalid model header name")
			}
		}
		switch name {
		case "Host", "Content-Length", "Transfer-Encoding", "Connection", "Content-Type", "Accept":
			return ModelConnection{}, ConnectionSecrets{}, errors.New("transport-controlled model header")
		}
		seen[name] = true
		names = append(names, name)
		size += len(k) + len(v)
	}
	if size > 32<<10 {
		return ModelConnection{}, ConnectionSecrets{}, errors.New("explicit model headers exceed 32 KiB")
	}
	sort.Strings(names)
	c.Revision++
	c.Name = u.Name
	c.Protocol = u.Protocol
	c.Endpoint = u.Endpoint
	c.CatalogURL = u.CatalogURL
	c.Auth = u.Auth
	c.Enabled = u.Enabled
	c.Models = append([]ConnectionModel(nil), u.Models...)
	c.HasAPIKey = secret.APIKey != ""
	c.HeaderNames = names
	if err := c.Validate(); err != nil {
		return ModelConnection{}, ConnectionSecrets{}, err
	}
	return c, secret, nil
}
func cloneConnectionHeaders(source map[string]string) map[string]string {
	result := make(map[string]string, len(source))
	for k, v := range source {
		result[k] = v
	}
	return result
}
func (c ModelConnection) ModelEndpoint(modelID string) string {
	return strings.ReplaceAll(c.Endpoint, "{model}", url.PathEscape(modelID))
}
func (c ModelConnection) Target(model ConnectionModel) Target {
	return Target{EngineID: "alva", ModelID: model.ID, ProviderID: c.Protocol, AccountSource: fmt.Sprintf("connection:%s@%d", c.ID, c.Revision), BillingRoute: c.ModelEndpoint(model.ID)}
}
