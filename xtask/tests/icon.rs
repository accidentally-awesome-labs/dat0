use std::path::Path;

#[test]
fn generates_pngs_with_valid_headers_and_sizes() {
    let tmp = tempfile::tempdir().unwrap();
    xtask_icon_generate(tmp.path());

    // Linux standalone 512.
    let p512 = tmp.path().join("dat0-512.png");
    assert!(p512.exists(), "missing dat0-512.png");
    assert_png(&p512, 512);

    // iconset members (a representative subset).
    for n in [16u32, 32, 128, 256, 512] {
        let f = tmp.path().join(format!("AppIcon.iconset/icon_{n}x{n}.png"));
        assert!(f.exists(), "missing iconset {n}");
        assert_png(&f, n);
    }
}

fn assert_png(path: &Path, expect_dim: u32) {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[0..8], b"\x89PNG\r\n\x1a\n", "not a PNG: {path:?}");
    let img = image::open(path).unwrap();
    assert_eq!(img.width(), expect_dim);
    assert_eq!(img.height(), expect_dim);
}

// thin wrapper so the test calls the lib fn (icon module is bin-private;
// expose via a `pub` test-only re-export or move icon into a small lib target).
fn xtask_icon_generate(out: &Path) { xtask::icon::generate(out).unwrap(); }
