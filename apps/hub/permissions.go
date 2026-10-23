// INPUT: Explicit next-Turn product permission selection, independent of SDK modes.
// OUTPUT: Bounded Ask/Auto/Full values and validation; empty persisted policy means Ask.
// POS: [L7 Hub product] Product policy data only; the native host enforces scope and approvals.
package hub

import "errors"

type PermissionPolicy string

const (
	PermissionAsk  PermissionPolicy = "ask"
	PermissionAuto PermissionPolicy = "auto"
	PermissionFull PermissionPolicy = "full"
)

func (p PermissionPolicy) Effective() PermissionPolicy {
	if p == "" {
		return PermissionAsk
	}
	return p
}

func (p PermissionPolicy) Validate() error {
	switch p.Effective() {
	case PermissionAsk, PermissionAuto, PermissionFull:
		return nil
	default:
		return errors.New("unknown product permission policy")
	}
}
