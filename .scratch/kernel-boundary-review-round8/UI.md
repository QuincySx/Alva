# Round 8 UI review

HEAD: c8aca9c8. Read-only product review. UI AGENTS, FRACTAL-DOCS, glossary, wireframes and motion requirements were read in the preceding UI pass; scope expanded to asynchronous session ownership. R79 excluded.

## U1 / P2: Pending session open overrides subsequent navigation

App.tsx:1040–1044 awaits getHubSession then unconditionally installs its response. resetSessionSurface at 962–981 clears state but does not invalidate the outstanding load. Repro: click session A, hold response, click New session (workspace chooser is visibly shown), release A. Old session reopens and workspace chooser disappears. The same unguarded loader permits slow A to override a later fast B selection.

Acceptance: deferred response tests for A→New and A→B with reversed completion order; only the latest navigation intent may install session, workspace, route, profile, timeline, detail or team. Include pending getTeamRunForLead at 1075–1079.

## U2 / P2: Old session event refresh installs its details in a different visible session

App.tsx:697–706 checks event ownership before getHubSession, but its .then only checks the component-level active flag. Repro: open A, receive fallback event and hold the requested A refresh; open B (completed), show Details and confirm B-only memory; release A refresh. B's Details now renders A-only memory. Active session and timeline remain B. Other consumers of sessionDetail (branches, usage, Inspector, activeBranch route derivation) receive the mismatched object too.

Acceptance: deferred terminal/fallback refresh A→B and A→New; assert displayed memory/branch/Inspector belongs to the selected session. Also scope the worker-event team refresh at 710–714 to its original lead, and the addMemory completion at 1087–1090 to its target session. Those two additional paths were statically identified, not independently probed.

Stop gate is NOT part of this finding: its explicit active/cancellable id check protected the probe, so an initial Stop-gate hypothesis was discarded.

## Validation

Command from crates/alva-hub-desktop/web:
`./node_modules/.bin/vitest run src/review-round8-ui.test.tsx -t Round8`

Result: 2 expected failures, 34 existing tests skipped. The test ran a copied App integration harness with controlled promises and actual UI navigation; neither sleep nor production logic duplication controls the outcomes. No backend or live Tauri claim.

Evidence: evidence/ui-session-races.test.tsx (original harness plus the two probes, trimmed of unrelated tests for reuse; copy to web/src/review-round8-ui.test.tsx to run); evidence/ui-session-races.txt (tool output). Temporary src test removed. No product edits.

