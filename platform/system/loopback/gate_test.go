// INPUT: Random out-of-band secrets and synthetic HTTP metadata without a listening service
// OUTPUT: Exact authority, Origin and duplicate-Bearer/URL-token/forwarded-host denial regressions
// POS: [platform / OS primitive] Shared local authentication acceptance independent of product state or protocol envelopes
package loopback

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestGateExactLocalAuthorityAndOutOfBandAuthorization(t *testing.T) {
	token, err := NewToken()
	if err != nil {
		t.Fatal(err)
	}
	for _, authority := range []string{"0.0.0.0:1234", "localhost:1234", "127.0.0.1:0", "127.0.0.1:01234", "[::1%lo0]:1234"} {
		if _, err = New(authority, token); err == nil {
			t.Fatal("ambiguous/public authority allowed", authority)
		}
	}
	if _, err = New("127.0.0.1:1234", "weak"); err == nil {
		t.Fatal("weak token")
	}
	g, err := New("127.0.0.1:1234", token)
	if err != nil {
		t.Fatal(err)
	}
	for _, mode := range []string{"valid", "cross-host", "cross-origin", "duplicate-origin", "cross-site", "duplicate-auth", "query", "missing-auth", "forwarded"} {
		t.Run(mode, func(t *testing.T) {
			r := httptest.NewRequest("GET", "/v1/health", nil)
			r.Host = "127.0.0.1:1234"
			r.Header.Set("Authorization", "Bearer "+token)
			want := 0
			switch mode {
			case "cross-host":
				r.Host = "evil.example:1234"
				want = http.StatusForbidden
			case "cross-origin":
				r.Header.Set("Origin", "https://evil.example")
				want = http.StatusForbidden
			case "duplicate-origin":
				r.Header.Add("Origin", "http://127.0.0.1:1234")
				r.Header.Add("Origin", "http://127.0.0.1:1234")
				want = http.StatusForbidden
			case "cross-site":
				r.Header.Set("Sec-Fetch-Site", "cross-site")
				want = http.StatusForbidden
			case "duplicate-auth":
				r.Header.Add("Authorization", "Bearer "+token)
				want = http.StatusUnauthorized
			case "query":
				r.URL.RawQuery = "token=" + token
				want = http.StatusBadRequest
			case "missing-auth":
				r.Header.Del("Authorization")
				want = http.StatusUnauthorized
			case "forwarded":
				r.Host = "evil.example"
				r.Header.Set("X-Forwarded-Host", "127.0.0.1:1234")
				want = http.StatusForbidden
			}
			if got := g.Check(r); got != want {
				t.Fatalf("gate=%d want %d", got, want)
			}
		})
	}
}
