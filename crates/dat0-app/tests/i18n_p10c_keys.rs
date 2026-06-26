#[test]
fn p10c_keys_resolve_and_orphan_removed() {
    for k in [
        "crash.dialog.title",
        "crash.dialog.body",
        "report.dialog.title",
        "report.dialog.body",
        "report.dialog.send",
        "report.dialog.note_placeholder",
        "menu.help.report_bug",
        "settings.profile.name_placeholder",
        "settings.profile.email_placeholder",
    ] {
        // dat0_i18n::t returns the KEY itself on a miss; a resolved value differs.
        assert_ne!(dat0_i18n::t(k), k, "missing i18n key: {k}");
    }
    assert_eq!(
        dat0_i18n::t("settings.update.auto_check"),
        "settings.update.auto_check",
        "orphan key must be removed (t() echoes the key on a miss)"
    );
}
