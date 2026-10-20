// INPUT: Explicit loopback/token configuration, registered UI owners and bounded diagnostic HTTP requests
// OUTPUT: Server lifecycle, view registration, controlled HTTP diagnostics and at-most-once UI actions
// POS: [L6 diagnostics] Closing transport detaches UI observers and joins admitted diagnostics without cancelling Agent execution.
package debug

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"image/png"
	"io"
	"log/slog"
	"mime"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"os"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/QuincySx/Alva/platform/storage/redaction"
	"github.com/QuincySx/Alva/platform/system/loopback"
	strictjson "github.com/QuincySx/Alva/platform/system/strictjson"
)

type Config struct {
	Token          string   `json:"-"`
	Secrets        []string `json:"-"`
	AllowControls  bool
	AllowExecution bool
}
type registration struct {
	ID, Type string
	Methods  []Method
	owner    *View
}
type operation struct {
	fingerprint [32]byte
	done        chan struct{}
	result      ActionResult
}
type Server struct {
	mu                   sync.Mutex
	config               Config
	life                 context.Context
	cancel               context.CancelFunc
	views                map[string]*registration
	operations           map[string]*operation
	ops                  sync.WaitGroup
	logs                 *logStore
	http                 *http.Server
	access               *loopback.Gate
	requests             chan struct{}
	connections          map[net.Conn]http.ConnState
	serveDone, closeDone chan struct{}
	closeOnce            sync.Once
	serveErr, closeErr   error
}

func New(config Config) (*Server, error) {
	if _, err := loopback.New("127.0.0.1:1", config.Token); err != nil {
		return nil, err
	}
	if config.AllowExecution && !config.AllowControls {
		return nil, errors.New("diagnostic execution also requires explicit UI controls")
	}
	if len(config.Secrets) > 128 {
		return nil, errors.New("too many explicit diagnostic secrets")
	}
	config.Secrets = append(append([]string(nil), config.Secrets...), config.Token)
	for _, secret := range config.Secrets {
		if len(secret) > 8192 {
			return nil, errors.New("diagnostic secret exceeds input budget")
		}
	}
	life, cancel := context.WithCancel(context.Background())
	return &Server{config: config, life: life, cancel: cancel, views: make(map[string]*registration), operations: make(map[string]*operation), logs: &logStore{secrets: config.Secrets}, requests: make(chan struct{}, 4), connections: make(map[net.Conn]http.ConnState), closeDone: make(chan struct{})}, nil
}
func (s *Server) Logger() *slog.Logger { return s.logs.logger() }
func (s *Server) Closed() bool         { return s.life.Err() != nil }

func (s *Server) Register(id, typeName string, methods []Method, view *View) (func(), error) {
	if !identifier.MatchString(id) || typeName == "" || len(typeName) > 128 || view == nil || len(methods) > 16 {
		return nil, errors.New("bounded actual diagnostic view required")
	}
	seen := make(map[string]bool)
	for _, method := range methods {
		if !identifier.MatchString(method.Name) || seen[method.Name] || len(method.Description) > 512 || (method.Effect != "ui" && method.Effect != "stop" && method.Effect != "execute") {
			return nil, errors.New("unique bounded declared UI methods required")
		}
		seen[method.Name] = true
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.life.Err() != nil {
		return nil, ErrUnavailable
	}
	if len(s.views) >= 32 || s.views[id] != nil {
		return nil, errors.New("diagnostic view limit or duplicate identity")
	}
	r := &registration{ID: id, Type: typeName, Methods: append([]Method(nil), methods...), owner: view}
	s.views[id] = r
	var once sync.Once
	return func() {
		once.Do(func() {
			s.mu.Lock()
			if s.views[id] == r {
				delete(s.views, id)
			}
			s.mu.Unlock()
			view.detach()
		})
	}, nil
}

func (s *Server) Start(address string) (string, error) {
	host, port, err := net.SplitHostPort(address)
	if err != nil {
		return "", errors.New("canonical numeric loopback IP:port required")
	}
	ip, ipErr := netip.ParseAddr(host)
	number, portErr := strconv.Atoi(port)
	if ipErr != nil || !ip.IsLoopback() || ip.Zone() != "" || portErr != nil || number < 0 || number > 65535 || net.JoinHostPort(ip.String(), strconv.Itoa(number)) != address {
		return "", errors.New("canonical numeric loopback IP:port required")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.life.Err() != nil || s.http != nil {
		return "", errors.New("diagnostic server already started or closed")
	}
	listener, err := net.Listen("tcp", address)
	if err != nil {
		return "", errors.New("diagnostic listener unavailable")
	}
	access, err := loopback.New(listener.Addr().String(), s.config.Token)
	if err != nil {
		listener.Close()
		return "", err
	}
	s.access = access
	s.http = &http.Server{Handler: s, ReadHeaderTimeout: 3 * time.Second, ReadTimeout: 6 * time.Second, WriteTimeout: 6 * time.Second, IdleTimeout: 15 * time.Second, MaxHeaderBytes: 16 << 10, BaseContext: func(net.Listener) context.Context { return s.life }, ConnState: s.connectionState}
	s.serveDone = make(chan struct{})
	go func() {
		err := s.http.Serve(listener)
		if !errors.Is(err, http.ErrServerClosed) {
			s.serveErr = err
		}
		close(s.serveDone)
	}()
	return "http://" + listener.Addr().String(), nil
}

func (s *Server) connectionState(connection net.Conn, state http.ConnState) {
	s.mu.Lock()
	drop := false
	if state == http.StateClosed || state == http.StateHijacked {
		delete(s.connections, connection)
	} else if state == http.StateNew && (s.life.Err() != nil || len(s.connections) >= 16) {
		drop = true
	} else {
		s.connections[connection] = state
	}
	s.mu.Unlock()
	if drop {
		_ = connection.Close()
	}
}

func (s *Server) BeginClose() {
	s.closeOnce.Do(func() {
		s.mu.Lock()
		s.cancel()
		s.logs.stopped.Store(true)
		server := s.http
		done := s.serveDone
		var idle []net.Conn
		for connection, state := range s.connections {
			if state == http.StateIdle || state == http.StateNew {
				idle = append(idle, connection)
			}
		}
		for _, view := range s.views {
			view.owner.detach()
		}
		s.views = make(map[string]*registration)
		s.mu.Unlock()
		go func() {
			for _, connection := range idle {
				_ = connection.Close()
			}
			if server != nil {
				s.closeErr = server.Shutdown(context.Background())
				<-done
				s.closeErr = errors.Join(s.closeErr, s.serveErr)
			}
			s.ops.Wait()
			close(s.closeDone)
		}()
	})
}
func (s *Server) Close(ctx context.Context) error {
	s.BeginClose()
	select {
	case <-s.closeDone:
		return s.closeErr
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (s *Server) view(id string) (*registration, error) {
	if !identifier.MatchString(id) {
		return nil, ErrUnavailable
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	view := s.views[id]
	if view == nil || view.owner.life.Err() != nil {
		return nil, ErrUnavailable
	}
	return view, nil
}
func safeError(err error) string {
	for _, known := range []error{ErrUnavailable, ErrBusy, ErrStale, ErrFramePending, context.Canceled, context.DeadlineExceeded} {
		if errors.Is(err, known) {
			return known.Error()
		}
	}
	return "diagnostic operation rejected; private cause omitted"
}
func (s *Server) act(ctx context.Context, action Action) (ActionResult, error) {
	if err := action.validate(); err != nil {
		return ActionResult{}, err
	}
	data, _ := json.Marshal(action)
	fingerprint := sha256.Sum256(data)
	s.mu.Lock()
	if prior := s.operations[action.OperationID]; prior != nil {
		s.mu.Unlock()
		if prior.fingerprint != fingerprint {
			return ActionResult{}, errors.New("operation ID was already used for a different request")
		}
		select {
		case <-prior.done:
			return prior.result, nil
		case <-ctx.Done():
			return ActionResult{OperationID: action.OperationID, Status: "pending"}, nil
		}
	}
	if s.life.Err() != nil {
		s.mu.Unlock()
		return ActionResult{}, ErrUnavailable
	}
	view := s.views[action.ViewID]
	if view == nil || view.owner.life.Err() != nil {
		s.mu.Unlock()
		return ActionResult{}, ErrUnavailable
	}
	allowed := false
	for _, method := range view.Methods {
		if method.Name == action.Method {
			allowed = s.config.AllowControls && (method.Effect != "execute" || s.config.AllowExecution)
			break
		}
	}
	if !allowed {
		s.mu.Unlock()
		return ActionResult{}, errors.New("this process operator did not enable the declared UI action")
	}
	if len(s.operations) >= 256 {
		s.mu.Unlock()
		return ActionResult{}, errors.New("diagnostic operation budget exhausted; IDs are never evicted for replay")
	}
	op := &operation{fingerprint: fingerprint, done: make(chan struct{})}
	s.operations[action.OperationID] = op
	s.ops.Add(1)
	s.mu.Unlock()
	settle := func(result viewResult) {
		defer s.ops.Done()
		op.result = result.action
		if result.err != nil {
			status := "rejected"
			if errors.Is(result.err, context.Canceled) || errors.Is(result.err, context.DeadlineExceeded) {
				status = "cancelled"
			}
			op.result = ActionResult{OperationID: action.OperationID, Status: status, Error: safeError(result.err)}
		}
		s.Logger().Info("UI diagnostic action settled", "view", action.ViewID, "method", action.Method, "operation", action.OperationID, "status", op.result.Status)
		close(op.done)
	}
	done, err := view.owner.action(ctx, action)
	if err != nil {
		settle(viewResult{err: err})
	} else {
		go func() { settle(<-done) }()
	}
	select {
	case <-op.done:
		return op.result, nil
	case <-ctx.Done():
		return ActionResult{OperationID: action.OperationID, Status: "pending"}, nil
	}
}

func writeJSON(w http.ResponseWriter, status int, value any) {
	data, err := json.Marshal(value)
	if err != nil || len(data) > 128<<10 {
		http.Error(w, "diagnostic response exceeds budget", http.StatusRequestEntityTooLarge)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_, _ = w.Write(data)
}
func inputJSON(w http.ResponseWriter, r *http.Request, out any) error {
	kind, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
	if err != nil || kind != "application/json" {
		return errors.New("exact JSON input required")
	}
	raw, err := io.ReadAll(http.MaxBytesReader(w, r.Body, 32<<10))
	if err != nil {
		return errors.New("diagnostic input exceeds budget")
	}
	if err := strictjson.ValidateObject(raw, out); err != nil {
		return err
	}
	return json.Unmarshal(raw, out)
}
func diagnosticQuery(r *http.Request, allowed ...string) (url.Values, error) {
	query, err := url.ParseQuery(r.URL.RawQuery)
	if err != nil {
		return nil, err
	}
	keys := make(map[string]bool)
	for _, key := range allowed {
		keys[key] = true
	}
	for key, values := range query {
		if !keys[key] || len(values) != 1 {
			return nil, errors.New("unexpected diagnostic query")
		}
	}
	return query, nil
}
func validateCapture(data []byte) error {
	if len(data) > 4<<20 {
		return errors.New("window PNG exceeds 4 MiB")
	}
	image, err := png.DecodeConfig(bytes.NewReader(data))
	if err != nil || image.Width <= 0 || image.Height <= 0 || image.Width > 8192 || image.Height > 8192 || int64(image.Width)*int64(image.Height) > 16_000_000 {
		return errors.New("bounded actual window PNG required")
	}
	if _, err := png.Decode(bytes.NewReader(data)); err != nil {
		return errors.New("registered window PNG is incomplete or corrupt")
	}
	return nil
}
func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	loopback.SecureHeaders(w)
	if status := s.access.Check(r); status != 0 {
		http.Error(w, "diagnostic access denied", status)
		return
	}
	if s.life.Err() != nil {
		http.Error(w, "diagnostic service closed", 503)
		return
	}
	select {
	case s.requests <- struct{}{}:
		defer func() { <-s.requests }()
	default:
		http.Error(w, "diagnostic request limit", 503)
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 5*time.Second)
	defer cancel()
	r = r.WithContext(ctx)
	if r.URL.RawPath != "" || strings.Contains(r.URL.Path, "//") || strings.ContainsAny(r.URL.Path, "\\\x00") {
		http.NotFound(w, r)
		return
	}
	fail := func(err error) { writeJSON(w, 400, map[string]string{"error": safeError(err)}) }
	switch {
	case r.URL.Path == "/v1/debug/health" && r.Method == http.MethodGet:
		if _, err := diagnosticQuery(r); err != nil {
			fail(err)
			return
		}
		writeJSON(w, 200, map[string]any{"status": "ready", "controls": s.config.AllowControls, "execution": s.config.AllowExecution, "process_id": os.Getpid()})
	case r.URL.Path == "/v1/debug/level" && (r.Method == http.MethodGet || r.Method == http.MethodPut):
		if _, err := diagnosticQuery(r); err != nil {
			fail(err)
			return
		}
		if r.Method == http.MethodPut {
			var body struct {
				Level string `json:"level"`
			}
			if !s.config.AllowControls {
				http.Error(w, "diagnostic controls disabled", 403)
				return
			}
			if err := inputJSON(w, r, &body); err != nil {
				fail(err)
				return
			}
			if err := s.logs.setLevel(body.Level); err != nil {
				fail(err)
				return
			}
		}
		writeJSON(w, 200, map[string]string{"level": strings.ToLower(s.logs.level.Level().String())})
	case r.URL.Path == "/v1/debug/logs" && r.Method == http.MethodGet:
		query, err := diagnosticQuery(r, "after", "high_water", "limit", "level", "q")
		if err != nil {
			fail(err)
			return
		}
		var parsed LogQuery
		parsed.Level = query.Get("level")
		parsed.Keyword = query.Get("q")
		for _, entry := range []struct {
			key    string
			target *uint64
		}{{"after", &parsed.After}, {"high_water", &parsed.HighWater}} {
			if raw := query.Get(entry.key); raw != "" {
				value, err := strconv.ParseUint(raw, 10, 64)
				if err != nil {
					fail(err)
					return
				}
				*entry.target = value
			}
		}
		if raw := query.Get("limit"); raw != "" {
			parsed.Limit, err = strconv.Atoi(raw)
			if err != nil {
				fail(err)
				return
			}
		}
		page, err := s.logs.page(parsed)
		if err != nil {
			fail(err)
			return
		}
		writeJSON(w, 200, page)
	case r.URL.Path == "/v1/debug/views" && r.Method == http.MethodGet:
		if _, err := diagnosticQuery(r); err != nil {
			fail(err)
			return
		}
		s.mu.Lock()
		var views []map[string]any
		for _, view := range s.views {
			if view.owner.life.Err() == nil {
				views = append(views, map[string]any{"id": view.ID, "type": view.Type, "methods": append([]Method(nil), view.Methods...)})
			}
		}
		s.mu.Unlock()
		sort.Slice(views, func(i, j int) bool { return views[i]["id"].(string) < views[j]["id"].(string) })
		writeJSON(w, 200, map[string]any{"views": views, "tree_kind": "registered product views; framework internal layout is not exported"})
	case (r.URL.Path == "/v1/debug/state" || r.URL.Path == "/v1/debug/tree") && r.Method == http.MethodGet:
		query, err := diagnosticQuery(r, "view")
		if err != nil {
			fail(err)
			return
		}
		view, err := s.view(query.Get("view"))
		if err != nil {
			fail(err)
			return
		}
		snapshot, err := view.owner.read(ctx, view.ID)
		if err != nil {
			fail(err)
			return
		}
		raw, _ := json.Marshal(snapshot)
		redacted, err := redaction.RedactJSON(raw, s.config.Secrets...)
		if err != nil {
			fail(err)
			return
		}
		writeJSON(w, 200, json.RawMessage(redacted))
	case r.URL.Path == "/v1/debug/actions" && r.Method == http.MethodPost:
		if _, err := diagnosticQuery(r); err != nil {
			fail(err)
			return
		}
		var action Action
		if err := inputJSON(w, r, &action); err != nil {
			fail(err)
			return
		}
		result, err := s.act(ctx, action)
		if err != nil {
			fail(err)
			return
		}
		writeJSON(w, 200, result)
	case r.URL.Path == "/v1/debug/capture" && r.Method == http.MethodPost:
		if _, err := diagnosticQuery(r); err != nil {
			fail(err)
			return
		}
		var body struct {
			ViewID string `json:"view_id"`
		}
		if err := inputJSON(w, r, &body); err != nil {
			fail(err)
			return
		}
		view, err := s.view(body.ViewID)
		if err != nil {
			fail(err)
			return
		}
		data, err := view.owner.image(ctx)
		if err != nil {
			fail(err)
			return
		}
		if err := validateCapture(data); err != nil {
			fail(err)
			return
		}
		w.Header().Set("Content-Type", "image/png")
		w.WriteHeader(200)
		_, _ = w.Write(data)
	case r.URL.Path == "/v1/debug/shutdown" && r.Method == http.MethodPost:
		if _, err := diagnosticQuery(r); err != nil {
			fail(err)
			return
		}
		var body struct{}
		if !s.config.AllowControls {
			http.Error(w, "diagnostic controls disabled", 403)
			return
		}
		if err := inputJSON(w, r, &body); err != nil {
			fail(err)
			return
		}
		writeJSON(w, 200, map[string]any{"diagnostic_shutdown": "requested", "agent_execution": "unchanged"})
		s.BeginClose()
	default:
		http.NotFound(w, r)
	}
}
