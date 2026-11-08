// INPUT: context, harness compaction config/reports, engine context contract, product connections and manual compaction intents, llm/wire usage
// OUTPUT: Per-attempt Alva compaction configuration and the harness→engine context report conversion
// POS: [L7 Hub assembly] Binds the declared route window, the one-shot manual intent and the adapter's ReportContext sink to one native session.
package bootstrap

import (
	"context"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// attemptCompaction builds the attempt's compaction policy. pending reports
// whether a product manual intent (not the request itself) asked for it, so
// the assembly clears that intent only after the request is admitted.
func attemptCompaction(service *product.HubService, request engine.Request, target hub.Target, nativeID string) (harness.CompactionConfig, bool) {
	config := harness.CompactionConfig{Force: request.CompactContext}
	if service != nil && product.IsConnectionTarget(target) {
		config.WindowTokens = service.Connections.ContextWindow(target)
	}
	pending := service != nil && service.ContextCompactionPending(request.OwnerID)
	config.Force = config.Force || pending
	report := request.ReportContext
	config.Report = func(_ context.Context, value harness.ContextReport) error {
		if report == nil {
			return nil
		}
		return report(engineContextReport(nativeID, value))
	}
	return config, pending
}

func engineContextReport(nativeID string, value harness.ContextReport) engine.ContextReport {
	var out engine.ContextReport
	if u := value.Usage; u != nil {
		usage := &engine.ContextUsage{InputTokens: u.InputTokens, OutputTokens: u.OutputTokens, WindowTokens: u.WindowTokens}
		if u.CachedInputTokens != nil {
			cached := *u.CachedInputTokens
			usage.CachedInputTokens = &cached
		}
		if p := u.Parts; p != nil {
			usage.Parts = &engine.ContextParts{System: p.System, Tools: p.Tools, Summary: p.Summary, Recent: p.Recent}
		}
		out.Usage = usage
	}
	if c := value.Compaction; c != nil {
		compaction := &engine.ContextCompaction{Status: engine.ContextCompactionStatus(c.Status), Trigger: string(c.Trigger), FromSequence: c.FromSequence, ThroughSequence: c.ThroughSequence, Messages: uint64(max(c.Messages, 0)), Error: c.Error}
		if c.Record != nil {
			compaction.Reference = &engine.Reference{SessionID: nativeID, RecordID: c.Record.RecordID, Sequence: c.Record.Sequence}
		}
		if c.Usage != nil {
			compaction.Usage = engineUsage(*c.Usage)
		}
		out.Compaction = compaction
	}
	return out
}

func engineUsage(usage wire.Usage) *engine.Usage {
	out := &engine.Usage{InputTokens: usage.InputTokens, OutputTokens: usage.OutputTokens}
	if usage.CachedInputTokens != nil {
		value := *usage.CachedInputTokens
		out.CachedInputTokens = &value
	}
	if usage.CacheWriteInputTokens != nil {
		value := *usage.CacheWriteInputTokens
		out.CacheWriteInputTokens = &value
	}
	if out.Validate() != nil {
		out.CachedInputTokens, out.CacheWriteInputTokens = nil, nil
	}
	return out
}
