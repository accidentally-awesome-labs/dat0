//! dat0 `.dat0` package format: model, writer, reader, diff, replay.
pub mod error;
pub mod model;
pub use error::{FormatError, Result};
pub use model::*;
/// Package format major version. dat0 1.x reads format 1.x (design D8).
pub const FORMAT_VERSION: u32 = 1;
