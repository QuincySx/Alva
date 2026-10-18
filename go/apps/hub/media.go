// INPUT: Explicit next-turn media capability and native decoder executable snapshots
// OUTPUT: MediaDecoder validation without native process or media imports
// POS: [L7 Hub product] Product consent values never imply image support, media processing or model delivery
package hub

import (
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

type MediaDecoder struct {
	FFmpeg             string `json:"ffmpeg"`
	FFprobe            string `json:"ffprobe"`
	Enabled            bool   `json:"enabled"`
	HostAccessApproved bool   `json:"host_access_approved"`
}

func (d MediaDecoder) Validate() error {
	if d.FFmpeg == "" && d.FFprobe == "" && !d.Enabled && !d.HostAccessApproved {
		return nil
	}
	for _, path := range []string{d.FFmpeg, d.FFprobe} {
		if !filepath.IsAbs(path) || len(path) > 4096 || !utf8.ValidString(path) || strings.ContainsAny(path, "\x00\r\n") {
			return errors.New("media decoder requires two explicit absolute executable paths")
		}
	}
	if d.Enabled && !d.HostAccessApproved {
		return errors.New("media decoder requires independent explicit full host access approval")
	}
	return nil
}
