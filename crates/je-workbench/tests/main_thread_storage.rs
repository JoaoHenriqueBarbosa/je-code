use je_workbench::api::browser::main_thread_secret_state::secret_state_key;

#[test]
fn secret_key_is_the_json_of_extension_id_and_key() {
    assert_eq!(secret_state_key("pub.ext", "token"), r#"{"extensionId":"pub.ext","key":"token"}"#);
}

#[test]
fn secret_key_escapes_quotes() {
    assert_eq!(secret_state_key("pub.ext", "a\"b"), r#"{"extensionId":"pub.ext","key":"a\"b"}"#);
}
