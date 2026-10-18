//! Mirage Mockery: custom Magic cards from a `.card.toml` to PDF and PNG.
//!
//! It asks Vivisurgeon's Insight whether the rules text is Magic it can read
//! ([`read`]). docs/research/mirage-mockery.md is the plan.

pub mod card;
pub mod read;
pub mod render;
pub mod symbols;
