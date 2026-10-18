// INPUT: testing, kernel/abi, kernel/abi/sessiontest
// OUTPUT: SQLite Session conformance against the public backend contract suite
// POS: [Native storage testing] Reuses SDK guarantees while retaining native privacy and durability policies.
package session

import (
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi/sessiontest"
	"testing"
)

func TestSQLiteSessionContract(t *testing.T) {
	sessiontest.Run(t, func(t *testing.T) abi.PersistentSession { _, log := repoSession(t); return log })
}
