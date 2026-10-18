// INPUT: Explicit next-turn browser executable, environment and independent host consent
// OUTPUT: BrowserSettings pure values and fail-closed configuration validation
// POS: [L7 Hub product] Product snapshots do not discover browsers, reuse profiles or own CDP/process policy
package hub

import (
	"errors"
	"net/url"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

type BrowserSettings struct {
	Executable         string            `json:"executable"`
	Env                map[string]string `json:"env,omitempty"`
	Headless           bool              `json:"headless"`
	Proxy              string            `json:"proxy,omitempty"`
	StartupTimeoutMS   int               `json:"startup_timeout_ms,omitempty"`
	OperationTimeoutMS int               `json:"operation_timeout_ms,omitempty"`
	Enabled            bool              `json:"enabled"`
	HostAccessApproved bool              `json:"host_access_approved"`
}

func (b BrowserSettings) Validate() error {
	if b.Executable == "" && len(b.Env) == 0 && b.Proxy == "" && b.StartupTimeoutMS == 0 && b.OperationTimeoutMS == 0 && !b.Enabled && !b.HostAccessApproved {
		return nil
	}
	if !filepath.IsAbs(b.Executable) || len(b.Executable) > 4096 || !utf8.ValidString(b.Executable) || strings.ContainsAny(b.Executable, "\x00\r\n") || b.StartupTimeoutMS < 0 || b.StartupTimeoutMS > 60000 || b.OperationTimeoutMS < 0 || b.OperationTimeoutMS > 60000 || len(b.Env) > 64 {
		return errors.New("browser requires an explicit absolute executable and bounded environment/timeouts")
	}
	if b.Enabled && !b.HostAccessApproved {
		return errors.New("browser requires independent explicit full host/network access approval")
	}
	if b.Proxy != "" {
		u, err := url.Parse(b.Proxy)
		if err != nil || len(b.Proxy) > 2048 || !utf8.ValidString(b.Proxy) || u.User != nil || u.Host == "" || (u.Scheme != "http" && u.Scheme != "https" && u.Scheme != "socks5") || u.Path != "" || u.RawQuery != "" || u.Fragment != "" {
			return errors.New("browser proxy must be explicit and omit credentials/path/query")
		}
	}
	total := 0
	for key, value := range b.Env {
		if key == "" || len(key) > 128 || strings.ContainsAny(key, "=\x00") || len(value) > 8192 || strings.ContainsRune(value, 0) || !utf8.ValidString(key+value) {
			return errors.New("invalid explicit browser environment")
		}
		switch key {
		case "HOME", "USERPROFILE", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "TMPDIR", "CHROME_USER_DATA_DIR":
			return errors.New("browser private home/profile environment cannot be overridden")
		}
		total += len(key) + len(value)
	}
	if total > 32<<10 {
		return errors.New("browser environment exceeds 32 KiB")
	}
	return nil
}
