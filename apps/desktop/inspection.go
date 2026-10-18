// INPUT: Native record metadata and versioned kernel audit vocabulary
// OUTPUT: InspectionGroup and InspectionGroupVisible for current-page native Inspector presentation
// POS: [L6 application] Categorization changes only the visible page, never query cursors, model history or native facts.
package desktop

import (
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"strings"
)

func InspectionGroup(entry session.RecordEntry) string {
	if entry.Namespace == "alva.kernel" && entry.SchemaVersion == 1 {
		switch entry.Kind {
		case abi.RecordModelIntent, abi.RecordModelOutcome:
			return "LLM"
		case abi.RecordToolIntent, abi.RecordToolOutcome:
			return "Tools"
		}
	}
	if entry.Kind == abi.RecordExtension && entry.Namespace == "alva.kernel.audit" && entry.SchemaVersion == 1 {
		if entry.Type == "middleware_hook_batch" {
			return "Middleware"
		}
		if strings.HasPrefix(entry.Type, "llm_") {
			return "LLM"
		}
	}
	return "Other"
}
func InspectionGroupVisible(entry session.RecordEntry, group string) bool {
	return group == "" || group == "All" || InspectionGroup(entry) == group
}
