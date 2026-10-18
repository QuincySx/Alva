// INPUT: errors.
// OUTPUT: explicit unsupported private browser-root validation for other platforms.
// POS: [platform / native execution host] Missing platform ACL ownership proof fails closed rather than claiming isolated profiles.
//go:build !darwin && !linux && !windows

package browser

import "errors"

func checkPrivate(string) error {
	return errors.New("private browser profile ownership validation not implemented on this platform")
}
