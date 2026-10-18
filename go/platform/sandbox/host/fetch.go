// INPUT: context, errors, io, net, net/http, net/url, strings, time, sandbox/abi
// OUTPUT: ValidateAllowedDomain
// POS: [platform / WASM host or guest] Explicit-domain HTTP capability enforcing authority/header rules on every redirect without ambient proxies.
package host

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net"
	"net/http"
	"net/url"
	"sort"
	"strings"
	"time"

	"github.com/QuincySx/Alva/go/sdk/sandbox/abi"
)

func ValidateAllowedDomain(value string) error { _, err := domainRule(value); return err }
func domainRule(value string) (string, error) {
	if value == "" || len(value) > 253 || strings.TrimSpace(value) != value {
		return "", errors.New("invalid fetch domain grant")
	}
	normal := strings.ToLower(strings.TrimSuffix(value, "."))
	host := strings.TrimPrefix(normal, "*.")
	if net.ParseIP(host) != nil {
		if host != normal {
			return "", errors.New("wildcard IP grant not supported")
		}
		return normal, nil
	}
	for _, label := range strings.Split(host, ".") {
		if label == "" || len(label) > 63 || label[0] == '-' || label[len(label)-1] == '-' {
			return "", errors.New("invalid domain grant")
		}
		for _, c := range label {
			if !((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-') {
				return "", errors.New("domain grants require ASCII hostnames without ports")
			}
		}
	}
	return normal, nil
}
func permitted(domains []string, target *url.URL) bool {
	if target.User != nil || (target.Scheme != "http" && target.Scheme != "https") || target.Hostname() == "" || strings.Contains(target.Hostname(), "%") {
		return false
	}
	host := strings.ToLower(strings.TrimSuffix(target.Hostname(), "."))
	for _, raw := range domains {
		rule, err := domainRule(raw)
		if err != nil {
			continue
		}
		if strings.HasPrefix(rule, "*.") {
			apex := rule[2:]
			if host != apex && strings.HasSuffix(host, "."+apex) {
				return true
			}
		} else if rule == host {
			return true
		}
	}
	return false
}
func executeFetch(ctx context.Context, domains []string, request abi.FetchRequest) (abi.FetchResponse, error) {
	target, err := url.Parse(request.URL)
	if err != nil || !permitted(domains, target) {
		return abi.FetchResponse{}, errors.New("fetch URL not authorized by job domain grants")
	}
	ctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	transport := &http.Transport{Proxy: nil, DialContext: (&net.Dialer{Timeout: 5 * time.Second}).DialContext, TLSHandshakeTimeout: 5 * time.Second, ResponseHeaderTimeout: 5 * time.Second, MaxResponseHeaderBytes: 64 << 10, DisableCompression: true, MaxIdleConns: 1, MaxConnsPerHost: 1}
	defer transport.CloseIdleConnections()
	client := &http.Client{Transport: transport, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	headers := make(http.Header)
	for _, header := range request.Headers {
		name := http.CanonicalHeaderKey(header.Name)
		switch name {
		case "Host", "Connection", "Content-Length", "Transfer-Encoding", "Trailer", "Upgrade", "Proxy-Authorization", "Proxy-Connection", "Te", "Keep-Alive":
			return abi.FetchResponse{}, errors.New("guest cannot override authority or connection framing headers")
		}
		headers.Add(name, header.Value)
	}
	method := request.Method
	body := request.Body
	for redirects := 0; redirects <= 10; redirects++ {
		if !permitted(domains, target) {
			return abi.FetchResponse{}, errors.New("fetch redirect target not authorized")
		}
		req, err := http.NewRequestWithContext(ctx, method, target.String(), bytes.NewReader(body))
		if err != nil {
			return abi.FetchResponse{}, errors.New("invalid fetch method or headers")
		}
		req.Header = headers.Clone()
		response, err := client.Do(req)
		if err != nil {
			return abi.FetchResponse{}, errors.New("fetch transport failed")
		}
		redirect := response.StatusCode == 301 || response.StatusCode == 302 || response.StatusCode == 303 || response.StatusCode == 307 || response.StatusCode == 308
		if redirect && response.Header.Get("Location") != "" {
			next, err := target.Parse(response.Header.Get("Location"))
			response.Body.Close()
			if err != nil || !permitted(domains, next) {
				return abi.FetchResponse{}, errors.New("fetch redirect target not authorized")
			}
			if redirects == 10 {
				return abi.FetchResponse{}, errors.New("fetch redirect limit exceeded")
			}
			if !strings.EqualFold(target.Host, next.Host) || target.Scheme != next.Scheme {
				headers.Del("Authorization")
				headers.Del("Cookie")
			}
			if response.StatusCode == 303 || (method != "GET" && method != "HEAD" && (response.StatusCode == 301 || response.StatusCode == 302)) {
				method = "GET"
				body = nil
				headers.Del("Content-Type")
			}
			target = next
			continue
		}
		data, err := io.ReadAll(io.LimitReader(response.Body, abi.MaxFetchResponseBodyBytes+1))
		closeErr := response.Body.Close()
		if err != nil || closeErr != nil {
			return abi.FetchResponse{}, errors.New("fetch response read failed")
		}
		if len(data) > abi.MaxFetchResponseBodyBytes {
			return abi.FetchResponse{}, errors.New("fetch response body budget exceeded")
		}
		out := abi.FetchResponse{Status: response.StatusCode, Body: data}
		names := make([]string, 0, len(response.Header))
		for name := range response.Header {
			names = append(names, name)
		}
		sort.Strings(names)
		for _, name := range names {
			for _, value := range response.Header.Values(name) {
				out.Headers = append(out.Headers, abi.Header{Name: name, Value: value})
				if len(out.Headers) > 256 {
					return abi.FetchResponse{}, errors.New("fetch response header count exceeded")
				}
			}
		}
		return out, nil
	}
	return abi.FetchResponse{}, errors.New("fetch redirect limit exceeded")
}
