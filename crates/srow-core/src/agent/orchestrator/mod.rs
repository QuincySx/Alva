// INPUT:  (none)
// OUTPUT: pub mod communication, instance, orchestrator, template, tools
// POS:    Module declaration for the multi-Agent orchestration layer.
//! Agent Orchestrator — the multi-Agent coordination layer.
//!
//! This module implements the core orchestration pattern:
//! - **brain** (Decision Agent): analyzes tasks, selects templates, dispatches work
//! - **reviewer** (Review Agent): checks results, judges quality
//! - **explorer** (Exploration Agent): brainstorms alternatives when reviewer rejects
//!
//! Execution Agents are created dynamically from templates and managed in an instance pool.

pub mod communication;
pub mod instance;
pub mod orchestrator;
pub mod template;
pub mod tools;
