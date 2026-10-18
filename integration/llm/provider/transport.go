// INPUT: Explicit Config/Kind, bounded JSON/SSE and standard HTTP lifecycle.
// OUTPUT: RawTransport, BuildTransport, CompleteRaw and StreamRaw without model conversion policy.
// POS: [integration / model transport] One credential/endpoint/cancellation transport is shared by SDK models and raw protocol Gateway.
package provider

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"mime"
	"net"
	"net/http"
	"net/url"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/integration/llm/codec"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const (
	defaultEventLimit = 1 << 20
	responseLimit     = 8 << 20
)

// Config names an explicit full native endpoint; credentials never come from
// environment or legacy configuration and cannot be serialized.
type Config struct {
	Endpoint        string            `json:"endpoint"`
	Model           string            `json:"model"`
	APIKey          string            `json:"-"`
	HTTPClient      *http.Client      `json:"-"`
	MaxEventBytes   int               `json:"max_event_bytes,omitempty"`
	MaxOutputTokens int               `json:"max_output_tokens,omitempty"`
	Headers         map[string]string `json:"-"`
}
type RawTransport struct {
	kind   Kind
	config Config
	client *http.Client
}

func BuildTransport(kind Kind, config Config) (*RawTransport, error) {
	if _, err := codec.NewProtocolAdapter(codec.Protocol(kind)); err != nil {
		return nil, err
	}
	if config.MaxOutputTokens < 0 || config.MaxOutputTokens > 1<<20 || len(config.Headers) > 64 {
		return nil, errors.New("provider output/header configuration exceeds limits")
	}
	ownedHeaders := make(map[string]string, len(config.Headers))
	headerBytes := 0
	for key, value := range config.Headers {
		canonical := http.CanonicalHeaderKey(key)
		if canonical == "" || len(key) > 128 || strings.ContainsAny(key, " :\r\n\x00") || strings.ContainsAny(value, "\r\n\x00") {
			return nil, errors.New("invalid explicit model header")
		}
		if _, exists := ownedHeaders[canonical]; exists {
			return nil, errors.New("duplicate case-insensitive model header")
		}
		switch canonical {
		case "Host", "Content-Length", "Transfer-Encoding", "Connection", "Content-Type", "Accept":
			return nil, errors.New("transport-controlled model header")
		}
		headerBytes += len(key) + len(value)
		if headerBytes > 32<<10 {
			return nil, errors.New("explicit model headers exceed 32 KiB")
		}
		ownedHeaders[canonical] = value
	}
	config.Headers = ownedHeaders
	if len(config.Endpoint) > 4096 || len(config.Model) > 256 || len(config.APIKey) > 8192 || !utf8.ValidString(config.Model) || strings.ContainsRune(config.Model, 0) {
		return nil, errors.New("model endpoint, model or API key exceeds configuration limits")
	}
	endpoint, err := url.Parse(config.Endpoint)
	if err != nil || endpoint.Host == "" || (endpoint.Scheme != "https" && endpoint.Scheme != "http") || endpoint.User != nil || endpoint.Fragment != "" || endpoint.RawQuery != "" {
		return nil, errors.New("explicit HTTP(S) endpoint without user info, query or fragment required")
	}
	if strings.TrimSpace(config.Model) == "" {
		return nil, errors.New("explicit model required")
	}
	if endpoint.Scheme == "http" && endpoint.Hostname() != "localhost" {
		address := net.ParseIP(endpoint.Hostname())
		if address == nil || !address.IsLoopback() {
			return nil, errors.New("non-loopback model endpoints require HTTPS")
		}
	}
	if strings.ContainsAny(config.APIKey, "\r\n") {
		return nil, errors.New("invalid API key")
	}
	if config.MaxEventBytes == 0 {
		config.MaxEventBytes = defaultEventLimit
	}
	if config.MaxEventBytes < 256 || config.MaxEventBytes > defaultEventLimit {
		return nil, errors.New("SSE event limit must be between 256 bytes and 1 MiB")
	}
	var copyClient http.Client
	if config.HTTPClient != nil {
		copyClient = *config.HTTPClient
	} else {
		copyClient.Timeout = 10 * time.Minute
	}
	if copyClient.Transport == nil {
		transport := &http.Transport{
			DialContext:       (&net.Dialer{Timeout: 30 * time.Second, KeepAlive: 30 * time.Second}).DialContext,
			ForceAttemptHTTP2: true, MaxIdleConns: 100, IdleConnTimeout: 90 * time.Second,
			TLSHandshakeTimeout: 10 * time.Second, ExpectContinueTimeout: time.Second,
		}
		if defaults, ok := http.DefaultTransport.(*http.Transport); ok {
			transport = defaults.Clone()
		}
		// A client that only sets a timeout still uses our proxy-free default.
		// Proxies require an explicitly supplied Config.HTTPClient.Transport.
		transport.Proxy = nil
		copyClient.Transport = transport
	}
	copyClient.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &RawTransport{kind: kind, config: config, client: &copyClient}, nil
}
func (t *RawTransport) endpoint(stream bool) string {
	if t.kind != GeminiKind {
		return t.config.Endpoint
	}
	parsed, _ := url.Parse(t.config.Endpoint)
	// Only standard operation suffixes are interchangeable; custom endpoints
	// stay explicit and never change authority/model path.
	for _, operation := range []string{":generateContent", ":streamGenerateContent"} {
		if strings.HasSuffix(parsed.Path, operation) {
			target := ":generateContent"
			if stream {
				target = ":streamGenerateContent"
			}
			parsed.Path = strings.TrimSuffix(parsed.Path, operation) + target
			parsed.RawPath = ""
			break
		}
	}
	if stream {
		query := parsed.Query()
		query.Set("alt", "sse")
		parsed.RawQuery = query.Encode()
	}
	return parsed.String()
}
func (t *RawTransport) request(ctx context.Context, raw json.RawMessage, stream bool) (*http.Response, error) {
	if len(raw) > responseLimit || !wire.ValidJSON(raw) {
		return nil, errors.New("model request JSON invalid or exceeds 8 MiB")
	}
	var object map[string]json.RawMessage
	if json.Unmarshal(raw, &object) != nil || object == nil {
		return nil, errors.New("model request JSON object required")
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, t.endpoint(stream), bytes.NewReader(raw))
	if err != nil {
		return nil, errors.New("could not build model request")
	}
	request.Header.Set("Content-Type", "application/json")
	accept := "application/json"
	if stream {
		accept = "text/event-stream"
	}
	request.Header.Set("Accept", accept)
	switch t.kind {
	case AnthropicKind:
		request.Header.Set("Anthropic-Version", "2023-06-01")
		if t.config.APIKey != "" {
			request.Header.Set("X-Api-Key", t.config.APIKey)
		}
	case GeminiKind:
		if t.config.APIKey != "" {
			request.Header.Set("X-Goog-Api-Key", t.config.APIKey)
		}
	default:
		if t.config.APIKey != "" {
			request.Header.Set("Authorization", "Bearer "+t.config.APIKey)
		}
	}
	for name, value := range t.config.Headers {
		request.Header.Set(name, value)
	}
	response, err := t.client.Do(request)
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, networkError(err)
	}
	if response.StatusCode != http.StatusOK {
		defer response.Body.Close()
		return nil, statusError(response)
	}
	mediaType, _, mediaErr := mime.ParseMediaType(response.Header.Get("Content-Type"))
	if mediaErr != nil || !strings.EqualFold(mediaType, accept) {
		response.Body.Close()
		return nil, errors.New("model response has unexpected content type")
	}
	return response, nil
}
func (t *RawTransport) CompleteRaw(ctx context.Context, raw json.RawMessage) (json.RawMessage, error) {
	response, err := t.request(ctx, raw, false)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()
	body, readErr := io.ReadAll(io.LimitReader(response.Body, responseLimit+1))
	if err = ctx.Err(); err != nil {
		if readErr != nil {
			readErr = &responseReadError{cause: readErr}
		}
		return nil, errors.Join(err, readErr)
	}
	if readErr != nil {
		return nil, &responseReadError{cause: readErr}
	}
	if len(body) > responseLimit || !wire.ValidJSON(body) {
		return nil, errors.New("model response JSON invalid or exceeds 8 MiB")
	}
	var object map[string]json.RawMessage
	if json.Unmarshal(body, &object) != nil || object == nil {
		return nil, errors.New("model response JSON object required")
	}
	return body, nil
}

// consume may stop only after its own legal native terminal. A simultaneous
// callback error always wins; bool true never erases that error.
func (t *RawTransport) StreamRaw(ctx context.Context, raw json.RawMessage, consume func(codec.Event) (bool, error)) error {
	if consume == nil {
		return errors.New("raw stream callback required")
	}
	response, err := t.request(ctx, raw, true)
	if err != nil {
		return err
	}
	defer response.Body.Close()
	return readSSE(ctx, response.Body, t.config.MaxEventBytes, consume)
}
func readSSE(ctx context.Context, reader io.Reader, limit int, consume func(codec.Event) (bool, error)) error {
	observed := &observedReader{reader: reader, maximum: responseLimit}
	buffered := bufio.NewReaderSize(observed, 4096)
	skipLF := false
	readLine := func() (string, error) {
		var line strings.Builder
		for {
			value, err := buffered.ReadByte()
			if err != nil {
				if errors.Is(err, io.EOF) && line.Len() > 0 {
					return line.String(), nil
				}
				return "", err
			}
			if skipLF {
				skipLF = false
				if value == '\n' {
					continue
				}
			}
			if value == '\r' {
				skipLF = true
				return line.String(), nil
			}
			if value == '\n' {
				return line.String(), nil
			}
			if line.Len() >= limit {
				return "", errors.New("model SSE line exceeds limit")
			}
			line.WriteByte(value)
		}
	}
	var data []string
	name := ""
	eventBytes := 0
	dispatch := func() (bool, error) {
		if len(data) == 0 {
			name = ""
			eventBytes = 0
			return false, nil
		}
		joined := []byte(strings.Join(data, "\n"))
		event := codec.Event{Name: name, Data: joined}
		data = nil
		name = ""
		eventBytes = 0
		if !utf8.Valid(joined) || !utf8.ValidString(event.Name) {
			return false, errors.New("model SSE contains invalid UTF-8")
		}
		done, err := consume(event)
		return done, errors.Join(err, ctx.Err(), observed.failure)
	}
	for {
		if err := ctx.Err(); err != nil {
			return err
		}
		line, readErr := readLine()
		if readErr != nil {
			if err := errors.Join(ctx.Err(), observed.failure); err != nil {
				return err
			}
			if !errors.Is(readErr, io.EOF) {
				return &responseReadError{cause: readErr}
			}
			if len(data) > 0 {
				return errors.New("model SSE ended inside an event")
			}
			return nil
		}
		if line == "" {
			done, err := dispatch()
			if err != nil {
				return err
			}
			if done {
				return nil
			}
			continue
		}
		if strings.HasPrefix(line, ":") {
			continue
		}
		if strings.HasPrefix(line, "event:") {
			name = strings.TrimPrefix(strings.TrimPrefix(line, "event:"), " ")
			if len(name) > 256 {
				return errors.New("model SSE event name exceeds limit")
			}
			continue
		}
		if strings.HasPrefix(line, "data:") {
			value := strings.TrimPrefix(strings.TrimPrefix(line, "data:"), " ")
			eventBytes += len(value) + 1
			if eventBytes > limit {
				return errors.New("model SSE event exceeds limit")
			}
			data = append(data, value)
		}
	}
}

type observedReader struct {
	reader         io.Reader
	failure        error
	bytes, maximum int
}

func (r *observedReader) Read(p []byte) (int, error) {
	if r.failure != nil {
		return 0, r.failure
	}
	if len(p) > r.maximum-r.bytes+1 {
		p = p[:r.maximum-r.bytes+1]
	}
	n, err := r.reader.Read(p)
	r.bytes += n
	if r.bytes > r.maximum {
		r.failure = errors.New("model SSE exceeds 8 MiB")
	}
	if err != nil && !errors.Is(err, io.EOF) {
		r.failure = errors.Join(r.failure, &responseReadError{cause: err})
	}
	if r.failure != nil {
		return n, r.failure
	}
	return n, err
}

type responseReadError struct{ cause error }

func (*responseReadError) Error() string   { return "model response read failed" }
func (e *responseReadError) Unwrap() error { return e.cause }
