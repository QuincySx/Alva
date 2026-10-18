// INPUT: testing, kernel/abi/sessiontest, kernel/abi
// OUTPUT: MemorySession conformance against the public backend contract suite
// POS: [L2.5 SDK assembly testing] Runs the same semantics as native and third-party logs.
package core_test

import (
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi/sessiontest"
	"testing"
)

func TestMemorySessionContract(t *testing.T) {
	sessiontest.Run(t, func(*testing.T) abi.PersistentSession { return core.NewMemorySession("contract") })
}
