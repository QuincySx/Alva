// INPUT: errors
// OUTPUT: exchange (private)
// POS: [platform / WASM host or guest] Explicit rejection outside WASIp1 rather than secretly executing capabilities on the native host.
//go:build !wasip1 || !wasm

package guest

import "errors"

func fileToolsEnabled() bool { return false }

func exchange(uint32, []byte, int) ([]byte, error) {
	return nil, errors.New("sandbox guest imports require wasip1/wasm")
}
