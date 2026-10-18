// INPUT: Shared product.HubService, authenticated local requests and bounded JSON payloads
// OUTPUT: New Server http.Handler for the same native product authority as desktop
// POS: [L7 Hub product] Transport-only HTTP facade; owns neither model execution nor a second business runtime
package server

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"io"
	"mime"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/platform/system/httpjson"
	"github.com/QuincySx/Alva/platform/system/loopback"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type Server struct {
	service   *product.HubService
	config    Config
	access    *loopback.Gate
	requests  chan struct{}
	uploads   chan struct{}
	streams   chan struct{}
	exports   chan struct{}
	done      chan struct{}
	closeOnce sync.Once
	// Freeze the explicit routes at assembly, rather than accepting fabricated
	// model/account/billing labels that happen to use an installed engine name.
	targets []hub.Target
}

func New(service *product.HubService, config Config) (*Server, error) {
	if service == nil || service.Store == nil || service.Runtime == nil || service.Repository == nil {
		return nil, errors.New("shared assembled product service required")
	}
	if err := config.Validate(); err != nil {
		return nil, err
	}
	access, err := loopback.New(config.Authority, config.Token)
	if err != nil {
		return nil, err
	}
	if config.PollInterval == 0 {
		config.PollInterval = 150 * time.Millisecond
	}
	targets := append([]hub.Target(nil), service.Targets...)
	for i := range targets {
		targets[i].RequiredCapabilities = append([]string(nil), targets[i].RequiredCapabilities...)
	}
	return &Server{service: service, config: config, access: access, requests: make(chan struct{}, 64), uploads: make(chan struct{}, 2), streams: make(chan struct{}, 16), exports: make(chan struct{}, 2), done: make(chan struct{}), targets: targets}, nil
}

// Close stops transport admission and observers, never the shared runtime.
// The application owner separately closes bootstrap.Application after draining HTTP.
func (s *Server) Close() error { s.closeOnce.Do(func() { close(s.done) }); return nil }

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if !s.authenticate(w, r) {
		return
	}
	select {
	case <-s.done:
		fail(w, 503)
		return
	default:
	}
	_ = http.NewResponseController(w).SetWriteDeadline(time.Now().Add(30 * time.Second))
	select {
	case s.requests <- struct{}{}:
		defer func() { <-s.requests }()
	default:
		fail(w, http.StatusServiceUnavailable)
		return
	}
	// No forwarding, static React bundle, implicit CORS, redirects or path-cleanup
	// aliases. A malformed/escaped route must not turn into a different mutation.
	if r.URL.RawPath != "" || strings.Contains(r.URL.Path, "//") || strings.ContainsAny(r.URL.Path, "\\\x00") {
		fail(w, 404)
		return
	}
	if r.URL.Path == "/v1/health" && r.Method == http.MethodGet {
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		respond(w, 200, map[string]any{"status": "ready", "demo": s.service.Demo})
		return
	}
	if r.URL.Path == "/v1/targets" && r.Method == http.MethodGet {
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		respond(w, 200, s.targets)
		return
	}
	if r.URL.Path == "/v1/engines" && r.Method == http.MethodGet {
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		respond(w, 200, s.service.Runtime.EngineDescriptions())
		return
	}
	if r.URL.Path == "/v1/gateway" {
		s.localGateway(w, r)
		return
	}
	if r.URL.Path == "/v1/sessions" {
		s.sessions(w, r)
		return
	}
	parts := strings.Split(strings.TrimPrefix(r.URL.Path, "/"), "/")
	if len(parts) < 3 || parts[0] != "v1" || parts[1] != "sessions" || !validID(parts[2]) {
		fail(w, 404)
		return
	}
	id := parts[2]
	// Every session subresource first resolves the committed product identity.
	// Neither an owner nor a native session ID may be chosen by the client.
	if _, err := s.service.Store.Detail(r.Context(), id); err != nil {
		operationError(w, err)
		return
	}
	if len(parts) == 3 && r.Method == http.MethodGet {
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		detail, err := s.service.Store.Detail(r.Context(), id)
		reply(w, detail, err)
		return
	}
	if len(parts) < 4 {
		fail(w, 405)
		return
	}
	if len(parts) > 4 {
		s.nested(w, r, id, parts[3:])
		return
	}
	s.sessionRoute(w, r, id, parts[3])
}

func (s *Server) sessions(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		q, err := query(r, "q", "updated", "cursor", "limit")
		if err != nil {
			fail(w, 400)
			return
		}
		limit, err := integer(q, "limit", 32, 1, 64)
		if err != nil {
			fail(w, 400)
			return
		}
		updated, err := integer(q, "updated", 0, 0, 1<<63-1)
		if err != nil {
			fail(w, 400)
			return
		}
		if len(q.Get("q")) > 256 || (q.Get("cursor") != "" && !validID(q.Get("cursor"))) || (updated == 0) != (q.Get("cursor") == "") {
			fail(w, 400)
			return
		}
		page, err := s.service.Store.PageSessions(r.Context(), q.Get("q"), hub.SessionCursor{Updated: updated, ID: q.Get("cursor")}, int(limit))
		if err != nil {
			operationError(w, err)
			return
		}
		respond(w, 200, struct {
			Sessions []hub.Session     `json:"sessions"`
			Next     hub.SessionCursor `json:"next"`
			HasMore  bool              `json:"has_more"`
		}{page.Sessions, page.Next, page.HasMore})
	case http.MethodPost:
		if _, err := query(r); err != nil {
			fail(w, 400)
			return
		}
		var value struct {
			Title     string `json:"title"`
			Workspace string `json:"workspace"`
		}
		if !decode(w, r, &value, 64<<10) {
			return
		}
		result, err := s.service.Create(r.Context(), value.Title, value.Workspace)
		if err != nil {
			operationError(w, err)
			return
		}
		respond(w, 201, result)
	default:
		fail(w, 405)
	}
}

func query(r *http.Request, allowed ...string) (url.Values, error) {
	q, err := url.ParseQuery(r.URL.RawQuery)
	if err != nil {
		return nil, err
	}
	for key, values := range q {
		ok := false
		for _, name := range allowed {
			ok = ok || key == name
		}
		if !ok || len(values) != 1 || !utf8.ValidString(values[0]) {
			return nil, errors.New("invalid query")
		}
	}
	return q, nil
}
func integer(q url.Values, key string, fallback, min, max int64) (int64, error) {
	if !q.Has(key) {
		return fallback, nil
	}
	value, err := strconv.ParseInt(q.Get(key), 10, 64)
	if err != nil || value < min || value > max || strconv.FormatInt(value, 10) != q.Get(key) {
		return 0, errors.New("invalid integer")
	}
	return value, nil
}
func validID(value string) bool {
	if len(value) == 0 || len(value) > 128 {
		return false
	}
	for _, c := range value {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-' || c == '_') {
			return false
		}
	}
	return true
}
func decode(w http.ResponseWriter, r *http.Request, out any, limit int64) bool {
	media, params, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
	if err != nil || media != "application/json" || len(params) > 1 || (len(params) == 1 && !strings.EqualFold(params["charset"], "utf-8")) {
		fail(w, 415)
		return false
	}
	data, err := io.ReadAll(http.MaxBytesReader(w, r.Body, limit))
	if err != nil {
		var large *http.MaxBytesError
		if errors.As(err, &large) {
			fail(w, 413)
		} else {
			fail(w, 400)
		}
		return false
	}
	if !wire.ValidJSON(data) {
		fail(w, 400)
		return false
	}
	if httpjson.ValidateObject(data, out) != nil {
		fail(w, 400)
		return false
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if decoder.Decode(out) != nil || decoder.Decode(new(any)) != io.EOF {
		fail(w, 400)
		return false
	}
	return true
}
func respond(w http.ResponseWriter, status int, value any) {
	data, err := json.Marshal(value)
	if err != nil || len(data) > 8<<20 {
		fail(w, 507)
		return
	}
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_, _ = w.Write(append(data, '\n'))
}
func fail(w http.ResponseWriter, status int) {
	// No raw filesystem/database/provider/child stderr is a transport error DTO.
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_, _ = io.WriteString(w, `{"error":"`+http.StatusText(status)+`"}`+"\n")
}
func reply(w http.ResponseWriter, value any, err error) {
	if err != nil {
		operationError(w, err)
		return
	}
	respond(w, 200, value)
}
func operationError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, sql.ErrNoRows), errors.Is(err, session.ErrNotFound):
		fail(w, 404)
	case errors.Is(err, context.Canceled), errors.Is(err, context.DeadlineExceeded):
		fail(w, 408)
	default:
		fail(w, 409)
	}
}

func (s *Server) targetAllowed(target hub.Target) bool {
	if target.Validate() != nil {
		return false
	}
	for _, configured := range s.targets {
		if hub.RouteKey(configured) == hub.RouteKey(target) {
			return true
		}
	}
	return false
}
