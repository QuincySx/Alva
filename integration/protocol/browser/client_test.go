// INPUT: context, strings, testing.
// OUTPUT: Independent protocol input/endpoint validation without an installed browser or network discovery.
// POS: [integration / external protocol] Invalid CDP and page capabilities fail explicitly before any implicit connection, file access or partial-success result.
package browser

import (
	"context"
	"strings"
	"testing"
)

func TestEndpointNeverDiscoversOrWidensLoopbackAuthority(t *testing.T) {
	for _, raw := range []string{"http://127.0.0.1:9222/", "ws://localhost:9222/devtools/browser/id", "ws://127.0.0.1/devtools/browser/id", "ws://127.0.0.1:0/devtools/browser/id", "ws://127.0.0.1:65536/devtools/browser/id", "ws://127.0.0.1:1234/devtools/page/id", "ws://127.0.0.1:1234/devtools/browser/", "ws://127.0.0.1:1234/devtools/browser/id?token=x", "ws://a:b@127.0.0.1:1234/devtools/browser/id", "ws://192.0.2.1:1234/devtools/browser/id"} {
		if err := ValidateEndpoint(raw); err == nil {
			t.Fatal("invalid browser endpoint accepted", raw)
		}
	}
	for _, raw := range []string{"ws://127.0.0.1:1234/devtools/browser/id", "ws://[::1]:1234/devtools/browser/id"} {
		if err := ValidateEndpoint(raw); err != nil {
			t.Fatal(raw, err)
		}
	}
}
func TestExplicitInputsRejectUnsupportedSourcesAndSilentSuccess(t *testing.T) {
	for _, raw := range []string{"file:///etc/passwd", "javascript:alert(1)", "data:text/html,hi", "chrome://settings", "https://user:password@example.com/", "https://"} {
		if err := validURL(raw); err == nil {
			t.Fatal("unsupported URL admitted", raw)
		}
	}
	for _, raw := range []string{"http://127.0.0.1:1234/path", "https://example.invalid/path", "about:blank"} {
		if err := validURL(raw); err != nil {
			t.Fatal(raw, err)
		}
	}
	c := &Client{}
	if _, err := c.Snapshot(context.Background(), "unknown", ""); err == nil {
		t.Fatal("unsupported snapshot mode returned empty success")
	}
	if _, err := c.Screenshot(context.Background(), ScreenshotOptions{Format: "jpeg", Quality: 101}); err == nil {
		t.Fatal("out-of-range quality accepted")
	}
	if _, err := c.Screenshot(context.Background(), ScreenshotOptions{Format: "png", FullPage: true, Selector: "article"}); err == nil {
		t.Fatal("ambiguous screenshot scope accepted")
	}
	if err := c.Action(context.Background(), Action{Kind: "type", Text: strings.Repeat("x", (64<<10)+1)}); err == nil {
		t.Fatal("oversized text accepted")
	}
}
