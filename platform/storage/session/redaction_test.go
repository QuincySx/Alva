// INPUT: context, encoding/json, strings, testing, kernel/abi, llm/wire.
// OUTPUT: Canonical known-secret redaction, immutable inputs and original artifact limit regressions.
// POS: [platform / durable storage] Native fact privacy is distinct from raw explicitly uploaded artifact preservation.
package session

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/redaction"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestConfiguredLiteralValuesAreRedactedAcrossStreamRepresentations(t *testing.T) {
	const secret = "opaque-credential-without-key"
	quoted := "credential-\"quote\\slash"
	for _, test := range []struct {
		name    string
		body    []byte
		encoded bool
		secrets []string
	}{
		{"unlabelled", []byte("before " + secret + " after\n"), false, []string{secret}},
		{"binary envelope", append([]byte{0xff}, []byte(secret)...), true, []string{secret}},
		{"escaped JSON string", []byte(fmt.Sprintf(`{"message":%q,"safe":"kept"}`, quoted)), false, []string{quoted}},
		{"JSON text inside JSON", []byte(fmt.Sprintf(`{"message":%q,"safe":"kept"}`, `{"message":"`+secret+`"}`)), false, []string{secret}},
		{"JSON number", []byte(`{"message":12345678901234567890,"safe":"kept"}`), false, []string{"12345678901234567890"}},
		{"JSON boolean", []byte(`{"message":true,"safe":"kept"}`), false, []string{"true"}},
		{"multiline short component", []byte("ab\n"), false, []string{"ab\nlonger-component"}},
		{"overlapping values", []byte("credential-long credential"), false, []string{"credential", "credential-long", "REDACTED"}},
	} {
		t.Run(test.name, func(t *testing.T) {
			frame := map[string]any{}
			if test.encoded {
				frame["base64"] = base64.StdEncoding.EncodeToString(test.body)
			} else {
				frame["text"] = string(test.body)
			}
			raw, _ := json.Marshal(frame)
			original := append([]byte(nil), raw...)
			clean, err := redaction.RedactStreamFrame(raw, test.secrets...)
			if err != nil {
				t.Fatal(err)
			}
			var result struct {
				Text, Base64 string
				Redacted     bool
			}
			if json.Unmarshal(clean, &result) != nil || !result.Redacted || !bytes.Equal(raw, original) {
				t.Fatal("redaction or immutable input lost", string(clean))
			}
			body := []byte(result.Text)
			if test.encoded {
				body, err = base64.StdEncoding.DecodeString(result.Base64)
				if err != nil {
					t.Fatal(err)
				}
			}
			if !bytes.Contains(body, []byte("[REDACTED]")) || bytes.Contains(body, []byte(secret)) || bytes.Contains(body, []byte(quoted)) || bytes.Contains(body, []byte("[[REDACTED]]")) {
				t.Fatal("configured value retained or replacement recursively rewritten", string(body))
			}
			if bytes.Contains(test.body, []byte(`"safe"`)) && !bytes.Contains(body, []byte(`"safe":"kept"`)) {
				t.Fatal("ordinary field lost", string(body))
			}
		})
	}
}

func TestTruncatedCaptureOmitsCredentialPrefixInsteadOfClaimingOriginalBytes(t *testing.T) {
	for _, raw := range []json.RawMessage{json.RawMessage(`{"text":"partial-configured-cred","truncated":true,"complete":false}`), json.RawMessage(`{"text":"partial-configured-cred","complete":false}`)} {
		clean, err := redaction.RedactStreamFrame(raw, "partial-configured-credential")
		if err != nil {
			t.Fatal(err)
		}
		var result struct {
			Text, Processing string
			Redacted         bool
			Omitted          int `json:"omitted_source_bytes"`
		}
		if json.Unmarshal(clean, &result) != nil || !result.Redacted || strings.Contains(result.Text, "partial-configured") || result.Omitted != len("partial-configured-cred") || !strings.Contains(result.Processing, "prefix omitted") {
			t.Fatal("partial secret retained or completeness hidden", string(clean))
		}
	}
}

func TestEnvironmentSecretsSnapshotIsExplicitAndExcessPatternsFailClosed(t *testing.T) {
	t.Setenv("ALVA_AMBIENT_CREDENTIAL", "ambient-must-not-be-discovered")
	env := map[string]string{"API_KEY": "x", "CUSTOM": "private-override", "LANG": "C", "EMPTY": ""}
	values := redaction.EnvironmentSecrets(env)
	env["CUSTOM"] = "changed-after-snapshot"
	if len(values) != 2 || values[0] != "private-override" || values[1] != "x" {
		t.Fatal("explicit immutable deterministic snapshot lost", values)
	}
	clean, err := redaction.RedactJSON(json.RawMessage(`{"message":"ambient-must-not-be-discovered"}`), values...)
	if err != nil || !bytes.Contains(clean, []byte("ambient-must-not-be-discovered")) {
		t.Fatal("ambient environment read", string(clean), err)
	}
	values = nil
	for i := 0; i < 513; i++ {
		values = append(values, fmt.Sprintf("opaque-%04d", i))
	}
	if _, err := redaction.RedactJSON(json.RawMessage(`{"message":"safe"}`), values...); err == nil {
		t.Fatal("excessive credential patterns silently skipped")
	}
}

func TestCommittedFactsCanonicalRedactionDoesNotMutateCaller(t *testing.T) {
	_, log := repoSession(t)
	arguments := json.RawMessage(`{"api_key":"fixture-secret","nested":{"Authorization":"Bearer private","safe":"完整日志🙂"}}`)
	call := wire.ToolCall{ID: "call", Name: "mcp_runtime", Arguments: arguments}
	intent := abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "run", Call: call}})
	payload := json.RawMessage(`{"authorization":"private","number":18446744073709551615,"safe":"日志🙂"}`)
	extension := abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "future", Type: "opaque", SchemaVersion: 37, Payload: payload}
	committed, err := log.AppendRecords(context.Background(), []abi.SessionRecord{intent, extension, conversation("api_key=private")})
	if err != nil {
		t.Fatal(err)
	}
	if string(call.Arguments) != string(arguments) || string(extension.Payload) != string(payload) {
		t.Fatal("caller facts mutated")
	}
	data, _ := json.Marshal(committed)
	if strings.Contains(string(data), "fixture-secret") || strings.Contains(string(data), "Bearer private") || strings.Contains(string(data), "api_key=private") {
		t.Fatal("known credentials persisted")
	}
	if !strings.Contains(string(committed[1].Payload), "18446744073709551615") || !strings.Contains(string(data), "完整日志🙂") || committed[1].SchemaVersion != 37 {
		t.Fatal("ordinary/unknown typed content changed")
	}
	restored, err := log.Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	got, _ := json.Marshal(restored)
	if string(got) != string(data) {
		t.Fatal("memory receipt and durable redaction differ")
	}
}

func TestStreamFrameRedactionKeepsOrdinaryBytesButCoversEncodedAndPartialKeys(t *testing.T) {
	tests := []struct {
		name            string
		body            []byte
		encoded, secret bool
	}{
		{"ordinary unicode line", []byte("完整日志🙂\n"), false, false},
		{"ordinary JSON formatting", []byte("{ \"safe\" : 18446744073709551615 }\n"), false, false},
		{"ordinary binary", []byte{0xff, 0x01, 'a', 'b', 'c', '\n'}, true, false},
		{"known split marker joined by upstream framing", append([]byte("api_"), []byte("key=split-byte-secret\n")...), false, true},
		{"JSON stderr key", []byte("{\"api_key\":\"json-secret\",\"safe\":\"kept\"}\n"), false, true},
		{"partial JSON whitespace key", []byte("{\"api_key\" \t : \"partial-secret"), false, true},
		{"escaped incomplete key", []byte(`{"api\u005fkey" : "escaped-secret`), false, true},
		{"invalid utf8 marker", append([]byte{0xff}, []byte("API_KEY=binary-secret\n")...), true, true},
		{"invalid utf8 JSON key", append([]byte{0xfe}, []byte(`{"API_KEY"  : "binary-json-secret"}`)...), true, true},
		{"encoded valid JSON", []byte(`{"client_secret":"encoded-json-secret","safe":true}`), true, true},
		{"JSON scalar marker", []byte(`"password=scalar-secret"`), false, true},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			frame := map[string]any{"complete": true}
			if test.encoded {
				frame["base64"] = base64.StdEncoding.EncodeToString(test.body)
			} else {
				frame["text"] = string(test.body)
			}
			raw, _ := json.Marshal(frame)
			original := append([]byte(nil), raw...)
			clean, err := redaction.RedactStreamFrame(raw)
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.Equal(raw, original) {
				t.Fatal("borrowed input changed")
			}
			var result struct {
				Text       string `json:"text"`
				Base64     string `json:"base64"`
				Encoding   string `json:"encoding"`
				Redacted   bool   `json:"redacted"`
				Processing string `json:"processing"`
			}
			if json.Unmarshal(clean, &result) != nil {
				t.Fatal("invalid result")
			}
			var body []byte
			if test.encoded {
				body, err = base64.StdEncoding.DecodeString(result.Base64)
				if err != nil {
					t.Fatal(err)
				}
			} else {
				body = []byte(result.Text)
			}
			if test.secret {
				if !result.Redacted || bytes.Contains(body, []byte("-secret")) || !bytes.Contains(body, []byte("[REDACTED]")) {
					t.Fatal("known credential leaked", string(clean), string(body))
				}
				if bytes.Contains(test.body, []byte(`"safe"`)) && !bytes.Contains(body, []byte(`"safe"`)) {
					t.Fatal("structured redaction erased ordinary fields", string(clean), string(body))
				}
			} else if !bytes.Equal(body, test.body) || result.Redacted {
				t.Fatal("ordinary bytes changed", string(clean))
			}
			if result.Encoding == "" || result.Processing == "" {
				t.Fatal("processing hidden")
			}
			for range 3 {
				reclean, err := redaction.RedactJSON(clean)
				if err != nil || !bytes.Equal(reclean, clean) {
					t.Fatal("canonical durable processing changed sanitized frame", string(clean), string(reclean), err)
				}
				clean = reclean
			}
		})
	}
	for _, raw := range []string{`{}`, `{"text":"x","base64":"eA=="}`, `{"base64":"!!!!"}`, `{"text":null}`, `{"base64":null}`, `[]`} {
		if _, err := redaction.RedactStreamFrame(json.RawMessage(raw)); err == nil {
			t.Fatal("invalid IO envelope accepted", raw)
		}
	}
}

func TestKnownJSONTextMarkersAreRedactedWithoutDecodingBusinessBase64(t *testing.T) {
	encoded := base64.StdEncoding.EncodeToString([]byte("api_key=ordinary-base64-business-data"))
	raw, _ := json.Marshal(map[string]any{"text": `{"api_key" : "json-text-secret","safe":"complete-body"}`, "base64_business": encoded})
	clean, err := redaction.RedactJSON(raw)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(clean), "json-text-secret") || !strings.Contains(string(clean), encoded) || !strings.Contains(string(clean), "complete-body") {
		t.Fatal("known plaintext redaction recursed into unrelated base64 or leaked", string(clean))
	}
	if _, err := redaction.RedactJSON(json.RawMessage(`{} {"api_key":"not-ignored"}`)); err == nil {
		t.Fatal("trailing JSON silently omitted from journal sanitization")
	}
}

func TestOriginalArtifactMaximumAndDistinctProcessingMetadata(t *testing.T) {
	repo, _ := repoSession(t)
	ctx := context.Background()
	input := storage.ArtifactInput{Kind: "upload", Name: "video-original.bin", MediaType: "application/octet-stream", Processing: "Original bytes only; no compression, extraction, transcription or model delivery", Data: make([]byte, storage.MaxArtifactBytes)}
	first, err := repo.PutArtifact(ctx, "owner", "native", input)
	if err != nil {
		t.Fatal(err)
	}
	if first.Bytes != storage.MaxArtifactBytes || first.SHA256 != digest(input.Data) {
		t.Fatal("20MiB original/hash changed")
	}
	duplicate, err := repo.PutArtifact(ctx, "owner", "native", input)
	if err != nil || duplicate.ID != first.ID {
		t.Fatal("exact artifact identity lost", err)
	}
	input.Name = "distinct-upload.bin"
	second, err := repo.PutArtifact(ctx, "owner", "native", input)
	if err != nil || second.ID == first.ID || second.Name != input.Name {
		t.Fatal("same bytes erased distinct upload metadata", err)
	}
	end, err := repo.ReadArtifact(ctx, "owner", "native", second.Reference().URI, second.Bytes-5, 10)
	if err != nil || len(end.Data) != 5 {
		t.Fatal("bounded original tail lost", err)
	}
	input.Data = append(input.Data, 1)
	if _, err = repo.PutArtifact(ctx, "owner", "native", input); err == nil {
		t.Fatal("over-limit artifact accepted")
	}
}
