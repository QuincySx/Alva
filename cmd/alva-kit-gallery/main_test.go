// INPUT: Gallery flags.
// OUTPUT: Proof flags select a page and appearance, reject unknown pages and stray arguments, and never open a window.
// POS: [L7 Kit tests] prepare is the whole testable startup path; the window itself needs a display.
//go:build desktop

package main

import (
	"bytes"
	"testing"
)

func TestPrepareSelectsPageAndAppearance(t *testing.T) {
	app, err := prepare([]string{"--page", "Switch", "--dark", "--narrow"}, &bytes.Buffer{})
	if err != nil || app.Pages[app.Index].Name != "Switch" || !app.Dark || !app.Narrow {
		t.Fatal("flags did not reach the gallery", err)
	}
}

func TestPrepareRejectsUnknownPageAndExtraArguments(t *testing.T) {
	for _, args := range [][]string{{"--page", "Nope"}, {"extra"}, {"--bogus"}} {
		if _, err := prepare(args, &bytes.Buffer{}); err == nil {
			t.Fatalf("%v accepted", args)
		}
	}
}
