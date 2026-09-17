//! UDAS — Unitary Disk Active Search.
//!
//! ## Three-Layer Architecture
//!
//! ```text
//! Semantic Layer  (LLM)       → restoration.rs
//! Bridge Layer    (embedding) → types.rs (Embedding)
//! Geometry Layer  (pure math) → geometry.rs + interference.rs
//! Precision Layer (adaptive)  → precision_control.rs
//! Memory Layer    (storage)   → memory.rs
//! ```
//!
//! ## Full Pipeline
//!
//! Cold Start → Active Search (interference gradient) → Interference Field
//! → Gradient Descent → Final Collapse (weighted random).
//!
//! ## Interference Model (2026-07-25)
//!
//! Replaced the additive KDE density field with a quantum-inspired
//! interference model. See `interference.rs` and
//! `UDAS-INTERFERENCE-ARCHITECTURE.md` for the mathematical framework.

pub mod breaker;
pub mod density;
pub mod engine;
pub mod env_signal;
pub mod geometry;
pub mod interference;
pub mod memory;
pub mod precision_control;
pub mod restoration;
pub mod types;
