// INPUT: Explicit browser temporary root and native Windows security descriptors
// OUTPUT: Same-user protected private browser-root validation on Windows
// POS: [platform / native execution host] Private profiles never borrow a user browser directory or pretend chmod enforces DACLs.
//go:build windows

package browser

import "github.com/QuincySx/Alva/go/platform/system/privatefs"

func checkPrivate(path string) error { return privatefs.CheckDir(path) }
