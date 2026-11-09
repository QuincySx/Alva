// INPUT: tokens.json and the committed tokens_gen.go two directories up.
// OUTPUT: Proof the committed Go is exactly what the generator produces from the committed JSON, plus generator error cases.
// POS: [L7 Kit tests] TestTokensGoMatchesJSON runs on every platform without Python or MyGo.
package main

import (
	"os"
	"strings"
	"testing"
)

func TestTokensGoMatchesJSON(t *testing.T) {
	data, err := os.ReadFile("../../tokens.json")
	if err != nil {
		t.Fatal(err)
	}
	want, err := generate(data)
	if err != nil {
		t.Fatal(err)
	}
	got, err := os.ReadFile("../../tokens_gen.go")
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != string(want) {
		t.Fatal("tokens_gen.go is stale: run go generate ./apps/ui/kit/theme and commit it with tokens.json")
	}
}

func TestGeneratorRejectsUnknownTypesAndVersions(t *testing.T) {
	for name, doc := range map[string]string{
		"version":  `{"version":2,"tokens":[]}`,
		"type":     `{"version":1,"tokens":[{"name":"a.b","type":"mystery","value":1}]}`,
		"color":    `{"version":1,"tokens":[{"name":"color.a","type":"color","value":"#12"}]}`,
		"notjson":  `{`,
		"shadow":   `{"version":1,"tokens":[{"name":"shadow.a","type":"shadow","value":{"color":"#1"}}]}`,
		"notcolor": `{"version":1,"tokens":[{"name":"color.a","type":"color","value":5}]}`,
	} {
		if _, err := generate([]byte(doc)); err == nil {
			t.Errorf("%s: accepted", name)
		}
	}
}

func TestIdentCamelCasesSegments(t *testing.T) {
	for in, want := range map[string]string{"font.size.body": "FontSizeBody", "space.1": "Space1", "size.panelHeader": "SizePanelHeader"} {
		if got := ident(in); got != want {
			t.Errorf("%s: %s want %s", in, got, want)
		}
	}
	if !strings.HasPrefix(ident("a"), "A") {
		t.Fatal("not exported")
	}
}
