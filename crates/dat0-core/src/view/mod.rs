//! Per-tab view state: the transformation stack, the column projection, and
//! the pure halves of the header and filter surfaces.
//!
//! `ViewModel` is the undo zipper the grid header, the pipeline bar and the
//! inspector all read. Mutators are pure and return a `ViewChange`; the engine
//! round-trip is the caller's job, which is why this compiles without a
//! renderer.

pub mod column_view;
pub mod distinct_values;
pub mod export_dialog;
pub mod filter_popover;
pub mod model;
pub mod pipeline_bar;
pub mod sort_header;
pub mod spawn;

pub use column_view::fold_columns;
pub use model::{HISTORY_CAP, ViewChange, ViewModel, route_outcome};
pub use spawn::start_view_change;
