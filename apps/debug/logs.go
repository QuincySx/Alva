// INPUT: Explicit diagnostic slog events, runtime level selection and configured credential literals
// OUTPUT: Bounded monotonic LogPage and slog.Handler with private-field redaction
// POS: [L6 diagnostics] No global stdout/provider capture, raw action arguments or ambient secret discovery.
package debug

import (
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/QuincySx/Alva/platform/storage/session"
)

type LogRecord struct {
	Sequence uint64          `json:"sequence"`
	Time     time.Time       `json:"time"`
	Level    string          `json:"level"`
	Data     json.RawMessage `json:"data"`
}
type LogQuery struct {
	After, HighWater uint64
	Level, Keyword   string
	Limit            int
}
type LogPage struct {
	Records   []LogRecord `json:"records"`
	HighWater uint64      `json:"high_water"`
	Oldest    uint64      `json:"oldest"`
	Next      uint64      `json:"next"`
	Dropped   bool        `json:"dropped"`
	HasMore   bool        `json:"has_more"`
}
type logStore struct {
	mu      sync.Mutex
	records []LogRecord
	next    uint64
	level   slog.LevelVar
	secrets []string
	stopped atomic.Bool
}
type logHandler struct {
	store  *logStore
	attrs  []boundLogAttr
	groups []string
}
type boundLogAttr struct {
	groups []string
	attr   slog.Attr
}

func (s *logStore) logger() *slog.Logger { return slog.New(&logHandler{store: s}) }
func (s *logStore) setLevel(level string) error {
	var selected slog.Level
	switch level {
	case "debug":
		selected = slog.LevelDebug
	case "info":
		selected = slog.LevelInfo
	case "warn":
		selected = slog.LevelWarn
	case "error":
		selected = slog.LevelError
	default:
		return errors.New("level must be debug, info, warn or error")
	}
	s.level.Set(selected)
	return nil
}
func (h *logHandler) Enabled(_ context.Context, level slog.Level) bool {
	return !h.store.stopped.Load() && level >= h.store.level.Level()
}
func (h *logHandler) WithAttrs(attrs []slog.Attr) slog.Handler {
	bound := append([]boundLogAttr(nil), h.attrs...)
	for _, attr := range attrs {
		if len(bound) >= 32 {
			break
		}
		bound = append(bound, boundLogAttr{groups: append([]string(nil), h.groups...), attr: attr})
	}
	return &logHandler{store: h.store, attrs: bound, groups: append([]string(nil), h.groups...)}
}
func (h *logHandler) WithGroup(name string) slog.Handler {
	groups := append([]string(nil), h.groups...)
	if name != "" && len(name) <= 128 && len(groups) < 4 {
		groups = append(groups, privateLogKey(name, h.store.secrets))
	}
	return &logHandler{store: h.store, attrs: append([]boundLogAttr(nil), h.attrs...), groups: groups}
}

func privateLogKey(key string, secrets []string) string {
	for _, secret := range secrets {
		if secret != "" && strings.Contains(key, secret) {
			return "[REDACTED_KEY]"
		}
	}
	return key
}
func logValue(value slog.Value, depth int, secrets []string) any {
	value = value.Resolve()
	if depth > 4 {
		return "[log group omitted]"
	}
	switch value.Kind() {
	case slog.KindString:
		if len(value.String()) > 8192 {
			return "[over-budget log value omitted]"
		}
		return value.String()
	case slog.KindBool:
		return value.Bool()
	case slog.KindInt64:
		return value.Int64()
	case slog.KindUint64:
		return value.Uint64()
	case slog.KindFloat64:
		return value.Float64()
	case slog.KindDuration:
		return value.Duration().String()
	case slog.KindTime:
		return value.Time()
	case slog.KindGroup:
		group := make(map[string]any)
		for i, attr := range value.Group() {
			if i >= 32 {
				break
			}
			if len(attr.Key) <= 128 {
				group[privateLogKey(attr.Key, secrets)] = logValue(attr.Value, depth+1, secrets)
			}
		}
		return group
	default:
		return "[unstructured log attribute omitted]"
	}
}
func (h *logHandler) Handle(_ context.Context, record slog.Record) error {
	if h.store.stopped.Load() {
		return nil
	}
	message := record.Message
	// Omit oversized original values whole. Truncating before literal redaction
	// would leave credential prefixes, repeating the native stream framing bug.
	if len(message) > 8192 {
		message = "[over-budget log message omitted]"
	}
	attrs := make(map[string]any)
	count := 0
	add := func(groups []string, attr slog.Attr) bool {
		if count >= 32 {
			return false
		}
		count++
		if len(attr.Key) <= 128 {
			parent := attrs
			for _, group := range groups {
				nested, _ := parent[group].(map[string]any)
				if nested == nil {
					nested = make(map[string]any)
					parent[group] = nested
				}
				parent = nested
			}
			value := logValue(attr.Value, 0, h.store.secrets)
			if group, ok := value.(map[string]any); ok && attr.Key == "" {
				for key, value := range group {
					parent[key] = value
				}
			} else {
				parent[privateLogKey(attr.Key, h.store.secrets)] = value
			}
		}
		return true
	}
	for _, attr := range h.attrs {
		if !add(attr.groups, attr.attr) {
			break
		}
	}
	record.Attrs(func(attr slog.Attr) bool { return add(h.groups, attr) })
	raw, err := json.Marshal(map[string]any{"message": message, "attributes": attrs})
	if err != nil || len(raw) > 32<<10 {
		raw = []byte(`{"message":"log metadata exceeded budget; body omitted"}`)
	}
	redacted, err := session.RedactJSON(raw, h.store.secrets...)
	if err != nil {
		return errors.New("diagnostic log redaction failed")
	}
	if len(redacted) > 8192 {
		redacted = []byte(`{"message":"redacted log exceeds display budget; body omitted"}`)
	}
	h.store.mu.Lock()
	defer h.store.mu.Unlock()
	h.store.next++
	item := LogRecord{Sequence: h.store.next, Time: record.Time, Level: strings.ToLower(record.Level.String()), Data: redacted}
	if len(h.store.records) == 512 {
		copy(h.store.records, h.store.records[1:])
		h.store.records[len(h.store.records)-1] = item
	} else {
		h.store.records = append(h.store.records, item)
	}
	return nil
}

func (s *logStore) page(query LogQuery) (LogPage, error) {
	if query.Limit == 0 {
		query.Limit = 8
	}
	if query.Limit < 1 || query.Limit > 8 || len(query.Keyword) > 256 {
		return LogPage{}, errors.New("bounded diagnostic log query required")
	}
	if query.Level != "" && query.Level != "debug" && query.Level != "info" && query.Level != "warn" && query.Level != "error" {
		return LogPage{}, errors.New("invalid log level filter")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	high := query.HighWater
	if high == 0 {
		high = s.next
	}
	if high > s.next || query.After > high {
		return LogPage{}, errors.New("log cursor exceeds observed high-water")
	}
	page := LogPage{HighWater: high, Next: query.After, Records: []LogRecord{}}
	if len(s.records) > 0 {
		page.Oldest = s.records[0].Sequence
		page.Dropped = query.After+1 < page.Oldest
	}
	for _, record := range s.records {
		if record.Sequence <= query.After || record.Sequence > high {
			continue
		}
		if query.Level != "" && record.Level != query.Level || query.Keyword != "" && !strings.Contains(string(record.Data), query.Keyword) {
			continue
		}
		if len(page.Records) == query.Limit {
			page.HasMore = true
			break
		}
		record.Data = append(json.RawMessage(nil), record.Data...)
		page.Records = append(page.Records, record)
		page.Next = record.Sequence
	}
	return page, nil
}
