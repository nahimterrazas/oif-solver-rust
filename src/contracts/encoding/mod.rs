// Re-export the traits and types
pub mod traits;
pub mod foundry_encoder;
pub mod alloy_encoder;

// Re-export everything for easy access
pub use traits::*;
pub use foundry_encoder::FoundryEncoder;
pub use alloy_encoder::AlloyEncoder; 