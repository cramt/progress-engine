//! Mirage Mockery: custom Magic cards from a `.card.toml` to PDF and PNG.
//!
//! It prints custom cards and refuses real ones ([`real`]), and it asks
//! Vivisurgeon's Insight whether the rules text is Magic it can read
//! ([`read`]). docs/research/mirage-mockery.md is the plan.

pub mod card;
pub mod read;
pub mod real;
pub mod render;
pub mod symbols;
