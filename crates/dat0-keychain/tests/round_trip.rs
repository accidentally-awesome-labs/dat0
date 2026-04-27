use dat0_keychain::Keychain;

#[test]
fn store_and_retrieve() {
    let kc = Keychain::new("dat0-test").unwrap();
    let key = "test-secret";
    let value = b"hunter2";

    kc.set(key, value).unwrap();
    let retrieved = kc.get(key).unwrap();
    assert_eq!(retrieved.as_deref(), Some(value.as_slice()));

    kc.delete(key).unwrap();
    assert!(kc.get(key).unwrap().is_none());
}
