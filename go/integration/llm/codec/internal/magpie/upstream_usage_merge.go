// INPUT: Request-local Magpie values.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

// carryUpdate keeps this message's known 1h writes when a partial split
// reports only 5m counts, then adds usage from earlier messages.
func (u Usage) carryUpdate(this, before Usage) Usage {
	if u.CacheWrite1h == 0 && u.cacheCreationHasCounts {
		u.CacheWrite1h = this.CacheWrite1h
	}
	return u.plus(before, true)
}

func (u Usage) plus(v Usage, only bool) Usage {
	if only && u.CacheWrite > 0 && !u.cacheCreationHasCounts && u.CacheWrite1h == 0 {
		u.cacheCreationReported = false
	}
	for _, f := range []struct{ a, b *int }{{&u.Input, &v.Input}, {&u.Output, &v.Output}, {&u.CacheRead, &v.CacheRead}, {&u.Reasoning, &v.Reasoning}} {
		if *f.a > 0 || !only {
			*f.a += *f.b
		}
	}
	writes := u.CacheWrite > 0
	if writes || !only {
		u.CacheWrite += v.CacheWrite
	}
	if u.CacheWrite1h > 0 || u.cacheCreationHasCounts || !only {
		u.CacheWrite1h += v.CacheWrite1h
	}
	if !only {
		u.cacheCreationReported = u.cacheCreationReported || v.cacheCreationReported
	}
	return u
}
