# Hub visual tests
> Playwright-backed deterministic browser screenshots for page and overlay hierarchy.

## Rules

- Use `?demo=1` for product fixtures and `?ui-lab=1` for primitive fixtures.
- Screenshot names follow the seven aggregate wireframes in `docs/ALVA-HUB-UI-WIREFRAMES.md`.
- Keep screenshots at 1440×900 with reduced motion and disabled caret animation.
- Do not mock Radix overlays; open them through real pointer or keyboard interactions.
- `windowing.spec.ts` checks measured bottom at 100/1000/10000 entries, resize/jump, all-row DOM bounds and the same reading row's pixel offset during streaming.
- `media.spec.ts` uses UI Lab real message/Dialog/HTTP-bridge components with intercepted account-free GET fixtures, a browser-generated JPEG, English/Chinese at 1440/720px, lazy reads, actual Blob decode/revocation, inert manifests and keyboard/focus. This is Chromium evidence, not native WebView or provider acceptance.

## Inventory

| File | Responsibility |
|---|---|
| media.spec.ts | Read-only derived media inspection, local decode and lifecycle/keyboard regression |
| windowing.spec.ts | Measured virtual message history and streaming reading position regression |
| Other *.spec.ts | Existing deterministic screenshot and UI interaction checks |
