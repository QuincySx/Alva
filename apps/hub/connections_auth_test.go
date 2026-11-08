// INPUT: Connection edits with and without an API key
// OUTPUT: Optional-key authentication regressions
// POS: [L7 Hub product tests] Pure connection policy; no network or storage
package hub

import "testing"

func TestConnectionWithoutAPIKeySendsNoCredential(t *testing.T) {
	base := ModelConnection{ID: "conn-1"}
	update := ConnectionUpdate{ID: "conn-1", Name: "OpenAI", Protocol: "openai_chat", Endpoint: "https://api.example.com/v1/chat/completions", Auth: "api_key", Enabled: true}

	got, secret, err := base.Apply(update, ConnectionSecrets{})
	if err != nil || got.Auth != "none" || got.HasAPIKey || secret.APIKey != "" {
		t.Fatalf("a connection left without a key was refused or kept key authentication: %+v %v", got, err)
	}

	update.APIKey = "dummy-key-not-a-credential"
	keyed, secret, err := base.Apply(update, ConnectionSecrets{})
	if err != nil || keyed.Auth != "api_key" || !keyed.HasAPIKey || secret.APIKey != update.APIKey {
		t.Fatalf("an entered key was not used: %+v %v", keyed, err)
	}

	clear := update
	clear.APIKey, clear.ClearAPIKey, clear.ExpectedRevision = "", true, keyed.Revision
	cleared, secret, err := keyed.Apply(clear, secret)
	if err != nil || cleared.Auth != "none" || cleared.HasAPIKey || secret.APIKey != "" {
		t.Fatalf("clearing the saved key did not leave a usable keyless connection: %+v %v", cleared, err)
	}
}
