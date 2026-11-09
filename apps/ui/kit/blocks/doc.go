// INPUT: Alva Kit components (apps/ui/kit) and view models defined in this package.
// OUTPUT: Alva business blocks: Composer shell and menus, voice input, run status, tool activity, code and citations, attachments, approval and question cards, model, connection and task rows, provider and model form shells.
// POS: [L7 Kit blocks] Presentation and interaction slots only; no product types, no tool or engine names, no business flow (K3 of ADR 0014).

// Package blocks composes Alva Kit components into the product's recurring blocks. Every block takes a view
// model made of enums for kinds and states and plain strings for content, and reports user intent through
// callbacks; mapping product facts to these models belongs to the product layer.
package blocks
