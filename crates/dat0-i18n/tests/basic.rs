use dat0_i18n::t;

#[test]
fn t_returns_known_key() {
    assert_eq!(t("app.name"), "dat0");
}

#[test]
fn t_returns_key_when_missing() {
    let s = t("does.not.exist");
    assert_eq!(
        s, "does.not.exist",
        "missing keys must surface the key itself"
    );
}
