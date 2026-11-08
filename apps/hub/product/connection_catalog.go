// INPUT: Explicit current connection drafts, exact saved-secret revisions and operator-triggered read-only catalog requests.
// OUTPUT: Bounded model candidates (with explicitly stated catalog context windows) and truthful catalog-access tests with sanitized errors and joined cancellation.
// POS: [L7 product service] Reuses the connection owner; no generation, discovery, redirects, retries or ambient proxy.
package product

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
)

var ErrConnectionCatalogUnavailable = errors.New("model catalog unavailable; enter an explicit same-origin catalog URL or add model IDs manually")

// ConnectionCatalog contains candidates only. Fetching never saves or enables them.
type ConnectionCatalog struct {
	Models []hub.ConnectionModel `json:"models"`
}

// ConnectionTest identifies the actual validation scope; generation and Agent compatibility remain untested.
type ConnectionTest struct {
	Scope      string `json:"scope"`
	ModelCount int    `json:"model_count"`
}

// TestConnection verifies access to the explicitly configured model catalog with a GET.
// It deliberately never submits a generation request, even when no catalog is available.
func (s *ConnectionsService) TestConnection(ctx context.Context, draft hub.ConnectionUpdate) (ConnectionTest, error) {
	result, err := s.FetchModels(ctx, draft)
	if err != nil {
		return ConnectionTest{}, err
	}
	return ConnectionTest{Scope: "model_catalog_access", ModelCount: len(result.Models)}, nil
}

func (s *ConnectionsService) FetchModels(ctx context.Context, draft hub.ConnectionUpdate) (ConnectionCatalog, error) {
	ctx, err := s.acquire(ctx)
	if err != nil {
		return ConnectionCatalog{}, err
	}
	defer s.release()
	c, secret, err := s.catalogDraft(ctx, draft)
	if err != nil {
		return ConnectionCatalog{}, err
	}
	if c.CatalogURL == "" {
		return ConnectionCatalog{}, ErrConnectionCatalogUnavailable
	}
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	transport := &http.Transport{Proxy: nil, DialContext: (&net.Dialer{Timeout: 10 * time.Second}).DialContext, TLSHandshakeTimeout: 10 * time.Second, ResponseHeaderTimeout: 10 * time.Second, DisableKeepAlives: true, MaxResponseHeaderBytes: 32 << 10}
	defer transport.CloseIdleConnections()
	client := &http.Client{Transport: transport, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	return fetchConnectionCatalog(ctx, client, c, secret)
}

func (s *ConnectionsService) catalogDraft(ctx context.Context, u hub.ConnectionUpdate) (hub.ModelConnection, hub.ConnectionSecrets, error) {
	current := hub.ModelConnection{ID: "catalog-draft"}
	var secret hub.ConnectionSecrets
	if u.ID != "" {
		var found bool
		current, found = s.Connection(u.ID)
		if !found {
			return hub.ModelConnection{}, secret, hub.ErrNotFound
		}
		if current.Revision != u.ExpectedRevision {
			return hub.ModelConnection{}, secret, hub.ErrConnectionConflict
		}
		// An edited source cannot silently borrow credentials from the saved source.
		changedSource := current.Protocol != u.Protocol || current.Endpoint != u.Endpoint || current.Auth != u.Auth
		if changedSource && ((current.HasAPIKey && u.APIKey == "" && !u.ClearAPIKey && u.Auth != "none") || len(current.HeaderNames) > 0 && !u.ReplaceHeaders) {
			return hub.ModelConnection{}, secret, errors.New("connection source changed; explicitly enter a key and replace saved headers before testing this source")
		}
		if !changedSource {
			var err error
			secret, err = s.store.ConnectionSecrets(ctx, u.ID, u.ExpectedRevision)
			if err != nil {
				return hub.ModelConnection{}, secret, connectionError("read", err)
			}
		}
	} else {
		if u.ExpectedRevision != 0 {
			return hub.ModelConnection{}, secret, hub.ErrConnectionConflict
		}
		u.ID = current.ID
	}
	checked, secret, err := current.Apply(u, secret)
	return checked, secret, err
}

func fetchConnectionCatalog(ctx context.Context, client *http.Client, c hub.ModelConnection, secret hub.ConnectionSecrets) (ConnectionCatalog, error) {
	var models []hub.ConnectionModel
	ids, cursors := map[string]bool{}, map[string]bool{}
	cursor := ""
	for page := 0; page < 8; page++ {
		address, _ := url.Parse(c.CatalogURL) // ModelConnection.Validate has already constrained the exact origin and URL.
		query := address.Query()
		switch c.Protocol {
		case "anthropic":
			query.Set("limit", "256")
			if cursor != "" {
				query.Set("after_id", cursor)
			}
		case "gemini":
			query.Set("pageSize", "256")
			if cursor != "" {
				query.Set("pageToken", cursor)
			}
		}
		address.RawQuery = query.Encode()
		request, err := http.NewRequestWithContext(ctx, http.MethodGet, address.String(), nil)
		if err != nil {
			return ConnectionCatalog{}, errors.New("invalid model catalog request")
		}
		request.Header.Set("Accept", "application/json")
		if c.Protocol == "anthropic" {
			request.Header.Set("Anthropic-Version", "2023-06-01")
		}
		if c.Auth == "api_key" {
			switch c.Protocol {
			case "anthropic":
				request.Header.Set("X-Api-Key", secret.APIKey)
			case "gemini":
				request.Header.Set("X-Goog-Api-Key", secret.APIKey)
			default:
				request.Header.Set("Authorization", "Bearer "+secret.APIKey)
			}
		}
		for k, v := range secret.Headers {
			request.Header.Set(k, v)
		}
		response, err := client.Do(request)
		if err != nil {
			return ConnectionCatalog{}, catalogNetworkError(ctx, err)
		}
		// Error bodies, headers, URLs and transport errors can echo credentials. Never expose them.
		if response.StatusCode != http.StatusOK {
			response.Body.Close()
			if response.StatusCode == 404 || response.StatusCode == 405 || response.StatusCode == 501 {
				return ConnectionCatalog{}, ErrConnectionCatalogUnavailable
			}
			if response.StatusCode >= 300 && response.StatusCode < 400 {
				return ConnectionCatalog{}, errors.New("model catalog redirect refused; enter the exact same-origin catalog URL")
			}
			return ConnectionCatalog{}, fmt.Errorf("model catalog request failed (HTTP %d)", response.StatusCode)
		}
		data, readErr := io.ReadAll(io.LimitReader(response.Body, (1<<20)+1))
		response.Body.Close()
		if readErr != nil {
			return ConnectionCatalog{}, catalogNetworkError(ctx, readErr)
		}
		if len(data) > 1<<20 {
			return ConnectionCatalog{}, errors.New("model catalog response exceeds 1 MiB")
		}
		entries, next, err := decodeConnectionCatalog(c.Protocol, data)
		if err != nil {
			return ConnectionCatalog{}, err
		}
		for _, entry := range entries {
			if catalogPrivateText(entry.ID+"\n"+entry.Name, secret) {
				return ConnectionCatalog{}, errors.New("model catalog returned private credential material")
			}
			if ids[entry.ID] {
				continue
			}
			ids[entry.ID] = true
			models = append(models, entry)
			if len(models) > 2048 {
				return ConnectionCatalog{}, errors.New("model catalog exceeds 2048 candidates")
			}
		}
		if next == "" {
			sort.Slice(models, func(i, j int) bool { return models[i].ID < models[j].ID })
			return ConnectionCatalog{Models: models}, nil
		}
		if len(next) > 2048 || !utf8.ValidString(next) || strings.ContainsAny(next, "\x00\r\n") || cursors[next] {
			return ConnectionCatalog{}, errors.New("model catalog returned an invalid pagination cursor")
		}
		cursors[next], cursor = true, next
	}
	return ConnectionCatalog{}, errors.New("model catalog exceeds eight pages; narrow the service catalog or add exact model IDs manually")
}
func catalogNetworkError(ctx context.Context, err error) error {
	if ctx.Err() != nil {
		return ctx.Err()
	}
	var network net.Error
	if errors.As(err, &network) && network.Timeout() {
		return context.DeadlineExceeded
	}
	return errors.New("model catalog network request failed")
}
func catalogPrivateText(value string, secret hub.ConnectionSecrets) bool {
	if secret.APIKey != "" && strings.Contains(value, secret.APIKey) {
		return true
	}
	for _, private := range secret.Headers {
		if private != "" && strings.Contains(value, private) {
			return true
		}
	}
	return false
}
func decodeConnectionCatalog(protocol string, data []byte) ([]hub.ConnectionModel, string, error) {
	var envelope struct {
		Data          json.RawMessage `json:"data"`
		Models        json.RawMessage `json:"models"`
		HasMore       bool            `json:"has_more"`
		LastID        string          `json:"last_id"`
		NextPageToken string          `json:"nextPageToken"`
	}
	invalid := errors.New("model catalog returned a malformed response")
	if json.Unmarshal(data, &envelope) != nil {
		return nil, "", invalid
	}
	raw := envelope.Data
	if protocol == "gemini" {
		raw = envelope.Models
	}
	if len(raw) == 0 || raw[0] != '[' {
		return nil, "", invalid
	}
	var items []struct {
		ID            string `json:"id"`
		Name          string `json:"name"`
		DisplayName   string `json:"displayName"`
		AnthropicName string `json:"display_name"`
		// Explicit context lengths in documented catalog shapes: Gemini
		// inputTokenLimit; gateways' context_length (OpenRouter, Together) or
		// max_model_len (vLLM). OpenAI's and Anthropic's own catalogs state
		// none, so their models stay undeclared. Only a JSON integer counts;
		// any other value leaves the model undeclared without failing the fetch.
		InputTokenLimit json.RawMessage `json:"inputTokenLimit"`
		ContextLength   json.RawMessage `json:"context_length"`
		MaxModelLength  json.RawMessage `json:"max_model_len"`
	}
	if json.Unmarshal(raw, &items) != nil {
		return nil, "", invalid
	}
	models := make([]hub.ConnectionModel, 0, len(items))
	for _, item := range items {
		id, name := item.ID, item.AnthropicName
		if protocol == "gemini" {
			if !strings.HasPrefix(item.Name, "models/") {
				return nil, "", invalid
			}
			id, name = strings.TrimPrefix(item.Name, "models/"), item.DisplayName
		}
		if strings.TrimSpace(id) == "" || len(id) > 256 || !utf8.ValidString(id) || strings.ContainsAny(id, "\x00\r\n") || len(name) > 512 || !utf8.ValidString(name) || strings.ContainsAny(name, "\x00\r\n") {
			return nil, "", invalid
		}
		// Display names are optional presentation. Preserve exact IDs and fit the shared editor's bound.
		for len(name) > 120 {
			_, size := utf8.DecodeLastRuneInString(name)
			name = name[:len(name)-size]
		}
		window := catalogContextWindow(item.ContextLength, item.MaxModelLength)
		if protocol == "gemini" {
			window = catalogContextWindow(item.InputTokenLimit)
		}
		models = append(models, hub.ConnectionModel{ID: id, Name: name, CatalogContextWindow: window})
	}
	next := ""
	if protocol == "anthropic" && envelope.HasMore {
		next = envelope.LastID
		if next == "" || len(items) == 0 {
			return nil, "", invalid
		}
	}
	if protocol == "gemini" {
		next = envelope.NextPageToken
	}
	return models, next, nil
}

// catalogContextWindow returns the first explicit integer token count within
// the accepted bounds; anything else leaves the window undeclared.
func catalogContextWindow(values ...json.RawMessage) int {
	for _, value := range values {
		if len(value) == 0 || string(value) == "null" {
			continue
		}
		tokens, err := strconv.ParseInt(string(value), 10, 64)
		if err == nil && tokens >= hub.MinContextWindow && tokens <= hub.MaxContextWindow {
			return int(tokens)
		}
		return 0
	}
	return 0
}
