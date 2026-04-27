#[test]
fn init_logging_returns_ok() {
    let result = dat0_app::boot::init_logging();
    assert!(result.is_ok());
}
