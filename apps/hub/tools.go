// INPUT: Explicit session capability choices and bounded shell/web configuration values
// OUTPUT: ToolSettings, ShellSettings, WebSettings and ToolSettingsStore
// POS: [L7 Hub product] Next-turn optional tool policy without importing implementation or executing resources.
package hub

import (
	"context"
	"errors"
	"net/url"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

type ShellSettings struct {
	Executable         string            `json:"executable"`
	Env                map[string]string `json:"env,omitempty"`
	Enabled            bool              `json:"enabled"`
	HostAccessApproved bool              `json:"host_access_approved"`
}
type WebSettings struct {
	Enabled               bool     `json:"enabled"`
	NetworkAccessApproved bool     `json:"network_access_approved"`
	ReadURLs              bool     `json:"read_urls"`
	SearchEndpoint        string   `json:"search_endpoint,omitempty"`
	AllowedPrivateOrigins []string `json:"allowed_private_origins,omitempty"`
	TimeoutMS             int      `json:"timeout_ms,omitempty"`
}
type ToolSettings struct {
	Planning    bool          `json:"planning"`
	Notebook    bool          `json:"notebook"`
	Interaction bool          `json:"interaction"`
	Utility     bool          `json:"utility"`
	Shell       ShellSettings `json:"shell"`
	Web         WebSettings   `json:"web"`
}
type ToolSettingsStore interface {
	ReplaceTools(context.Context, string, ToolSettings) error
	SetToolEnabled(context.Context, string, string, bool, bool) error
}

func (s ToolSettings) Validate() error {
	if s.Shell.Executable != "" || len(s.Shell.Env) > 0 || s.Shell.Enabled || s.Shell.HostAccessApproved {
		// Reuse only the pure argv/env bounds; a shell still has separate
		// full-host consent and never inherits Hook approval.
		n, err := (NativeCommand{Command: s.Shell.Executable, Env: s.Shell.Env, Enabled: s.Shell.Enabled, HostAccessApproved: s.Shell.HostAccessApproved}).validate(0)
		if err != nil {
			return err
		}
		if n > 64<<10 {
			return errors.New("shell configuration exceeds 64 KiB")
		}
		if !filepath.IsAbs(s.Shell.Executable) {
			return errors.New("absolute explicit shell executable required")
		}
	}
	w := s.Web
	if w.Enabled && (!w.NetworkAccessApproved || !w.ReadURLs && w.SearchEndpoint == "") {
		return errors.New("web capability requires independent network consent and a configured operation")
	}
	if w.TimeoutMS < 0 || w.TimeoutMS > 60000 || len(w.AllowedPrivateOrigins) > 16 {
		return errors.New("web timeout/origin budget exceeded")
	}
	checkURL := func(raw string) (*url.URL, error) {
		u, err := url.Parse(raw)
		if err != nil || len(raw) > 4096 || !utf8.ValidString(raw) || strings.ContainsAny(raw, "\x00\r\n") || u.Host == "" || u.User != nil || u.Fragment != "" || u.RawQuery != "" || u.Scheme != "http" && u.Scheme != "https" {
			return nil, errors.New("explicit bounded HTTP(S) URL without credentials/query/fragment required")
		}
		return u, nil
	}
	if w.SearchEndpoint != "" {
		if _, err := checkURL(w.SearchEndpoint); err != nil {
			return err
		}
	}
	for _, raw := range w.AllowedPrivateOrigins {
		u, err := checkURL(raw)
		if err != nil {
			return err
		}
		if u.Path != "" || u.Scheme+"://"+u.Host != raw {
			return errors.New("private web grants must be exact origins")
		}
	}
	return nil
}
