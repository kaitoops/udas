//! udas-introspect — Introspection system for UDAS.
//!
//! Provides three core capabilities:
//! - **CSL Classification**: Analyze git diffs and classify changes by
//!   semantic level (CSL-0 detail → CSL-3 architectural).
//! - **Hot File Management**: Read/write the `UDAS-STATE.md` hot file
//!   with automatic 3-slot ring-buffer backup.
//! - **Timeline Logging**: Append-only event log with correction chains.
//!
//! ## CSL Levels
//!
//! | Level | Definition | Hot file action |
//! |-------|-----------|-----------------|
//! | CSL-0 | Detail (typo, comment, format) | None |
//! | CSL-1 | Local fix (bug fix, param tweak) | None, mark in timeline |
//! | CSL-2 | Functional (interface/data flow/deps) | AGENT evaluates |
//! | CSL-3 | Architectural (crate reorg, direction) | Force update |

pub mod csl;
pub mod diff;
pub mod hot_file;
pub mod timeline;

pub use csl::{CslLevel, CslResult, CslClassifier};
pub use diff::{ChangeFeatures, DiffAnalyzer};
pub use hot_file::HotFileManager;
pub use timeline::{TimelineEntry, TimelineEntryKind, TimelineLogger};
