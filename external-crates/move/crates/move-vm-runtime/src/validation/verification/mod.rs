//! Static verification of Move packages.
//!
//! This module performs comprehensive static analysis to ensure Move code
//! adheres to the language's safety guarantees. Verification catches errors
//! at load time rather than runtime, providing strong safety properties.
//!
//! Verification checks:
//! - Type safety and type parameter bounds
//! - Reference safety and borrowing rules
//! - Resource safety (no duplication or loss)
//! - Stack safety and control flow integrity
//! - Cross-package linkage compatibility

pub mod ast;
pub mod linkage;
pub mod translate;
