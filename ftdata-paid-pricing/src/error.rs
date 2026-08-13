//! Error types for the pricing library.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum PricingError {
    #[error("pairs_count must be >= 1, got {got}")]
    InvalidPairCount { got: usize },

    #[error("price computation overflow: {context}")]
    InternalOverflow { context: &'static str },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_pair_count_display_contains_value() {
        let e = PricingError::InvalidPairCount { got: 0 };
        assert!(e.to_string().contains("0"));
        assert!(e.to_string().contains("pairs_count"));
    }

    #[test]
    fn internal_overflow_display_contains_context() {
        let e = PricingError::InternalOverflow {
            context: "test_context",
        };
        assert!(e.to_string().contains("test_context"));
    }

    #[test]
    fn errors_implement_std_error() {
        // Compile-time check: PricingError is a std::error::Error.
        fn assert_error<E: std::error::Error>() {}
        assert_error::<PricingError>();
    }
}
