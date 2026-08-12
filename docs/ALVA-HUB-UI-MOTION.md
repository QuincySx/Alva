# ALVA Hub UI Motion Standard

> Status: Accepted motion vocabulary for the ALVA Hub Tauri UI system.
>
> Source principles: `emilkowalski/skills`, especially `emil-design-eng`,
> `animate`, `find-animation-opportunities`, `improve-animations`, and
> `review-animations`.

## Decision Sequence

Before adding motion, answer in order:

1. How often does the user perform this interaction?
2. Does motion provide feedback, spatial continuity, state explanation, or
   protection from a jarring change?
3. Is the element entering/exiting, moving within the viewport, changing
   color, or continuously moving?
4. Can the user reverse the action before it finishes?
5. What state information remains under reduced motion?

If there is no useful answer to question 2, do not animate.

## Frequency Rules

| Frequency | Default rule |
|---|---|
| Constant or 100+ times per day | No positional motion |
| Tens of times per day | Color/focus feedback; remove or drastically shorten movement |
| Occasional | Short standard transition when it explains relationship or state |
| Rare | May use restrained delight after the functional transition is correct |

Keyboard-initiated navigation and commands do not add positional motion.

## Tokens

```css
--duration-color: 120ms;
--duration-press: 120ms;
--duration-tooltip: 140ms;
--duration-disclosure: 180ms;
--duration-popover: 180ms;
--duration-dialog: 220ms;
--duration-drawer: 240ms;

--ease-out: cubic-bezier(0.23, 1, 0.32, 1);
--ease-in-out: cubic-bezier(0.77, 0, 0.175, 1);
--ease-drawer: cubic-bezier(0.32, 0.72, 0, 1);
```

No UI `ease-in` token exists. Exit must respond immediately rather than start
slowly. Normal interactive motion remains below 300 ms.

## Component Matrix

| Component | Frequency | Default motion | Reduced motion |
|---|---:|---|---|
| Button/IconButton | High | Pointer press `scale(.97)`, 120 ms ease-out | Color only |
| Sidebar/Nav/List/Tabs | High | Background, text, and focus color only | Same |
| TextField/TextArea/Search | High | Border/ring/color only | Same |
| Tooltip | Medium-high | First tooltip opacity + `scale(.97)`; consecutive tooltips instant | Opacity |
| Menu/Popover/Select | Occasional | Trigger-origin opacity + `scale(.98)`, 180 ms ease-out | Opacity |
| Dialog | Occasional | Center-origin opacity + `scale(.985)`, 220 ms ease-out | Opacity |
| Workbench/Sheet | Occasional | Source-direction `translateX`, 240 ms drawer curve | Opacity or instant |
| Disclosure | Medium | Chevron rotate; interruptible content reveal, 180 ms ease-out | Instant content |
| Switch | Medium | Thumb translation + track color, 160–180 ms ease-out | State color retained |
| Notice/Toast enter | Occasional | Opacity + short source-direction offset | Opacity |
| Spinner | Waiting | 700–900 ms linear rotation | Rotation retained |
| Skeleton | Waiting | Low-contrast shimmer only for meaningful waits | Static skeleton |

Pointer press motion is gated by pointer capability. Keyboard activation keeps
focus and color feedback without scaling.

## Implementation Rules

- Prefer CSS transitions driven by Radix `data-state` and semantic data
  attributes.
- Use transitions rather than keyframes when an interaction can reverse.
- Reserve Web Animations API for a complex sequence that cannot be expressed
  as an interruptible transition.
- Do not introduce a general motion library without a new architecture decision.
- Animate `transform` and `opacity`; do not animate width, height, margin,
  padding, top, or left for ordinary UI transitions.
- Disclosure may use a grid-row or measured Radix-size transition only when it
  remains reversible and avoids nested layout churn.
- Gate hover-only behavior with
  `(hover: hover) and (pointer: fine)`.
- Never use `transition: all`.
- Popover `transform-origin` follows its trigger/collision result.
- Dialog `transform-origin` remains centered.
- Sheet and Workbench movement follows the edge from which the surface appears.

## Reduced Motion

`prefers-reduced-motion: reduce` removes or makes positional displacement
instant. It does not erase all state transitions:

- focus, selected, error, success, disabled, and destructive colors remain;
- useful short opacity changes may remain;
- progress indicators remain understandable;
- continuous decorative shimmer becomes static;
- opening and closing never delay input or focus transfer.

## Current Review

| Before | After | Why |
|---|---|---|
| Popover/Dialog exit uses `ease-in` | Short interruptible ease-out transition | Closing should respond immediately |
| Popover/Dialog/Disclosure rely on keyframes | State-driven transitions | Rapid reversal should continue from the current frame |
| NavItem and Tabs scale on activation | Color/focus feedback only | These are high-frequency interactions |
| Button press uses only `scale(.99)` | Pointer press uses `scale(.97)` | The feedback becomes perceptible without being playful |
| Hover behavior applies to every pointer | Fine-hover media query | Touch must not retain sticky hover |
| Reduced motion clears all transitions | Remove displacement, retain useful state | Accessibility must not hide status |
| Overlay uses generic fast/normal duration | Component-specific duration tokens | Size and frequency require different timing |
| Disclosure animates raw height | Reversible grid/measured transition | Reduce layout churn and restart artifacts |

## Acceptance

- Run each open/close interaction repeatedly and reverse it mid-transition.
- Inspect interactions at 0.25× speed for origin, overshoot, and exit latency.
- Verify mouse, keyboard, and touch-capability behavior independently.
- Capture normal and reduced-motion fixture screenshots.
- Verify consecutive Tooltip browsing does not repeat delay or entrance motion.
- Record a performance trace for common flows and reject visible dropped frames.
- Review every changed motion using `Before | After | Why`.
