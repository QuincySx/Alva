// INPUT: Alva design tokens (kit/theme), icons (kit/icon) and MyGo v0.7.4 *Base controls.
// OUTPUT: Alva Kit, the product-neutral component library: Text, Icon, Button, IconButton, Input, TextArea, Field, Select, Checkbox, Switch, Tooltip, Menu.
// POS: [L7 Kit] Thin Alva skin over MyGo; depends only on the standard library, MyGo and itself, and is built by desktop-tagged files so the token/icon data packages stay portable.

// Package kit is Alva Kit (ADR 0014): immediate-mode components of the shape
// Name(c, data, NameProps) ui.Element. Props zero values are the design defaults;
// only Disabled, Busy, Invalid and ReadOnly are state Props, while Hover, Pressed and
// Focus come from the runtime. See AGENTS.md for the conventions.
package kit
