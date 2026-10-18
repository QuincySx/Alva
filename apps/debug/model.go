// INPUT: Versioned pure UI inspection/action values and bounded JSON validation
// OUTPUT: Node, Snapshot, Method, Action, ActionResult and diagnostic errors
// POS: [L6 diagnostics] Describe registered product views without exporting framework internals or execution authority.
package debug

import (
	"bytes"
	"encoding/json"
	"errors"
	"regexp"

	strictjson "github.com/QuincySx/Alva/platform/system/strictjson"
)

var ErrUnavailable = errors.New("diagnostic view is unavailable")
var ErrBusy = errors.New("diagnostic view already has a queued or running request")
var ErrStale = errors.New("diagnostic snapshot revision changed; inspect the actual view again")
var ErrFramePending = errors.New("registered window has not rendered its current view state; wait for an actual frame before capture")
var identifier = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$`)
var revisionPattern = regexp.MustCompile(`^[a-f0-9]{64}$`)

type Bounds struct {
	X      float64 `json:"x"`
	Y      float64 `json:"y"`
	Width  float64 `json:"width"`
	Height float64 `json:"height"`
}
type Node struct {
	ID         string         `json:"id"`
	Type       string         `json:"type"`
	Bounds     *Bounds        `json:"bounds,omitempty"`
	Properties map[string]any `json:"properties,omitempty"`
	Children   []Node         `json:"children,omitempty"`
}
type Method struct {
	Name        string `json:"name"`
	Description string `json:"description"`
	Effect      string `json:"effect"` // ui, stop or execute; metadata does not authorize it.
}
type Snapshot struct {
	ViewID   string         `json:"view_id"`
	Revision string         `json:"revision"`
	Tree     Node           `json:"tree"`
	State    map[string]any `json:"state"`
}
type Action struct {
	OperationID string          `json:"operation_id"`
	ViewID      string          `json:"view_id"`
	Revision    string          `json:"revision"`
	Method      string          `json:"method"`
	Args        json.RawMessage `json:"args"`
}
type ActionResult struct {
	OperationID string `json:"operation_id"`
	Status      string `json:"status"` // applied, rejected, cancelled or pending; never an Agent terminal status.
	Revision    string `json:"revision,omitempty"`
	Error       string `json:"error,omitempty"`
}

func (a Action) validate() error {
	if !identifier.MatchString(a.OperationID) || !identifier.MatchString(a.ViewID) || !identifier.MatchString(a.Method) || !revisionPattern.MatchString(a.Revision) || len(a.Args) == 0 || len(a.Args) > 32<<10 {
		return errors.New("bounded exact action identity/revision/args required")
	}
	if args := bytes.TrimSpace(a.Args); len(args) == 0 || args[0] != '{' {
		return errors.New("UI action arguments require an object")
	}
	return strictjson.Validate(a.Args)
}
func validateNode(node Node, depth int, budget *int) error {
	*budget--
	if depth > 16 || *budget < 0 || !identifier.MatchString(node.ID) || node.Type == "" || len(node.Type) > 128 || len(node.Properties) > 32 {
		return errors.New("registered view tree exceeds bounds")
	}
	for _, child := range node.Children {
		if err := validateNode(child, depth+1, budget); err != nil {
			return err
		}
	}
	return nil
}
