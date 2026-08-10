//! Backend-generic plotters draw + screen(BGRA)/SVG buffer helpers. One `draw`
//! routine targets both BitMapBackend (screen + PNG export) and SVGBackend (SVG export).

use crate::charts::data::PlotTable;
use crate::charts::spec::{ChartSpec, ChartType};
use plotters::coord::Shift;
use plotters::prelude::*;

type DErr = Box<dyn std::error::Error>;

/// The colours a chart draws with.
///
/// Plotters' stock `RED`/`BLUE`/`GREEN` are a plotting library's idea of a
/// palette, not an application's. The Dioxus build resolves this from
/// `ThemeTokens` so a chart is the same blue as the focus ring; the GPUI build
/// keeps drawing through [`Palette::legacy`], which reproduces the stock
/// colours exactly, so nothing about it changes before it is deleted.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    /// The drawing area fill.
    pub background: RGBColor,
    /// Caption, axis labels and axis lines.
    pub foreground: RGBColor,
    /// Categorical series, cycled by [`Palette::series_at`]. Each single-series
    /// chart type takes a fixed index ([`Palette::LINE`] and friends) so two
    /// types plotted from the same table do not come out identical.
    pub series: Vec<RGBColor>,
}

impl Palette {
    /// Series index used by line and area.
    pub const LINE: usize = 0;
    /// Series index used by scatter.
    pub const SCATTER: usize = 1;
    /// Series index used by bar.
    pub const BAR: usize = 2;
    /// Series index used by histogram bins.
    pub const HISTOGRAM: usize = 0;

    /// Plotters' stock colours, in the roles the pre-palette code used them:
    /// line/area/histogram red, scatter blue, bar green, on white.
    pub fn legacy() -> Self {
        Palette {
            background: WHITE,
            foreground: BLACK,
            series: vec![RED, BLUE, GREEN],
        }
    }

    /// Build from `#rrggbb` strings. A malformed entry falls back to black
    /// rather than failing the render: a chart drawn in the wrong colour is a
    /// far better outcome than a pane that shows nothing.
    pub fn from_css(background: &str, foreground: &str, series: &[&str]) -> Self {
        Palette {
            background: parse_hex(background),
            foreground: parse_hex(foreground),
            series: series.iter().map(|s| parse_hex(s)).collect(),
        }
    }

    /// The `i`th series colour, cycling. Black when the palette is empty.
    pub fn series_at(&self, i: usize) -> RGBColor {
        if self.series.is_empty() {
            return BLACK;
        }
        self.series[i % self.series.len()]
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::legacy()
    }
}

/// `#rrggbb` (or `rrggbb`) to an RGB colour; black on anything else.
fn parse_hex(s: &str) -> RGBColor {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return BLACK;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    match (byte(0), byte(2), byte(4)) {
        (Some(r), Some(g), Some(b)) => RGBColor(r, g, b),
        _ => BLACK,
    }
}

/// Draw `spec` over `data` in plotters' stock colours.
pub fn draw<DB: DrawingBackend>(
    area: &DrawingArea<DB, Shift>,
    spec: &ChartSpec,
    data: &PlotTable,
) -> Result<(), DErr>
where
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    draw_with(area, spec, data, &Palette::legacy())
}

/// Draw `spec` over `data` into a prepared plotters drawing area, in `pal`.
pub fn draw_with<DB: DrawingBackend>(
    area: &DrawingArea<DB, Shift>,
    spec: &ChartSpec,
    data: &PlotTable,
    pal: &Palette,
) -> Result<(), DErr>
where
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    area.fill(&pal.background)?;
    match spec.chart_type {
        ChartType::Line => line_or_area(area, spec, data, false, pal),
        ChartType::Area => line_or_area(area, spec, data, true, pal),
        ChartType::Scatter => scatter(area, spec, data, pal),
        ChartType::Bar => bar(area, spec, data, pal),
        ChartType::Histogram => histogram(area, spec, data, pal),
        ChartType::BoxPlot => boxplot(area, spec, data, pal),
        ChartType::Heatmap => heatmap(area, spec, data, pal),
    }
}

fn finite_pairs(xs: &[f64], ys: &[f64]) -> Vec<(f64, f64)> {
    xs.iter()
        .zip(ys)
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .map(|(a, b)| (*a, *b))
        .collect()
}

fn range(v: &[f64]) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &x in v.iter().filter(|x| x.is_finite()) {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    if !lo.is_finite() {
        (0.0, 1.0)
    } else if (hi - lo).abs() < f64::EPSILON {
        (lo - 1.0, hi + 1.0)
    } else {
        (lo, hi)
    }
}

/// Shared `ChartBuilder::on(area)...build_cartesian_2d` for the f64×f64 types
/// (line/area/scatter/bar/histogram/heatmap). Boxplot uses a segmented x-axis
/// and so inlines its own builder.
fn cartesian<'a, DB: DrawingBackend>(
    area: &'a DrawingArea<DB, Shift>,
    spec: &ChartSpec,
    xr: std::ops::Range<f64>,
    yr: std::ops::Range<f64>,
    pal: &Palette,
) -> Result<
    ChartContext<
        'a,
        DB,
        Cartesian2d<plotters::coord::types::RangedCoordf64, plotters::coord::types::RangedCoordf64>,
    >,
    DErr,
>
where
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let mut cb = ChartBuilder::on(area);
    cb.caption(
        spec.title.clone(),
        ("sans-serif", 18).into_font().color(&pal.foreground),
    )
    .x_label_area_size(30)
    .y_label_area_size(44)
    .margin(8);
    let mut chart = cb.build_cartesian_2d(xr, yr)?;
    chart
        .configure_mesh()
        .x_desc(spec.x.clone().unwrap_or_default())
        .y_desc(spec.y.clone().unwrap_or_default())
        .label_style(("sans-serif", 12).into_font().color(&pal.foreground))
        .axis_desc_style(("sans-serif", 12).into_font().color(&pal.foreground))
        .axis_style(pal.foreground)
        // Grid lines follow the text, faintly. A fixed grey disappears on a
        // dark surface and shouts on a light one.
        .light_line_style(pal.foreground.mix(0.08))
        .bold_line_style(pal.foreground.mix(0.16))
        .draw()?;
    Ok(chart)
}

fn line_or_area<DB: DrawingBackend>(
    area: &DrawingArea<DB, Shift>,
    spec: &ChartSpec,
    d: &PlotTable,
    fill: bool,
    pal: &Palette,
) -> Result<(), DErr>
where
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let x = d.num_at(0).ok_or("x must be numeric")?; // contract: [0]=x:num
    let y = d.num_at(1).ok_or("y must be numeric")?; // [1]=y:num
    let pts = finite_pairs(x, y);
    let (x0, x1) = range(x);
    let (y0, y1) = range(y);
    let c = pal.series_at(Palette::LINE);
    // Area baselines at 0, so include the origin when the data is all-positive.
    let mut chart = cartesian(area, spec, x0..x1, y0.min(0.0)..y1, pal)?;
    if fill {
        chart.draw_series(AreaSeries::new(pts.iter().copied(), 0.0, c.mix(0.2)).border_style(c))?;
    } else {
        chart.draw_series(LineSeries::new(pts.iter().copied(), c.stroke_width(2)))?;
    }
    Ok(())
}

fn scatter<DB: DrawingBackend>(
    area: &DrawingArea<DB, Shift>,
    spec: &ChartSpec,
    d: &PlotTable,
    pal: &Palette,
) -> Result<(), DErr>
where
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let x = d.num_at(0).ok_or("x must be numeric")?; // [0]=x:num
    let y = d.num_at(1).ok_or("y must be numeric")?; // [1]=y:num
    let pts = finite_pairs(x, y);
    let (x0, x1) = range(x);
    let (y0, y1) = range(y);
    let c = pal.series_at(Palette::SCATTER);
    let mut chart = cartesian(area, spec, x0..x1, y0..y1, pal)?;
    // CRITICAL NOTE 1: explicit Circle marker (bare PointSeries::new is ambiguous on 0.3.7).
    chart.draw_series(pts.iter().map(|&(x, y)| Circle::new((x, y), 3, c.filled())))?;
    Ok(())
}

fn bar<DB: DrawingBackend>(
    area: &DrawingArea<DB, Shift>,
    spec: &ChartSpec,
    d: &PlotTable,
    pal: &Palette,
) -> Result<(), DErr>
where
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let cats = d.text_at(0).ok_or("category must be text")?; // [0]=category:text
    let vals = d.num_at(1).ok_or("value must be numeric")?; // [1]=value:num
    let n = cats.len().min(vals.len());
    let (_, vmax) = range(&vals[..n]);
    // One colour for every bar: the categories are the x-axis, so colouring
    // them individually would encode the same thing twice.
    let c = pal.series_at(Palette::BAR);
    let mut chart = cartesian(
        area,
        spec,
        0.0..(n.max(1) as f64),
        0.0..(vmax.max(1.0) * 1.1),
        pal,
    )?;
    chart.draw_series((0..n).map(|i| {
        let v = if vals[i].is_finite() { vals[i] } else { 0.0 };
        Rectangle::new(
            [(i as f64 + 0.1, 0.0), (i as f64 + 0.9, v)],
            c.mix(0.7).filled(),
        )
    }))?;
    Ok(())
}

fn histogram<DB: DrawingBackend>(
    area: &DrawingArea<DB, Shift>,
    spec: &ChartSpec,
    d: &PlotTable,
    pal: &Palette,
) -> Result<(), DErr>
where
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    let v: Vec<f64> = d
        .num_at(0) // contract: [0]=values:num
        .ok_or("histogram needs a numeric column")?
        .iter()
        .copied()
        .filter(|x| x.is_finite())
        .collect();
    let (lo, hi) = range(&v);
    let bins = crate::charts::histogram_bins(lo, hi, &v, 20); // reuse existing pure binning
    let ymax = bins.iter().map(|b| b.count).max().unwrap_or(1) as f64;
    let c = pal.series_at(Palette::HISTOGRAM);
    let mut chart = cartesian(area, spec, lo..hi, 0.0..(ymax * 1.1).max(1.0), pal)?;
    chart.draw_series(
        bins.iter()
            .map(|b| Rectangle::new([(b.lo, 0.0), (b.hi, b.count as f64)], c.mix(0.6).filled())),
    )?;
    Ok(())
}

fn boxplot<DB: DrawingBackend>(
    area: &DrawingArea<DB, Shift>,
    spec: &ChartSpec,
    d: &PlotTable,
    pal: &Palette,
) -> Result<(), DErr>
where
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    use plotters::prelude::{Boxplot, Quartiles, SegmentValue};
    let cats = d.text_at(0).ok_or("boxplot needs a category")?; // [0]=category:text
    let vals = d.num_at(1).ok_or("boxplot needs a value")?; // [1]=value:num
    // Group values by category, preserving first-seen order.
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<f64>> = std::collections::HashMap::new();
    for (c, v) in cats.iter().zip(vals).filter(|(_, v)| v.is_finite()) {
        if !groups.contains_key(c) {
            order.push(c.clone());
        }
        groups.entry(c.clone()).or_default().push(*v);
    }
    let (mut ylo, mut yhi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in groups.values() {
        for &x in v {
            ylo = ylo.min(x);
            yhi = yhi.max(x);
        }
    }
    if !ylo.is_finite() {
        ylo = 0.0;
        yhi = 1.0;
    }
    // Pad the y-range so single-value groups (zero spread) still draw a box.
    let pad = ((yhi - ylo).abs() * 0.1).max(1.0);
    let keys: Vec<&str> = order.iter().map(String::as_str).collect();
    let mut cb = ChartBuilder::on(area);
    cb.caption(
        spec.title.clone(),
        ("sans-serif", 18).into_font().color(&pal.foreground),
    )
    .x_label_area_size(40)
    .y_label_area_size(44)
    .margin(8);
    // Segmented categorical x-axis; y is f32 — plotters' Quartiles/Boxplot resolve
    // their YType to f32, so the y-range must be f32 to match (CRITICAL NOTE 2).
    let mut chart = cb.build_cartesian_2d(
        keys[..].into_segmented(),
        ((ylo - pad) as f32)..((yhi + pad) as f32),
    )?;
    chart
        .configure_mesh()
        .label_style(("sans-serif", 12).into_font().color(&pal.foreground))
        .axis_style(pal.foreground)
        .light_line_style(pal.foreground.mix(0.08))
        .bold_line_style(pal.foreground.mix(0.16))
        .draw()?;
    // The segmented coordinate over `[&str]` yields `SegmentValue<&&str>`, so
    // iterate `keys` (each `k` is `&&str`) — borrowing the slice keeps it alive.
    // Boxes take the foreground, not a series colour: a box plot's boxes are
    // structure (quartiles + whiskers), not one series per category, and on a
    // dark surface plotters' default black is invisible.
    chart.draw_series(keys.iter().map(|k| {
        let q = Quartiles::new(&groups[*k]);
        Boxplot::new_vertical(SegmentValue::CenterOf(k), &q).style(pal.foreground)
    }))?;
    Ok(())
}

fn heatmap<DB: DrawingBackend>(
    area: &DrawingArea<DB, Shift>,
    spec: &ChartSpec,
    d: &PlotTable,
    pal: &Palette,
) -> Result<(), DErr>
where
    DB::ErrorType: std::error::Error + Send + Sync + 'static,
{
    use plotters::style::HSLColor;
    let xs = d.text_at(0).ok_or("heatmap x")?; // [0]=x:text
    let ys = d.text_at(1).ok_or("heatmap y")?; // [1]=y:text
    let vs = d.num_at(2).ok_or("heatmap value")?; // [2]=value:num
    let mut xcat: Vec<String> = Vec::new();
    let mut ycat: Vec<String> = Vec::new();
    for x in xs {
        if !xcat.contains(x) {
            xcat.push(x.clone());
        }
    }
    for y in ys {
        if !ycat.contains(y) {
            ycat.push(y.clone());
        }
    }
    let (vlo, vhi) = range(vs);
    // The ramp stays a perceptual blue→red HSL sweep rather than a theme
    // colour: it encodes a continuous magnitude, and a two-token gradient
    // through the app's palette reads as two categories, not a scale.
    let mut chart = cartesian(
        area,
        spec,
        0.0..(xcat.len().max(1) as f64),
        0.0..(ycat.len().max(1) as f64),
        pal,
    )?;
    let idx = |cat: &[String], val: &str| cat.iter().position(|c| c == val).unwrap_or(0);
    chart.draw_series(xs.iter().zip(ys).zip(vs).map(|((xv, yv), v)| {
        let xi = idx(&xcat, xv) as f64;
        let yi = idx(&ycat, yv) as f64;
        let t = if (vhi - vlo).abs() < f64::EPSILON {
            0.5
        } else {
            (v - vlo) / (vhi - vlo)
        };
        Rectangle::new(
            [(xi, yi), (xi + 1.0, yi + 1.0)],
            HSLColor(0.6 - 0.6 * t, 0.7, 0.45).filled(),
        )
    }))?;
    Ok(())
}

/// Render to an SVG string in plotters' stock colours (export path).
pub fn render_svg(spec: &ChartSpec, data: &PlotTable, size: (u32, u32)) -> String {
    render_svg_with(spec, data, size, &Palette::legacy())
}

/// Render to an SVG string in `pal`. This is the Dioxus screen path: the chart
/// pane drops the string straight into the DOM, so there is no bitmap, no
/// `image` crate and no supersampling factor to keep in step with the display.
pub fn render_svg_with(
    spec: &ChartSpec,
    data: &PlotTable,
    (w, h): (u32, u32),
    pal: &Palette,
) -> String {
    let mut s = String::new();
    {
        let root = SVGBackend::with_string(&mut s, (w, h)).into_drawing_area();
        let _ = draw_with(&root, spec, data, pal);
        let _ = root.present();
    }
    s
}

/// Render to a BGRA buffer for gpui `img(RenderImage)`. `(w,h)` are PHYSICAL pixels.
pub fn render_bgra(spec: &ChartSpec, data: &PlotTable, (w, h): (u32, u32)) -> (Vec<u8>, u32, u32) {
    let mut rgb = vec![0u8; (w * h * 3) as usize];
    {
        let root = BitMapBackend::with_buffer(&mut rgb, (w, h)).into_drawing_area();
        let _ = draw(&root, spec, data);
        let _ = root.present();
    }
    let mut bgra = vec![0u8; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        bgra[4 * i] = rgb[3 * i + 2];
        bgra[4 * i + 1] = rgb[3 * i + 1];
        bgra[4 * i + 2] = rgb[3 * i];
        bgra[4 * i + 3] = 255;
    }
    (bgra, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charts::data::{PlotColumn, PlotTable};
    use crate::charts::spec::{ChartSpec, ChartType};

    fn num(name: &str, v: Vec<f64>) -> PlotColumn {
        PlotColumn {
            name: name.into(),
            num: Some(v),
            text: None,
        }
    }
    fn txt(name: &str, v: &[&str]) -> PlotColumn {
        PlotColumn {
            name: name.into(),
            num: None,
            text: Some(v.iter().map(|s| s.to_string()).collect()),
        }
    }

    /// A synthetic plot table shaped to each type's POSITIONAL column contract
    /// (see charts/query.rs): the column order is what render.rs reads.
    fn table_for(t: ChartType) -> PlotTable {
        let columns = match t {
            ChartType::Line | ChartType::Area | ChartType::Scatter => {
                vec![num("x", vec![1.0, 2.0, 3.0]), num("y", vec![2.0, 1.0, 3.0])]
            }
            ChartType::Bar | ChartType::BoxPlot => {
                vec![txt("k", &["a", "b", "a"]), num("v", vec![1.0, 3.0, 2.0])]
            }
            ChartType::Histogram => vec![num("v", vec![1.0, 2.0, 2.0, 3.0, 9.0])],
            ChartType::Heatmap => vec![
                txt("x", &["a", "a", "b"]),
                txt("y", &["p", "q", "p"]),
                num("v", vec![1.0, 4.0, 2.0]),
            ],
        };
        PlotTable {
            rows: columns
                .first()
                .map(|c| {
                    c.num
                        .as_ref()
                        .map(|v| v.len())
                        .or(c.text.as_ref().map(|v| v.len()))
                        .unwrap_or(0)
                })
                .unwrap_or(0),
            columns,
        }
    }

    fn spec(t: ChartType) -> ChartSpec {
        ChartSpec {
            chart_type: t,
            source: "\"t\"".into(),
            x: Some("x".into()),
            y: Some("y".into()),
            group: None,
            color: None,
            title: "T".into(),
        }
    }

    #[test]
    fn every_type_renders_svg_nonempty() {
        for t in ChartType::ALL {
            let svg = render_svg(&spec(t), &table_for(t), (400, 300));
            assert!(svg.contains("<svg"), "{t:?} produced no svg");
        }
    }

    /// The whole point of the palette: the emitted SVG is painted in the
    /// colours handed in, not in plotters' stock red/blue/green.
    #[test]
    fn a_themed_render_uses_the_palette_and_not_the_stock_colours() {
        let pal = Palette::from_css("#101318", "#c9d1d9", &["#58a6ff", "#bc8cff", "#56d364"]);
        let svg = render_svg_with(
            &spec(ChartType::Line),
            &table_for(ChartType::Line),
            (400, 300),
            &pal,
        );
        assert!(
            svg.contains("#58A6FF") || svg.contains("#58a6ff"),
            "line is not the palette's first series: {svg:.400}"
        );
        assert!(
            !svg.contains("#FF0000") && !svg.contains("#ff0000"),
            "stock red survived a themed render"
        );
        assert!(
            svg.contains("#101318"),
            "the background is not the palette's"
        );
    }

    /// `draw` must keep painting exactly as it did before the palette existed —
    /// it is what the GPUI build still renders with.
    #[test]
    fn the_legacy_palette_is_the_stock_colours() {
        let p = Palette::legacy();
        assert_eq!(p.series_at(Palette::LINE), RED);
        assert_eq!(p.series_at(Palette::SCATTER), BLUE);
        assert_eq!(p.series_at(Palette::BAR), GREEN);
        // Cycling, not clamping: index 3 wraps to the first series.
        assert_eq!(p.series_at(3), RED);
    }

    /// A malformed token must degrade to a drawable colour, never panic: the
    /// palette is built from strings that come out of a JSON theme file.
    #[test]
    fn a_malformed_colour_falls_back_instead_of_panicking() {
        let p = Palette::from_css("not-a-colour", "#zzzzzz", &["", "#58a6ff"]);
        assert_eq!(p.background, BLACK);
        assert_eq!(p.foreground, BLACK);
        assert_eq!(p.series_at(0), BLACK);
        assert_eq!(p.series_at(1), RGBColor(0x58, 0xa6, 0xff));
    }

    /// An empty series list is a theme bug, but it must still render.
    #[test]
    fn an_empty_palette_still_draws() {
        let pal = Palette::from_css("#ffffff", "#000000", &[]);
        let svg = render_svg_with(
            &spec(ChartType::Bar),
            &table_for(ChartType::Bar),
            (200, 150),
            &pal,
        );
        assert!(svg.contains("<svg"));
    }

    #[test]
    fn bgra_buffer_has_right_length() {
        let (buf, w, h) = render_bgra(
            &spec(ChartType::Line),
            &table_for(ChartType::Line),
            (200, 150),
        );
        assert_eq!(buf.len(), (w * h * 4) as usize);
        assert_eq!((w, h), (200, 150));
    }
}
