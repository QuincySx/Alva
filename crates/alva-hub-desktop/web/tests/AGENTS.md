# Hub visual tests
> Playwright-backed deterministic browser screenshots for page and overlay hierarchy.

## Rules

- Use `?demo=1` for product fixtures and `?ui-lab=1` for primitive fixtures.
- Screenshot names follow the seven aggregate wireframes in `docs/ALVA-HUB-UI-WIREFRAMES.md`.
- Keep screenshots at 1440×900 with reduced motion and disabled caret animation.
- Do not mock Radix overlays; open them through real pointer or keyboard interactions.
