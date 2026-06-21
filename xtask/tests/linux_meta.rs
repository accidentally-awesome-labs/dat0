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
