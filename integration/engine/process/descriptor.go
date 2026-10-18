// INPUT: Explicit configured external engine adapter, without executable/account discovery
// OUTPUT: Conservative external engine Descriptor for the actual Go request/config surface
// POS: [integration / native external-engine adapter] Vendor CLI flags are not evidence of equivalent Alva permission profiles or native Inspector support.
package process

import "github.com/QuincySx/Alva/integration/engine"

func (a *Adapter) Descriptor() engine.Descriptor {
	descriptor := engine.DescribeRuntime(a)
	descriptor.Install = engine.InstallMetadata{RequiredBinary: a.config.Vendor, InstallHint: "Supply an explicit executable and separately approve host/account access; Alva does not install or discover it."}
	if a.config.Vendor == "claude" {
		descriptor.Config.SupportsSystemPrompt = true
		descriptor.Config.SupportsMaxTurns = true
	}
	// No provider headers/fast mode are routed. Codex/Pi user-message prefixing
	// is not a real system prompt; those options remain explicitly unsupported.
	return descriptor
}
