// INPUT: Explicit model routes, canonical provider transport, pure codec and strict local HTTP access gate
// OUTPUT: Authenticated four-protocol Gateway with real Magpie conversion and owned cancellation
// POS: [L6 application] Independent L6 model API; never a second Agent loop, credential discovery or native conversation store
package gateway

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"mime"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/QuincySx/Alva/go/integration/llm/codec"
	"github.com/QuincySx/Alva/go/integration/llm/provider"
	"github.com/QuincySx/Alva/go/platform/system/httpjson"
	"github.com/QuincySx/Alva/go/platform/system/loopback"
)

type Route struct {
	Kind     provider.Kind
	Provider provider.Config
}
type Observation struct {
	Stage              string
	Alias              string
	From, To           codec.Protocol
	Converter, Quality string
	Steps              []codec.Step
	SHA256             string
}
type Config struct {
	Authority string
	Token     string `json:"-"`
	Routes    map[string]Route
	// Observe is a trusted host sink. It receives metadata/hashes, never keys,
	// user messages, raw responses or arbitrary conversion diagnostic text.
	Observe func(context.Context, Observation) error
}
type modelRoute struct {
	protocol  codec.Protocol
	model     string
	maxTokens int
	transport *provider.RawTransport
}
type Server struct {
	access  *loopback.Gate
	routes  map[string]modelRoute
	observe func(context.Context, Observation) error
	owner   context.Context
	cancel  context.CancelFunc
	mu      sync.Mutex
	closed  bool
	active  int
	idle    chan struct{}
}

func New(config Config) (*Server, error) {
	access, err := loopback.New(config.Authority, config.Token)
	if err != nil {
		return nil, err
	}
	if len(config.Routes) < 1 || len(config.Routes) > 16 {
		return nil, errors.New("gateway requires 1..16 explicit model routes")
	}
	routes := make(map[string]modelRoute, len(config.Routes))
	for alias, route := range config.Routes {
		if !validModel(alias) {
			return nil, errors.New("invalid bounded gateway model alias")
		}
		transport, err := provider.BuildTransport(route.Kind, route.Provider)
		if err != nil {
			return nil, err
		}
		routes[alias] = modelRoute{protocol: codec.Protocol(route.Kind), model: route.Provider.Model, maxTokens: route.Provider.MaxOutputTokens, transport: transport}
	}
	owner, cancel := context.WithCancel(context.Background())
	return &Server{access: access, routes: routes, observe: config.Observe, owner: owner, cancel: cancel}, nil
}

func validModel(alias string) bool {
	if alias == "" || len(alias) > 256 {
		return false
	}
	for _, c := range alias {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || strings.ContainsRune("._-:/", c)) {
			return false
		}
	}
	return true
}

func (s *Server) enter() (func(), bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed || s.active >= 8 {
		return nil, false
	}
	if s.active == 0 {
		s.idle = make(chan struct{})
	}
	s.active++
	return func() {
		s.mu.Lock()
		s.active--
		if s.active == 0 {
			close(s.idle)
		}
		s.mu.Unlock()
	}, true
}

// Close stops admission and cancels actual upstream work before waiting for its
// real handlers. A timeout retains the owner and may be retried; no request is
// automatically replayed and no billing acceptance is inferred from cancellation.
func (s *Server) Close(ctx context.Context) error {
	s.mu.Lock()
	s.closed = true
	s.cancel()
	active, idle := s.active, s.idle
	s.mu.Unlock()
	if active > 0 {
		select {
		case <-idle:
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	return nil
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	loopback.SecureHeaders(w)
	if status := s.access.Check(r); status != 0 {
		if status == 401 {
			w.Header().Set("WWW-Authenticate", "Bearer")
		}
		writeError(w, codec.OpenAIChat, status)
		return
	}
	end, ok := s.enter()
	if !ok {
		writeError(w, codec.OpenAIChat, 503)
		return
	}
	defer end()
	ctx, cancel := context.WithTimeout(s.owner, 10*time.Minute)
	stop := context.AfterFunc(r.Context(), cancel)
	defer stop()
	defer cancel()
	if r.Context().Err() != nil {
		return
	}
	r = r.WithContext(ctx)
	if r.URL.RawPath != "" || strings.Contains(r.URL.Path, "//") || strings.ContainsAny(r.URL.Path, "\\\x00") {
		writeError(w, codec.OpenAIChat, 404)
		return
	}
	if r.Method == http.MethodGet && r.URL.Path == "/v1/models" && r.URL.RawQuery == "" {
		aliases := make([]string, 0, len(s.routes))
		for alias := range s.routes {
			aliases = append(aliases, alias)
		}
		sort.Strings(aliases)
		data := make([]map[string]any, 0, len(aliases))
		for _, alias := range aliases {
			data = append(data, map[string]any{"id": alias, "object": "model", "owned_by": "explicit-local-route"})
		}
		writeJSON(w, 200, map[string]any{"object": "list", "data": data})
		return
	}
	inbound, geminiAlias, pathStream, found := requestProtocol(r.URL.Path)
	if !found {
		writeError(w, codec.OpenAIChat, 404)
		return
	}
	if r.Method != http.MethodPost {
		writeError(w, inbound, 405)
		return
	}
	if r.URL.RawQuery != "" && (inbound != codec.Gemini || !pathStream || r.URL.RawQuery != "alt=sse") {
		writeError(w, inbound, 400)
		return
	}
	media, params, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
	if err != nil || media != "application/json" || len(params) > 1 || (len(params) == 1 && !strings.EqualFold(params["charset"], "utf-8")) {
		writeError(w, inbound, 415)
		return
	}
	if r.ContentLength > 8<<20 {
		writeError(w, inbound, 413)
		return
	}
	body, err := io.ReadAll(http.MaxBytesReader(w, r.Body, 8<<20))
	if err != nil {
		var large *http.MaxBytesError
		if errors.As(err, &large) {
			writeError(w, inbound, 413)
		} else {
			writeError(w, inbound, 400)
		}
		return
	}
	if httpjson.Validate(body) != nil {
		writeError(w, inbound, 400)
		return
	}
	var envelope map[string]json.RawMessage
	if json.Unmarshal(body, &envelope) != nil {
		writeError(w, inbound, 400)
		return
	}
	// These are transport controls, not opaque provider extensions. Go DTO
	// decoding is otherwise case-insensitive; never let a casing alias choose
	// a different model/stream mode after this exact envelope was admitted.
	for key := range envelope {
		if (strings.EqualFold(key, "model") && key != "model") || (strings.EqualFold(key, "stream") && key != "stream") {
			writeError(w, inbound, 400)
			return
		}
	}
	alias := geminiAlias
	stream := pathStream
	if inbound != codec.Gemini {
		if json.Unmarshal(envelope["model"], &alias) != nil || !validModel(alias) {
			writeError(w, inbound, 400)
			return
		}
		if value, present := envelope["stream"]; present {
			var flag *bool
			if json.Unmarshal(value, &flag) != nil || flag == nil {
				writeError(w, inbound, 400)
				return
			}
			stream = *flag
		}
	} else if _, present := envelope["stream"]; present {
		writeError(w, inbound, 400)
		return
	}
	route, ok := s.routes[alias]
	if !ok {
		writeError(w, inbound, 404)
		return
	}
	exchange, err := codec.NewExchange(inbound, route.protocol, codec.Options{Model: route.model, ResponseModel: alias, DefaultMaxOutputTokens: route.maxTokens, Stream: stream})
	if err != nil {
		writeError(w, inbound, 400)
		return
	}
	converted, err := exchange.Request(ctx, body)
	if err != nil {
		writeError(w, inbound, 400)
		return
	}
	if err = s.observation(ctx, "request", alias, converted); err != nil {
		writeError(w, inbound, 500)
		return
	}
	w.Header().Set("X-Alva-Request-Converter", converted.Converter)
	w.Header().Set("X-Alva-Request-Quality", converted.Quality)
	w.Header().Set("X-Alva-Request-Steps", strconv.Itoa(len(converted.Steps)))
	if stream {
		s.stream(w, r, inbound, alias, route, exchange, converted.Body)
		return
	}
	response, err := route.transport.CompleteRaw(ctx, converted.Body)
	if err != nil {
		writeError(w, inbound, 502)
		return
	}
	result, err := exchange.Response(ctx, response)
	if err != nil {
		writeError(w, inbound, 502)
		return
	}
	if err = s.observation(ctx, "response", alias, result); err != nil {
		writeError(w, inbound, 500)
		return
	}
	if len(result.Body) > 8<<20 {
		writeError(w, inbound, 502)
		return
	}
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Header().Set("X-Alva-Response-Converter", result.Converter)
	w.Header().Set("X-Alva-Response-Steps", strconv.Itoa(len(result.Steps)))
	_ = http.NewResponseController(w).SetWriteDeadline(time.Now().Add(30 * time.Second))
	w.WriteHeader(200)
	_, _ = w.Write(result.Body)
}

func requestProtocol(path string) (codec.Protocol, string, bool, bool) {
	switch path {
	case "/v1/chat/completions":
		return codec.OpenAIChat, "", false, true
	case "/v1/responses":
		return codec.OpenAIResponses, "", false, true
	case "/v1/messages":
		return codec.Anthropic, "", false, true
	}
	if strings.HasPrefix(path, "/v1beta/models/") {
		model, action, ok := strings.Cut(strings.TrimPrefix(path, "/v1beta/models/"), ":")
		if ok && validModel(model) && !strings.Contains(model, "/") && (action == "generateContent" || action == "streamGenerateContent") {
			return codec.Gemini, model, action == "streamGenerateContent", true
		}
	}
	return "", "", false, false
}

func (s *Server) observation(ctx context.Context, stage, alias string, result codec.Result) error {
	if s.observe == nil {
		return nil
	}
	digest := sha256.Sum256(result.Body)
	return s.observe(ctx, Observation{Stage: stage, Alias: alias, From: result.From, To: result.To, Converter: result.Converter, Quality: result.Quality, Steps: append([]codec.Step(nil), result.Steps...), SHA256: hex.EncodeToString(digest[:])})
}
