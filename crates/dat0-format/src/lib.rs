//! dat0 `.dat0` package format: model, writer, reader, diff, replay.
pub mod diff;
pub mod error;
pub mod model;
pub mod reader;
pub mod replay;
pub mod writer;
pub use diff::{PackageDiff, compute, diff};
pub use error::{FormatError, Result};
pub use model::*;
pub use reader::Reader;
pub use writer::Writer;
/// Package format major version. dat0 1.x reads format 1.x (design D8).
pub const FORMAT_VERSION: u32 = 1;

/// The zip entry that holds table `name`'s data.
pub fn data_entry(name: &str) -> String {
    format!("data/{name}.parquet")
}

/// Whether `name` can name a table in a package.
///
/// A table's name is also a file name: its data is `data/<name>.parquet`, and
/// unpacking, replaying and writing a package each join that onto a directory.
/// So it must be one path component that stays where it is joined: not empty,
/// not `.` or `..`, and without a path separator (either kind), a NUL or any
/// other control character.
pub fn is_safe_table_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| c == '/' || c == '\\' || c.is_control())
}
