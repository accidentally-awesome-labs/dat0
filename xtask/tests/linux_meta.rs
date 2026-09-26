use xtask::linux::{desktop_entry, mime_xml};

#[test]
fn desktop_entry_declares_dat0_mime_and_file_arg() {
    let d = desktop_entry();
    assert!(d.contains("[Desktop Entry]"));
    assert!(d.contains("Exec=dat0 %F")); // receives the file path as argv
    assert!(d.contains("MimeType=application/x-dat0;"));
    assert!(d.contains("Icon=dat0"));
}

#[test]
fn mime_xml_registers_dat0_glob() {
    let m = mime_xml();
    assert!(m.contains("application/x-dat0"));
    assert!(m.contains("*.dat0"));
}

use std::path::Path;
use xtask::linux::{
    BUNDLED_LIBRARIES, HOST_LIBRARIES, RPATH, classify, needed, parse_needed, parse_rpath,
};

#[test]
fn readelf_needed_lines_are_parsed_in_order() {
    let out = "\
Dynamic section at offset 0x4617f40 contains 40 entries:
  Tag        Type                         Name/Value
 0x0000000000000001 (NEEDED)             Shared library: [libxdo.so.3]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
 0x000000000000001d (RUNPATH)            Library runpath: [$ORIGIN/../lib]
";
    assert_eq!(parse_needed(out), ["libxdo.so.3", "libc.so.6"]);
}

/// Step 7: a DT_RUNPATH serves only the binary's own libraries, so libxdo
/// did not find the X extensions bundled beside it; the binary must carry a
/// DT_RPATH, which serves the libraries it loads as well.
#[test]
fn only_a_dt_rpath_counts_as_the_library_path() {
    let rpath = " 0x000000000000000f (RPATH)              Library rpath: [$ORIGIN/../lib]\n";
    let runpath = " 0x000000000000001d (RUNPATH)            Library runpath: [$ORIGIN/../lib]\n";
    assert_eq!(parse_rpath(rpath), Some(RPATH));
    assert_eq!(parse_rpath(runpath), None);
}

#[cfg(target_os = "linux")]
#[test]
fn readelf_reads_what_a_real_binary_links() {
    let libs = needed(Path::new(env!("CARGO_BIN_EXE_xtask"))).unwrap();
    assert!(libs.iter().any(|l| l.starts_with("libc.so")), "{libs:?}");
}

/// Step 7: an AppImage that carried WebKit opened a blank window on a host
/// whose WebKit differed from the build machine's, because the bundled library
/// starts the host's helper processes. WebKit, GTK and GLib come from the host.
#[test]
fn the_appimage_carries_libxdo_and_takes_webkit_from_the_host() {
    let linked = [
        "libxdo.so.3",
        "libwebkit2gtk-4.1.so.0",
        "libjavascriptcoregtk-4.1.so.0",
        "libgtk-3.so.0",
        "libglib-2.0.so.0",
        "libssl.so.3",
        "libc.so.6",
    ]
    .map(String::from);
    assert_eq!(classify(&linked).unwrap(), ["libxdo.so.3"]);
    // libxdo's own: the X extensions travel with it, X11 itself is the host's.
    let libxdo = [
        "libX11.so.6",
        "libXtst.so.6",
        "libXinerama.so.1",
        "libxkbcommon.so.0",
        "libc.so.6",
    ]
    .map(String::from);
    assert_eq!(
        classify(&libxdo).unwrap(),
        ["libXtst.so.6", "libXinerama.so.1"]
    );
    for lib in &linked[1..] {
        assert!(
            !BUNDLED_LIBRARIES.contains(&lib.as_str()) && HOST_LIBRARIES.contains(&lib.as_str()),
            "{lib} comes from the host"
        );
    }
}

#[test]
fn a_library_neither_list_names_stops_the_bundle() {
    let err = classify(&["libduckdb.so".to_string()])
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("libduckdb.so") && err.contains("HOST_LIBRARIES"),
        "{err}"
    );
}
