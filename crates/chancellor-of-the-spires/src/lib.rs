//! Chancellor of the Spires: a rules engine for Magic.
//!
//! The game is a typed Rust state machine: priority, the stack, choices,
//! hidden information, randomness. What is true now (characteristics
//! through the layers, CR 613, and legality) is derived by ascent from
//! Insight's ability trees as facts under one fixed set of general rules.
//! docs/research/rules-engine.md is the plan and the measurements behind it.
