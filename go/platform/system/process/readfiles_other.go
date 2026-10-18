// INPUT: Explicit regular descriptor inheritance on unsupported non-Unix native platforms
// OUTPUT: Fail-closed descriptor validation error
// POS: [platform / OS primitive] Never claim read-only inherited capabilities where the platform contract is not implemented
//go:build !unix

package process

import (
	"errors"
	"os"
)

func validateReadFile(*os.File) error {
	return errors.New("read-only inherited descriptors are unsupported on this platform")
}
