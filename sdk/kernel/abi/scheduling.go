// INPUT: path, strings
// OUTPUT: NormalizeResource, SchedulingConflicts
// POS: [L1 kernel contracts] Pure lexical resource identity and reader/writer conflict vocabulary without filesystem authorization.
package abi

import (
	"errors"
	"path"
	"strings"
)

func (scheduling Scheduling) Validate() error {
	switch scheduling.ExecutionMode {
	case "", Parallel, SerialGlobal, Coordinator:
	default:
		return errors.New("unknown tool execution mode")
	}
	for _, claim := range scheduling.Resources {
		if claim.Resource == "" || (claim.Access != AccessRead && claim.Access != AccessWrite) {
			return errors.New("invalid tool resource claim")
		}
	}
	return nil
}

func NormalizeResource(resource, workspace string) string {
	if strings.Contains(resource, "://") {
		return resource
	}
	if workspace != "" && !strings.HasPrefix(resource, "/") {
		resource = path.Join(workspace, resource)
	}
	return path.Clean(resource)
}
func SchedulingConflicts(left, right Scheduling, workspace string) bool {
	if left.Validate() != nil || right.Validate() != nil {
		return true
	}
	if left.ExecutionMode == SerialGlobal || right.ExecutionMode == SerialGlobal {
		return true
	}
	for _, a := range left.Resources {
		for _, b := range right.Resources {
			if NormalizeResource(a.Resource, workspace) == NormalizeResource(b.Resource, workspace) && (a.Access == AccessWrite || b.Access == AccessWrite) {
				return true
			}
		}
	}
	return false
}
