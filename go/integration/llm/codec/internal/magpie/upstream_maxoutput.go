// INPUT: Request-local Magpie values.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

// defaultMaxTokens is what a request naming no reply limit is sent on
// Anthropic's API for a model whose limit isn't known.
const defaultMaxTokens = 16384
