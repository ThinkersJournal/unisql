// SPDX-License-Identifier: MIT OR Apache-2.0
//! UniSQL core: the dialect-neutral IR, the tiers and report, and the shared printer.
//!
//! This crate knows no dialect. A dialect supplies a [`Target`] (writer side) and a reader that
//! produces [`ast::Statement`]. See `docs/DESIGN.md` of the workspace.

pub mod ast;
pub mod needs;
pub mod print;
pub mod report;
pub mod rewrite;
pub mod target;

pub use print::write;
pub use report::{
    Allowance, Entry, Options, ParamBinding, Read, ReadError, ReadNote, Report, Tier, Translation,
    Untranslatable,
};
pub use target::{Ctx, Target};
