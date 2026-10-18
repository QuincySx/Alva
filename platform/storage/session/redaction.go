// INPUT: bytes, encoding/base64/json, errors, io, sort, strings, unicode/utf8, kernel/abi.
// OUTPUT: RedactRecord, RedactJSON, RedactStreamFrame and explicit EnvironmentSecrets snapshots.
// POS: [platform / durable storage] Shared memory/durable journal policy removes known credential fields without truncating ordinary payloads.
package session

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

// This policy is intentionally explicit, not a claim to discover arbitrary
// secrets in prose or encrypt raw user-owned uploaded bytes.
func RedactJSON(data json.RawMessage, configuredValues ...string) (json.RawMessage, error) {
	known, err := prepareSecrets(configuredValues)
	if err != nil {
		return nil, err
	}
	return redactJSONPrepared(data, known)
}
func redactJSONPrepared(data json.RawMessage, known []string) (json.RawMessage, error) {
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	var value any
	if err := decoder.Decode(&value); err != nil {
		return nil, err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return nil, errors.New("redaction requires exactly one JSON value")
	}
	value = redactValue(value, "", known...)
	return json.Marshal(value)
}
func RedactRecord(record *abi.SessionRecord) error {
	data, err := json.Marshal(record)
	if err != nil {
		return err
	}
	data, err = RedactJSON(data)
	if err != nil {
		return err
	}
	return json.Unmarshal(data, record)
}

// RedactStreamFrame applies only to explicit protocol text/base64 I/O
// envelopes. It never recursively decodes arbitrary artifact/business base64.
// Upstream must frame complete bounded lines (or clearly marked EOF/limit
// partials), not arbitrary Read chunks that split a known credential marker.
func RedactStreamFrame(raw json.RawMessage, configuredValues ...string) (json.RawMessage, error) {
	known, err := prepareSecrets(configuredValues)
	if err != nil {
		return nil, err
	}
	const maxBody = 16 << 20
	if len(raw) > (24<<20)+4096 {
		return nil, errors.New("stream frame exceeds bounded redaction limit")
	}
	var frame map[string]json.RawMessage
	if !utf8.Valid(raw) || json.Unmarshal(raw, &frame) != nil || frame == nil {
		return nil, errors.New("stream frame must be a valid UTF-8 JSON object")
	}
	textValue, textExists := frame["text"]
	base64Value, base64Exists := frame["base64"]
	if textExists == base64Exists {
		return nil, errors.New("stream frame requires exactly one text/base64 body")
	}
	var encoded string
	var body []byte
	encoding := "utf-8"
	if textExists {
		if bytes.Equal(bytes.TrimSpace(textValue), []byte("null")) || json.Unmarshal(textValue, &encoded) != nil {
			return nil, errors.New("stream text must be a string")
		}
		body = []byte(encoded)
	} else {
		encoding = "base64"
		if bytes.Equal(bytes.TrimSpace(base64Value), []byte("null")) || json.Unmarshal(base64Value, &encoded) != nil || len(encoded) > ((maxBody+2)/3)*4+8 {
			return nil, errors.New("stream base64 body is invalid or oversized")
		}
		var err error
		body, err = base64.StdEncoding.Strict().DecodeString(encoded)
		if err != nil {
			return nil, errors.New("stream base64 body is malformed")
		}
	}
	if len(body) > maxBody {
		return nil, errors.New("decoded stream body exceeds budget")
	}
	redacted := false
	var prior bool
	if json.Unmarshal(frame["redacted"], &prior) == nil {
		redacted = prior
	}
	processing := "Original captured body bytes preserved; known credential markers checked; no arbitrary-secret detection or media transformation"
	var truncated bool
	var complete bool
	partialConfigured := json.Unmarshal(frame["complete"], &complete) == nil && !complete && len(known) > 0
	if json.Unmarshal(frame["truncated"], &truncated) == nil && truncated || partialConfigured {
		frame["omitted_source_bytes"], _ = json.Marshal(len(body))
		body = []byte("[OMITTED: incomplete/truncated source body cannot be safely credential-redacted]")
		redacted = true
		processing = "Source exceeded its framing limit or ended with an incomplete configured-credential frame; entire captured prefix omitted to avoid retaining a partial configured credential; not complete/original body bytes"
	} else if utf8.Valid(body) && json.Valid(body) {
		decoder := json.NewDecoder(bytes.NewReader(body))
		decoder.UseNumber()
		var value any
		if err := decoder.Decode(&value); err != nil {
			return nil, err
		}
		original, _ := json.Marshal(value)
		clean, err := redactJSONPrepared(body, known)
		if err != nil {
			return nil, err
		}
		if !bytes.Equal(original, clean) {
			body = clean
			redacted = true
			processing = "Known credential fields/markers redacted from structured JSON; JSON formatting canonicalized; not original bytes"
		}
	} else if hasKnownStreamMarker(body) {
		body = []byte("[REDACTED]")
		redacted = true
		processing = "Known plaintext credential marker found in raw stream bytes; entire body redacted, including invalid UTF-8/binary; secret bytes not retained"
	} else {
		clean := string(body)
		clean = redactLiterals(clean, known)
		if !bytes.Equal(body, []byte(clean)) {
			body = []byte(clean)
			redacted = true
			processing = "Explicit configured credential values redacted from captured body; not original bytes; no arbitrary-secret detection"
		}
	}
	var value []byte
	if textExists {
		value, _ = json.Marshal(string(body))
		frame["text"] = value
	} else {
		value, _ = json.Marshal(base64.StdEncoding.EncodeToString(body))
		frame["base64"] = value
	}
	frame["encoding"], _ = json.Marshal(encoding)
	frame["redacted"], _ = json.Marshal(redacted)
	frame["processing"], _ = json.Marshal(processing)
	data, err := json.Marshal(frame)
	if err != nil {
		return nil, err
	}
	return RedactJSON(data)
}

// Bytes are ASCII-folded without attempting lossy UTF-8 decoding. Known JSON
// key markers are recognized even in incomplete/invalid-UTF8 JSON fragments,
// with whitespace before ':' and escaped key spelling handled explicitly.
func hasKnownStreamMarker(body []byte) bool {
	lower := append([]byte(nil), body...)
	for i, value := range lower {
		if value >= 'A' && value <= 'Z' {
			lower[i] = value + ('a' - 'A')
		}
	}
	for _, marker := range []string{"authorization: bearer ", "authorization=bearer ", "api_key=", "apikey=", "access_token=", "refresh_token=", "client_secret=", "password=", "cookie:", "set-cookie:"} {
		if bytes.Contains(lower, []byte(marker)) {
			return true
		}
	}
	for start := 0; start < len(body); start++ {
		if body[start] != '"' {
			continue
		}
		end := start + 1
		for end < len(body) {
			if body[end] == '\\' {
				end += 2
				continue
			}
			if body[end] == '"' {
				break
			}
			end++
		}
		if end >= len(body) {
			break
		}
		after := end + 1
		for after < len(body) && (body[after] == ' ' || body[after] == '\t' || body[after] == '\r' || body[after] == '\n') {
			after++
		}
		if after < len(body) && body[after] == ':' {
			var key string
			if json.Unmarshal(body[start:end+1], &key) == nil && secretKey(key) {
				return true
			}
		}
		start = end
	}
	return false
}
func secretKey(key string) bool {
	normalized := strings.NewReplacer("-", "_", ".", "_").Replace(strings.ToLower(key))
	return normalized == "authorization" || normalized == "cookie" || normalized == "set_cookie" || strings.Contains(normalized, "api_key") || normalized == "apikey" || strings.Contains(normalized, "access_token") || strings.Contains(normalized, "refresh_token") || strings.Contains(normalized, "client_secret") || normalized == "password" || strings.HasSuffix(normalized, "_password") || strings.HasSuffix(normalized, "_secret")
}
func redactValue(value any, key string, known ...string) any {
	if secretKey(key) {
		return "[REDACTED]"
	}
	switch typed := value.(type) {
	case map[string]any:
		for key, child := range typed {
			typed[key] = redactValue(child, key, known...)
		}
	case []any:
		for index, child := range typed {
			typed[index] = redactValue(child, "", known...)
		}
	case string:
		// A journal text field may itself contain a complete JSON body. Keep
		// its ordinary fields and do not re-mask an already sanitized body
		// wholesale merely because a redacted credential key remains present.
		// This unwraps JSON text only, never arbitrary encoded binary/base64.
		data := []byte(typed)
		if json.Valid(data) {
			decoder := json.NewDecoder(bytes.NewReader(data))
			decoder.UseNumber()
			var nested any
			if decoder.Decode(&nested) == nil {
				original, _ := json.Marshal(nested)
				clean, _ := json.Marshal(redactValue(nested, "", known...))
				if !bytes.Equal(original, clean) {
					return string(clean)
				}
				return typed
			}
		}
		if hasKnownStreamMarker(data) {
			return "[REDACTED]"
		}
		return redactLiterals(typed, known)
	case json.Number:
		for _, secret := range known {
			if strings.Contains(typed.String(), secret) {
				return "[REDACTED]"
			}
		}
	case bool, nil:
		encoded, _ := json.Marshal(typed)
		for _, secret := range known {
			if strings.Contains(string(encoded), secret) {
				return "[REDACTED]"
			}
		}
	}
	return value
}

func redactLiterals(value string, known []string) string {
	if len(known) == 0 {
		return value
	}
	pairs := make([]string, 0, len(known)*2)
	for _, secret := range known {
		pairs = append(pairs, secret, "[REDACTED]")
	}
	// One pass avoids redacting the replacement marker itself when another
	// configured value happens to be a substring of "[REDACTED]".
	return strings.NewReplacer(pairs...).Replace(value)
}

// EnvironmentSecrets accepts only explicitly configured values, never ambient
// os.Environ. Known credential keys include short values; other >=6-byte
// overrides are treated as private too. Output is a new immutable snapshot.
func EnvironmentSecrets(environment map[string]string) []string {
	values := []string{}
	for key, value := range environment {
		if value != "" && (secretKey(key) || len(value) >= 6) {
			values = append(values, value)
		}
	}
	sort.Slice(values, func(i, j int) bool {
		if len(values[i]) != len(values[j]) {
			return len(values[i]) > len(values[j])
		}
		return values[i] < values[j]
	})
	return values
}
func prepareSecrets(values []string) ([]string, error) {
	seen := map[string]bool{}
	add := func(value string) {
		if value != "" {
			seen[value] = true
		}
	}
	for _, value := range values {
		add(value)
		encoded, _ := json.Marshal(value)
		if len(encoded) >= 2 {
			add(string(encoded[1 : len(encoded)-1]))
		}
		for _, line := range strings.FieldsFunc(value, func(r rune) bool { return r == '\n' || r == '\r' }) {
			add(line)
		}
		if len(seen) > 512 {
			return nil, errors.New("configured credential redaction exceeds 512 literal patterns")
		}
	}
	out := make([]string, 0, len(seen))
	for value := range seen {
		out = append(out, value)
	}
	sort.Slice(out, func(i, j int) bool {
		if len(out[i]) != len(out[j]) {
			return len(out[i]) > len(out[j])
		}
		return out[i] < out[j]
	})
	return out, nil
}
