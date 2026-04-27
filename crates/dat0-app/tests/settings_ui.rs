use dat0_app::settings_ui::sections;

#[test]
fn registry_has_profile_and_theme() {
    let all = sections::all_sections();
    let ids: Vec<_> = all.iter().map(|s| s.id()).collect();
    assert!(ids.contains(&"profile"), "profile section missing");
    assert!(ids.contains(&"theme"), "theme section missing");
}

#[test]
fn each_section_has_resolvable_name_key() {
    for section in sections::all_sections() {
        let resolved = dat0_i18n::t(section.name_key());
        assert_ne!(
            resolved,
            section.name_key(),
            "section name key {} did not resolve via i18n",
            section.name_key()
        );
    }
}
