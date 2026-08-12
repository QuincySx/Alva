# ADR-0006: Hub Tauri UI System

- Status: Accepted
- Date: 2026-08-12
- Scope: ALVA Hub Desktop UI architecture, component ownership, motion, reference alignment, and visual acceptance

## Context

ALVA Hub Desktop has repeatedly improved individual screens without first
settling the ownership of application regions, reusable surfaces, controls,
overlay dimensions, scrolling, or motion. That produced locally adjusted
widths and heights, inconsistent hover states, competing scroll containers,
and dialogs or popovers that looked unrelated even when they represented the
same interaction class.

The project now has 67 local Cindy reference screenshots, 67 state-level
lineworks, and seven aggregate wireframes. Cindy is a useful interaction and
information-architecture reference, but Hub has its own identity, product
domains, and technical constraints. The reference therefore needs an explicit
authority boundary rather than informal visual copying.

The product UI is implemented by `crates/alva-hub-desktop` with Tauri, React,
Radix primitives, and CSS. The previous GPUI product-UI direction is retired.
Existing uncommitted UI refactors contain useful component work, but they have
not passed a systematic architecture or visual review.

## Decision

### Product and reference authority

- ALVA Hub Desktop remains a Tauri + React product.
- Cindy is the authority for page structure, information hierarchy, density,
  trigger relationships, overlay direction, interaction paths, and closing
  behavior for the referenced states.
- ALVA owns color semantics, typography tokens, icon treatment, branding, and
  final visual emphasis.
- The target is therefore “Cindy layout and interaction, ALVA visual language,”
  not pixel-for-pixel brand cloning.
- The 67 state-level lineworks remain interaction evidence.
- The seven aggregate wireframes are redrawn at a consistent scale and gain
  explicit component, width, docking, and scroll-ownership annotations.

### Five-layer component model

The dependency direction is fixed:

```text
Tokens → Controls → Regions/Surfaces → Domain Components → Pages
```

- Tokens own semantic color, typography, spacing, radius, shadow, z-layer,
  density, size, duration, and easing values.
- Controls own primitive interaction states, accessibility, and motion.
- Regions and surfaces own width, scroll, docking, responsive behavior, and
  repeated visual grouping.
- Domain components combine lower layers with Hub data and actions.
- Pages only arrange public components and bind product state.
- Lower layers never import business domains.
- Pages and domain components may not restyle primitive hover, active, focus,
  disabled, loading, open, or close behavior.
- Layout overrides are expressed through documented component variants and
  slots rather than arbitrary descendant selectors.

### Primitive and API strategy

- Radix remains the accessibility and interaction primitive foundation for
  focus management, keyboard behavior, portals, and collision handling.
- Business code imports only the ALVA `ui/` facade and never imports Radix
  directly.
- Reusable components use composition plus finite typed variants.
- Large configuration-driven God Components are rejected.
- Unbounded free-form styling APIs are rejected for visual states and motion.
- Two density variants are supported: `default` for pages and forms, and
  `compact` for Sidebar, menus, model lists, and Workbench tools.
- Pages do not invent additional control heights.

### Layout and responsive ownership

- `AppFrame` owns the Sidebar/Main/Workbench relationship and global
  breakpoints.
- `ContentFrame` owns the main reading width and horizontal gutter.
- `ScrollRegion` and `WorkbenchPage` each own exactly one scroll container for
  their region.
- `DockRegion` owns persistent composer, approval, and status content.
- On wide desktop windows the Workbench is docked and the Main region adjusts
  as a whole.
- On narrower windows the Workbench becomes an overlay Sheet; the Sidebar may
  also become an overlay on the smallest breakpoint.
- Breakpoints alter region relationships, not individual business-component
  geometry.

### Styling architecture

- Styling remains zero-runtime layered global CSS.
- CSS cascade layers separate tokens, primitives, regions, domains, pages,
  and tightly controlled utilities.
- Components expose stable class and data-attribute contracts.
- `tokens.css` is the only owner of shared visual and motion constants.
- Primitive CSS owns control state and animation.
- Domain and page CSS may arrange public slots but may not reach into primitive
  internals.
- Dark theme is implemented first. Semantic tokens must support a later light
  mapping without component rewrites.
- English and Chinese fixture copy must both avoid clipping and broken layout;
  full localization infrastructure is not required by this decision.

### Motion system

- Motion follows the frequency and purpose rules from
  `emilkowalski/skills`.
- A motion must provide feedback, communicate spatial relationship, explain a
  state change, or prevent a jarring transition. Decorative motion without a
  purpose is removed.
- High-frequency navigation, tabs, list selection, search, and keyboard-driven
  commands do not translate or scale.
- Pointer button presses use a short restrained scale response. Keyboard
  activation uses color/focus feedback without displacement.
- Tooltip, menu, popover, dialog, disclosure, notice, and Workbench transitions
  use component-specific duration and easing tokens.
- Normal UI motion remains under 300 ms.
- Interruptible CSS transitions and Radix `data-state` are preferred over
  keyframes. Web Animations API is reserved for a genuinely complex sequence;
  no general motion runtime is introduced.
- UI enter and exit transitions do not use `ease-in`.
- Motion normally changes `transform` and `opacity`, not layout dimensions.
- Hover motion is gated by `(hover: hover) and (pointer: fine)`.
- `prefers-reduced-motion` removes displacement while retaining useful color
  and opacity state feedback.

### Information disclosure

- Default product UI presents user-relevant state, the reason for a problem,
  and the next available action.
- Provider details, internal IDs, raw logs, billing details, and raw events are
  progressively disclosed.
- Agent remains the child-agent relationship surface.
- Inspector remains the execution-debugging surface.
- Technical detail is not removed when it is truthful and available; it moves
  to the appropriate detail or Inspector surface.

### Component lab and test data

- A development/test-only in-application UI Lab renders controls, regions,
  domain components, and aggregate page states inside the real Vite/Tauri web
  environment.
- UI Lab uses deterministic fixtures so states and screenshots are reproducible.
- A small set of live scenarios binds the same components to real stores and
  Tauri commands.
- UI Lab is not exposed in normal production navigation.

### Visual and interaction acceptance

- Playwright drives the web UI Lab at fixed viewports and captures stable
  screenshots.
- Full Tauri startup and native command integration receive separate smoke
  coverage rather than owning the complete visual matrix.
- Visual acceptance combines machine-generated screenshots and difference
  images with human review. Strict pixel equality is not required because ALVA
  owns visual tokens.
- Review records use `Before | After | Why` and must explain the user-visible
  improvement.
- Components cover every applicable default, hover, active, focus, disabled,
  loading, open, close, error, empty, and reduced-motion state.
- Completion requires full keyboard operation, visible focus, WCAG AA contrast
  for text and controls, pointer/touch behavior, and reduced-motion behavior.
- macOS is the first visual-acceptance platform. Components and APIs remain
  cross-platform so Windows and Linux can receive later platform-specific QA.
- A page is complete only when deterministic fixture states and the real
  product flow both pass.

### Existing uncommitted UI work

- Existing uncommitted UI changes are not discarded wholesale or accepted as
  the target by default.
- Their current appearance is captured before restructuring.
- Each change is audited against this ADR and either migrated into the new
  layers, rewritten, or removed with an explicit reason.

## Consequences

### Positive

- Width, scrolling, docking, focus, overlay, and motion problems have a single
  owner instead of page-local patches.
- Cindy can guide behavior without erasing ALVA's identity.
- Deterministic fixtures make all component states reproducible.
- Typed variants and strict layering prevent visual drift as Hub adds tools and
  pages.
- Motion becomes predictable, interruptible, accessible, and cheap to render.

### Negative

- Existing pages must be migrated rather than merely restyled.
- The UI Lab and Playwright baseline add development surface and maintenance.
- Strict component boundaries can make a one-off visual tweak slower until the
  correct shared variant exists.
- Dark-first delivery defers full light-theme visual acceptance.
- Windows and Linux do not block the first macOS visual milestone.

## Verification

- The seven aggregate wireframes name every region and its width, docking, and
  scroll owner.
- Business components contain no native interactive elements or direct Radix
  imports outside the UI facade.
- CSS contains no `transition: all`, UI `ease-in`, page-local shared constants,
  or high-frequency navigation displacement.
- Sidebar, Main, Composer, notices, and Workbench remain aligned while the
  window is resized through every documented breakpoint.
- UI Lab renders the defined component-state matrix with deterministic data.
- Playwright captures each mapped reference state at fixed viewports.
- Human review validates hierarchy, interaction path, density, and motion
  against the Cindy reference and ALVA token intent.
- English and Chinese fixtures do not clip or force unsupported control sizes.
- Real Hub stores and Tauri commands exercise the same components after fixture
  acceptance.
- The 67 referenced states pass before the redesign is declared complete.
