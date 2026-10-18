// INPUT: context, errors, fmt, io, net/http/url/netip, strings, sync/atomic, time.
// OUTPUT: NetworkPolicy and bounded explicit HTTP fetching with pinned validated IP connections.
// POS: [L4 extension] Optional web access rejects ambient proxies, credential forwarding and private-address redirects.
package web

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"strings"
	"sync/atomic"
	"time"
)

const maxBody = 10 << 20

type NetworkPolicy struct {
	// Exact origins explicitly authorized by the host; applies only to the
	// initial request. Redirects must always resolve to public addresses.
	AllowedPrivateOrigins []string
}

type fetched struct {
	Body                            []byte
	InitialURL, FinalURL, MediaType string
	Status                          int
}

type HTTPError struct{ cause error }

func (*HTTPError) Error() string   { return "explicit web request failed; no automatic retry" }
func (e *HTTPError) Unwrap() error { return e.cause }

func parseURL(raw string) (*url.URL, error) {
	if len(raw) == 0 || len(raw) > 8192 || strings.ContainsAny(raw, "\x00\r\n") {
		return nil, errors.New("invalid bounded HTTP URL")
	}
	u, err := url.Parse(raw)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Hostname() == "" || u.User != nil || u.Opaque != "" || u.Fragment != "" {
		return nil, errors.New("explicit HTTP/HTTPS URL without credentials or fragment required")
	}
	if strings.Contains(u.Hostname(), "%") || strings.HasSuffix(u.Hostname(), ".") {
		return nil, errors.New("zoned or ambiguous host not allowed")
	}
	if port := u.Port(); port != "" {
		if _, err := net.LookupPort("tcp", port); err != nil {
			return nil, errors.New("invalid URL port")
		}
	}
	return u, nil
}

func origin(u *url.URL) string { return strings.ToLower(u.Scheme + "://" + u.Host) }

func publicIP(addr netip.Addr) bool {
	a := addr.Unmap()
	if !a.IsGlobalUnicast() || a.IsPrivate() || a.IsLoopback() || a.IsLinkLocalUnicast() || a.IsUnspecified() {
		return false
	}
	// Special-purpose IPv4 ranges not covered by IsPrivate/IsGlobalUnicast.
	for _, cidr := range []string{"0.0.0.0/8", "100.64.0.0/10", "192.0.0.0/24", "192.0.2.0/24", "198.18.0.0/15", "198.51.100.0/24", "203.0.113.0/24", "240.0.0.0/4", "2001:db8::/32", "2001::/32", "2002::/16", "64:ff9b::/96", "64:ff9b:1::/48"} {
		prefix, _ := netip.ParsePrefix(cidr)
		if prefix.Contains(a) {
			return false
		}
	}
	return true
}

func fetch(ctx context.Context, raw string, policy NetworkPolicy) (fetched, error) {
	var result fetched
	u, err := parseURL(raw)
	if err != nil {
		return result, err
	}
	result.InitialURL = raw
	initial := origin(u)
	private := false
	for _, allowed := range policy.AllowedPrivateOrigins {
		if allowed == initial {
			private = true
		}
	}
	var redirected atomic.Bool
	transport := &http.Transport{Proxy: nil, DisableKeepAlives: true, DisableCompression: true, MaxResponseHeaderBytes: 64 << 10, ResponseHeaderTimeout: 15 * time.Second, TLSHandshakeTimeout: 10 * time.Second}
	// Resolve and validate ALL answers, then connect to one of those exact IPs.
	// TLS retains the original URL's host/SNI; there is no second DNS lookup.
	transport.DialContext = func(ctx context.Context, network, address string) (net.Conn, error) {
		host, port, err := net.SplitHostPort(address)
		if err != nil {
			return nil, err
		}
		answers, err := net.DefaultResolver.LookupNetIP(ctx, "ip", host)
		if err != nil || len(answers) == 0 {
			return nil, errors.New("web DNS resolution failed")
		}
		for _, ip := range answers {
			if !publicIP(ip) && !(private && !redirected.Load()) {
				return nil, errors.New("web destination is not an authorized public address")
			}
		}
		var failures []error
		for _, ip := range answers {
			connection, dialErr := (&net.Dialer{Timeout: 10 * time.Second}).DialContext(ctx, network, net.JoinHostPort(ip.String(), port))
			if dialErr == nil {
				actual, _, parseErr := net.SplitHostPort(connection.RemoteAddr().String())
				actualIP, ipErr := netip.ParseAddr(actual)
				if parseErr != nil || ipErr != nil || actualIP.Unmap() != ip.Unmap() {
					connection.Close()
					return nil, errors.New("web connection address changed")
				}
				return connection, nil
			}
			failures = append(failures, dialErr)
		}
		return nil, errors.Join(failures...)
	}
	defer transport.CloseIdleConnections()
	client := &http.Client{Transport: transport, CheckRedirect: func(req *http.Request, via []*http.Request) error {
		if len(via) > 5 {
			return errors.New("web redirect limit exceeded")
		}
		if _, err := parseURL(req.URL.String()); err != nil {
			return err
		}
		redirected.Store(true)
		// No caller-provided headers, cookies, credentials or referrer travel.
		req.Header = make(http.Header)
		req.Header.Set("Accept-Encoding", "identity")
		return nil
	}}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, raw, nil)
	if err != nil {
		return result, err
	}
	request.Header.Set("Accept-Encoding", "identity")
	response, err := client.Do(request)
	if err != nil {
		return result, &HTTPError{cause: err}
	}
	defer response.Body.Close()
	result.Status, result.FinalURL, result.MediaType = response.StatusCode, response.Request.URL.String(), response.Header.Get("Content-Type")
	if response.Header.Get("Content-Encoding") != "" && response.Header.Get("Content-Encoding") != "identity" {
		return result, errors.New("compressed web body was not requested and is not silently decoded")
	}
	if response.ContentLength > maxBody {
		return result, errors.New("web response exceeds 10 MiB")
	}
	result.Body, err = io.ReadAll(io.LimitReader(response.Body, maxBody+1))
	if err != nil {
		return result, &HTTPError{cause: err}
	}
	if len(result.Body) > maxBody {
		result.Body = nil
		return result, errors.New("web response exceeds 10 MiB; no successful prefix")
	}
	if result.Status < 200 || result.Status > 599 {
		return result, fmt.Errorf("invalid HTTP status %d", result.Status)
	}
	return result, nil
}
