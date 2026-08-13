#[test]
fn init_logging_returns_ok() {
    let result = dat0_core::boot::init_logging();
    assert!(result.is_ok());
}
