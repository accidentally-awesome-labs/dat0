use anyhow::{Context, Result};
use image::{Rgba, RgbaImage};
use std::path::{Path, PathBuf};

const BG: Rgba<u8> = Rgba([0x10, 0x12, 0x18, 0xff]); // near-black brand bg
const FG: Rgba<u8> = Rgba([0x4f, 0x9d, 0xff, 0xff]); // brand blue

/// Deterministic placeholder mark: brand-blue disc ("0") with a notch ("d")
/// on a dark rounded square. Pure pixel math — no font dependency.
fn render(size: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(size, size, Rgba([0, 0, 0, 0]));
    let s = size as f32;
    let r = s * 0.18; // corner radius
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32, y as f32);
            // rounded-square background
            let inside = rounded_square(fx, fy, s, r);
            if !inside {
                continue;
            }
            // disc ("0"): ring centered, FG; notch on the right makes a "d".
            let cx = s * 0.5;
            let cy = s * 0.5;
            let dist = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let outer = s * 0.32;
            let inner = s * 0.17;
            let is_ring = dist <= outer && dist >= inner;
            let is_stem = fx >= cx + s * 0.20
                && fx <= cx + s * 0.27
                && fy >= cy - outer
                && fy <= cy + outer;
            img.put_pixel(x, y, if is_ring || is_stem { FG } else { BG });
        }
    }
    img
}

fn rounded_square(x: f32, y: f32, s: f32, r: f32) -> bool {
    let (lo, hi) = (r, s - r);
    let cx = x.clamp(lo, hi);
    let cy = y.clamp(lo, hi);
    ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() <= r
        && x >= 0.0
        && y >= 0.0
        && x < s
        && y < s
}

pub fn generate(out: &Path) -> Result<PathBuf> {
    let iconset = out.join("AppIcon.iconset");
    std::fs::create_dir_all(&iconset).context("create iconset dir")?;

    // .icns / iconset standard sizes with @2x variants.
    let specs: &[(u32, &str)] = &[
        (16, "icon_16x16.png"),
        (32, "icon_16x16@2x.png"),
        (32, "icon_32x32.png"),
        (64, "icon_32x32@2x.png"),
        (128, "icon_128x128.png"),
        (256, "icon_128x128@2x.png"),
        (256, "icon_256x256.png"),
        (512, "icon_256x256@2x.png"),
        (512, "icon_512x512.png"),
        (1024, "icon_512x512@2x.png"),
    ];
    for (size, name) in specs {
        render(*size)
            .save(iconset.join(name))
            .with_context(|| format!("save {name}"))?;
    }
    // Linux standalone.
    render(512)
        .save(out.join("dat0-512.png"))
        .context("save dat0-512.png")?;

    // macOS: assemble .icns from the iconset.
    #[cfg(target_os = "macos")]
    {
        let icns = out.join("dat0.icns");
        let status = std::process::Command::new("iconutil")
            .args(["-c", "icns"])
            .arg(&iconset)
            .arg("-o")
            .arg(&icns)
            .status()
            .context("run iconutil")?;
        anyhow::ensure!(status.success(), "iconutil failed");
    }
    Ok(iconset)
}
