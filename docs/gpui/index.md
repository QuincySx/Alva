## GPUI
### Create new GPUI components
Use when building components, writing UI elements, or creating new component implementations.
`./new-component.md`

### Context management in GPUI including App, Window, and AsyncApp
Use when working with contexts, entity updates, or window operations. Different context types provide different capabilities for UI rendering, entity management, and async operations.
`./context.md`

### Implementing custom elements using GPUI's low-level Element API (vs. high-level Render/RenderOnce APIs)
Use when you need maximum control over layout, prepaint, and paint phases for complex, performance-critical custom UI components that cannot be achieved with Render/RenderOnce traits.
`./element.md`

### Global state management in GPUI
Use when implementing global state, app-wide configuration, or shared resources.
`./global.md`

### Action definitions and keyboard shortcuts in GPUI.
Use when implementing actions, keyboard shortcuts, or key bindings.
`./action.md`

### Event handling and subscriptions in GPUI
Use when implementing events, observers, or event-driven patterns. Supports custom events, entity observations, and event subscriptions for coordinating between components.
`./event.md`

### Async operations and background tasks in GPUI
Use when working with async, spawn, background tasks, or concurrent operations. Essential for handling async I/O, long-running computations, and coordinating between foreground UI updates and background work.
`./async.md`

### Entity management and state handling in GPUI
Use when working with entities, managing component state, coordinating between components, handling async operations with state updates, or implementing reactive patterns. Entities provide safe concurrent access to application state.
`./entity.md`

### Focus management and keyboard navigation in GPUI
Use when handling focus, focus handles, or keyboard navigation. Enables keyboard-driven interfaces with proper focus tracking and navigation between focusable elements.
`./focus-handle.md`

### GPUI Component project style guide based on gpui-component code patterns
Use when writing new components, reviewing code, or ensuring consistency with existing gpui-component implementations. Covers component structure, trait implementations, naming conventions, and API patterns observed in the actual codebase.
`./style-guide.md`

### Layout and styling in GPUI
Use when styling components, layout systems, or CSS-like properties.
`./layout-and-style.md`

## Test
### GUI Test
Writing tests for GPUI applications. Use when testing components, async operations, or UI behavior.
`./test.md`
