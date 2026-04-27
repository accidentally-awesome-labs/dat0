use dat0_app::platform;

#[test]
fn config_dir_is_under_user_home() {
    let path = platform::config_dir().expect("config dir");
    assert!(path.starts_with(dirs::home_dir().expect("home")));
    assert!(path.ends_with("dat0"));
}

#[test]
fn data_dir_is_under_user_home() {
    let path = platform::data_dir().expect("data dir");
    assert!(path.starts_with(dirs::home_dir().expect("home")));
    assert!(path.ends_with("dat0"));
}

#[test]
fn cache_dir_is_under_user_home() {
    let path = platform::cache_dir().expect("cache dir");
    assert!(path.starts_with(dirs::home_dir().expect("home")));
    assert!(path.ends_with("dat0"));
}
