//! The grid's data plane: paged Arrow access, cell rendering to display
//! strings, the selection model, and TSV clipboard round-tripping.
//!
//! Everything here is about *what* the grid shows, never how it is painted.

pub mod clipboard;
pub mod data_source;
pub mod edit_ops;
pub mod header;
pub mod keymap;
pub mod renderers;
pub mod selection;

pub use data_source::GridDataSource;
pub use header::{ColumnHeaderZone, HEADER_FUNNEL_PX, HEADER_GRIP_PX, HEADER_SORT_PX, zone_from_x};
