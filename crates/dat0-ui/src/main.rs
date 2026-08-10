//! The `dat0` binary.
//!
//! Everything is in [`dat0_ui::launch`]; this file exists so the ordering there
//! is one readable sequence rather than something spread across `main`.

fn main() -> anyhow::Result<()> {
    dat0_ui::launch::main()
}
