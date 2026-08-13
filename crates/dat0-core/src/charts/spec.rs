//! Chart specification types. Relocated into `dat0-engine` (P9a-2) so the
//! `.dat0` package format (`dat0-format`) can share the same serde definition —
//! mirroring how `dat0_engine::transform::Transformation` is shared. Re-exported
//! here so every `crate::charts::spec::*` importer is unchanged.
pub use dat0_engine::chart_spec::*;
