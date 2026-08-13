//! Error types for the pricing library.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum PricingError {
    #[error("pairs_count must be >= 1, got {got}")]
    InvalidPairCount { got: usize },

    #[error("price computation overflow: {context}")]
    InternalOverflow { context: &'static str },
}
