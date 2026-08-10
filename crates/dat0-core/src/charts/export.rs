//! Chart export to files. SVG = the render_svg string. PNG = a BitMapBackend that
//! writes the file directly (plotters `bitmap_encoder`).

use crate::charts::data::PlotTable;
use crate::charts::render::{draw, render_svg};
use crate::charts::spec::ChartSpec;
use plotters::prelude::*;
use std::path::Path;

pub fn export_svg(
    spec: &ChartSpec,
    data: &PlotTable,
    size: (u32, u32),
    path: &Path,
) -> std::io::Result<()> {
    std::fs::write(path, render_svg(spec, data, size))
}

pub fn export_png(
    spec: &ChartSpec,
    data: &PlotTable,
    size: (u32, u32),
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    {
        let root = BitMapBackend::new(path, size).into_drawing_area();
        draw(&root, spec, data)?;
        root.present()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charts::data::{PlotColumn, PlotTable};
    use crate::charts::spec::{ChartSpec, ChartType};

    fn fixture() -> (ChartSpec, PlotTable) {
        let spec = ChartSpec {
            chart_type: ChartType::Line,
            source: "\"t\"".into(),
            x: Some("x".into()),
            y: Some("y".into()),
            group: None,
            color: None,
            title: "T".into(),
        };
        let t = PlotTable {
            rows: 3,
            columns: vec![
                PlotColumn {
                    name: "x".into(),
                    num: Some(vec![1.0, 2.0, 3.0]),
                    text: None,
                },
                PlotColumn {
                    name: "y".into(),
                    num: Some(vec![2.0, 1.0, 3.0]),
                    text: None,
                },
            ],
        };
        (spec, t)
    }

    #[test]
    fn writes_png_with_magic() {
        let (spec, t) = fixture();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.png");
        export_png(&spec, &t, (640, 400), &p).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(&bytes[0..4], &[0x89, b'P', b'N', b'G']);
    }

    #[test]
    fn writes_svg_root() {
        let (spec, t) = fixture();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.svg");
        export_svg(&spec, &t, (640, 400), &p).unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("<svg"));
    }
}
