//! Chancellor of the Spires: a rules engine for Magic.
//!
//! The game is a typed Rust state machine: priority, the stack, choices,
//! hidden information, randomness. What is true now (characteristics
//! through the layers, CR 613, and legality) is derived with ascent from
//! Insight's ability trees, lowered into the engine's own effects
//! (ADR-0037). docs/research/rules-engine.md is the plan.

pub mod layers;
pub mod model;

pub use layers::characteristics;
