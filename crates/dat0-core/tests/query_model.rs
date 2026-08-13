use dat0_core::query::{ResultTarget, SqlTabMeta};

#[test]
fn sql_tab_meta_new_has_title_and_id() {
    let t = SqlTabMeta::new("Query 1");
    assert_eq!(t.title, "Query 1");
    assert_eq!(t.result_target, ResultTarget::MainGrid);
    assert!(t.last_result_view.is_none());
}

#[test]
fn result_view_name_is_namespaced() {
    let name = dat0_core::query::result_view_name("ab12", 3);
    assert_eq!(name, "__dat0_qr_ab12_3");
}

#[test]
fn result_target_default_is_main_grid() {
    assert_eq!(ResultTarget::default(), ResultTarget::MainGrid);
}
