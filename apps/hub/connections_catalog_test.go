// INPUT: Request endpoint spellings for every supported protocol shape
// OUTPUT: Default same-origin catalog derivation regressions
// POS: [L7 Hub product tests] Pure connection policy; no network or storage
package hub

import "testing"

func TestDefaultCatalogURLStaysOnTheEndpointOrigin(t *testing.T) {
	for endpoint, want := range map[string]string{
		"https://api.openai.com/v1/responses":                                             "https://api.openai.com/v1/models",
		"https://api.deepseek.com/chat/completions":                                       "https://api.deepseek.com/models",
		"https://api.deepseek.com/v1/chat/completions":                                    "https://api.deepseek.com/v1/models",
		"http://127.0.0.1:11434/v1/completions":                                           "http://127.0.0.1:11434/v1/models",
		"https://api.anthropic.com/v1/messages":                                           "https://api.anthropic.com/v1/models",
		"https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent": "https://generativelanguage.googleapis.com/v1beta/models",
		"https://gateway.example/openai/v1/chat/completions":                              "https://gateway.example/openai/v1/models",
		"https://api.example/v1/generate":                                                 "",
		"https://api.example/v1/chat/completions?key=1":                                   "",
		"https://user:secret@api.example/v1/chat/completions":                             "",
		"ftp://api.example/v1/chat/completions":                                           "",
		"":                                                                                "",
	} {
		if got := DefaultCatalogURL(endpoint); got != want {
			t.Errorf("DefaultCatalogURL(%q) = %q, want %q", endpoint, got, want)
		}
	}
	// Every derived catalog must satisfy the same-origin rule it exists for.
	connection := ModelConnection{ID: "c", Revision: 1, Name: "c", Protocol: "openai_chat", Auth: "none", Endpoint: "https://api.deepseek.com/chat/completions"}
	connection.CatalogURL = DefaultCatalogURL(connection.Endpoint)
	if err := connection.Validate(); err != nil {
		t.Fatal("derived catalog rejected by the connection policy", err)
	}
}
