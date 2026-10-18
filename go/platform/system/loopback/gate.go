// INPUT: Actual numeric loopback authority, out-of-band secret and HTTP request metadata
// OUTPUT: Immutable Gate, random 256-bit tokens and shared no-store response headers
// POS: [platform / OS primitive] Native local transport boundary; no Hub, model, credential discovery or execution policy
package loopback

import (
	"crypto/rand"
	"crypto/subtle"
	"encoding/base64"
	"errors"
	"net"
	"net/http"
	"net/netip"
	"strconv"
	"strings"
)

type Gate struct{ authority, expected string }

func New(authority, token string) (*Gate, error) {
	host, port, err := net.SplitHostPort(authority)
	if err != nil {
		return nil, errors.New("explicit loopback IP and actual port required")
	}
	ip, err := netip.ParseAddr(host)
	n, portErr := strconv.Atoi(port)
	if err != nil || !ip.IsLoopback() || ip.Zone() != "" || portErr != nil || n < 1 || n > 65535 || net.JoinHostPort(ip.String(), strconv.Itoa(n)) != authority {
		return nil, errors.New("canonical loopback listener authority required")
	}
	decoded, err := base64.RawURLEncoding.DecodeString(token)
	if err != nil || len(decoded) != 32 || base64.RawURLEncoding.EncodeToString(decoded) != token {
		return nil, errors.New("32-byte base64url secret token required")
	}
	return &Gate{authority: authority, expected: "Bearer " + token}, nil
}

func NewToken() (string, error) {
	var data [32]byte
	if _, err := rand.Read(data[:]); err != nil {
		return "", err
	}
	return base64.RawURLEncoding.EncodeToString(data[:]), nil
}

// Check returns zero only for an authorized request. Callers retain their own
// protocol error envelopes; an Origin or query value never grants access.
func (g *Gate) Check(r *http.Request) int {
	if g == nil || r == nil || r.URL == nil || r.Host != g.authority || len(r.URL.RawQuery) > 4096 || r.URL.User != nil || r.URL.IsAbs() {
		return http.StatusForbidden
	}
	origins := r.Header.Values("Origin")
	if len(origins) > 1 || (len(origins) == 1 && origins[0] != "http://"+g.authority) {
		return http.StatusForbidden
	}
	sites := r.Header.Values("Sec-Fetch-Site")
	if len(sites) > 1 || (len(sites) == 1 && sites[0] != "same-origin" && sites[0] != "none") {
		return http.StatusForbidden
	}
	auth := r.Header.Values("Authorization")
	if len(auth) != 1 || len(auth[0]) != len(g.expected) || subtle.ConstantTimeCompare([]byte(auth[0]), []byte(g.expected)) != 1 {
		return http.StatusUnauthorized
	}
	if strings.Contains(r.URL.RawQuery, "token") {
		return http.StatusBadRequest
	}
	return 0
}

func SecureHeaders(w http.ResponseWriter) {
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	w.Header().Set("Referrer-Policy", "no-referrer")
	w.Header().Set("Content-Security-Policy", "default-src 'none'; frame-ancestors 'none'")
	w.Header().Set("Cross-Origin-Resource-Policy", "same-origin")
}
