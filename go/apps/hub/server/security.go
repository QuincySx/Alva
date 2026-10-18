// INPUT: Explicit loopback gate, product polling interval and HTTP metadata
// OUTPUT: Product Config validation and protocol-owned authentication errors
// POS: [L7 Hub product] Hub consumes shared native security without making other HTTP products depend on Hub
package server

import (
	"errors"
	"net/http"
	"time"

	"github.com/QuincySx/Alva/go/platform/system/loopback"
)

type Config struct {
	Authority    string
	Token        string `json:"-"`
	PollInterval time.Duration
}

func (c Config) Validate() error {
	if _, err := loopback.New(c.Authority, c.Token); err != nil {
		return err
	}
	if c.PollInterval < 0 || c.PollInterval > time.Second {
		return errors.New("poll interval must be at most one second")
	}
	return nil
}
func (s *Server) authenticate(w http.ResponseWriter, r *http.Request) bool {
	loopback.SecureHeaders(w)
	if status := s.access.Check(r); status != 0 {
		if status == http.StatusUnauthorized {
			w.Header().Set("WWW-Authenticate", "Bearer")
		}
		fail(w, status)
		return false
	}
	return true
}
