//! Testes próprios de `src/vs/base/common/dataTransfer.ts` (o VS Code não tem `dataTransfer.test.ts`).

use je_base::data_transfer::*;
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;

fn text_item(text: &str) -> DataTransferItem {
    create_string_data_transfer_item(text, None)
}

fn file_item(name: &str) -> DataTransferItem {
    create_file_data_transfer_item(name, None, || Promise::resolved(vec![1, 2, 3]), None)
}

#[test]
fn string_item() {
    let item = create_string_data_transfer_item("olá", Some("id1".to_string()));
    assert_eq!(item.id.as_deref(), Some("id1"));
    assert!(item.as_file().is_none());
    assert_eq!(item.value.type_of(), "string");
    let result = item.as_string();
    runtime::run_until_idle();
    assert_eq!(result.result().unwrap().unwrap(), "olá");
}

#[test]
fn string_item_from_a_promise_has_undefined_value() {
    let item = create_string_data_transfer_item(Promise::resolved("later".to_string()), None);
    assert!(item.value.is_undefined());
    let result = item.as_string();
    runtime::run_until_idle();
    assert_eq!(result.result().unwrap().unwrap(), "later");
}

#[test]
fn file_item_exposes_the_file_and_an_empty_string() {
    let uri = URI::file("/tmp/x.txt", Platform::Posix);
    let item = create_file_data_transfer_item("x.txt", Some(uri.clone()), || Promise::resolved(vec![9]), None);
    let file = item.as_file().expect("arquivo");
    assert_eq!(file.name, "x.txt");
    assert_eq!(file.uri.as_ref(), Some(&uri));
    assert!(!file.id.is_empty());
    let data = file.data();
    let text = item.as_string();
    runtime::run_until_idle();
    assert_eq!(data.result().unwrap().unwrap(), vec![9]);
    assert_eq!(text.result().unwrap().unwrap(), "");
    // `asFile` devolve sempre o mesmo arquivo.
    assert_eq!(item.as_file().unwrap().id, file.id);
}

#[test]
fn append_replace_delete_and_size() {
    let mut transfer = VSDataTransfer::new();
    assert_eq!(transfer.size(), 0);
    transfer.append("text/plain", text_item("a"));
    transfer.append("text/plain", text_item("b"));
    transfer.append("text/html", text_item("c"));
    assert_eq!(transfer.size(), 2);
    assert_eq!(transfer.iter().count(), 3);
    assert!(transfer.has("TEXT/PLAIN"));
    assert!(!transfer.has("text/*"));
    assert_eq!(transfer.get("Text/Plain").unwrap().value.get_str("length").type_of(), "number");

    transfer.replace("text/plain", text_item("z"));
    assert_eq!(transfer.iter().filter(|(mime, _)| *mime == "text/plain").count(), 1);
    transfer.delete("TEXT/HTML");
    assert_eq!(transfer.size(), 1);
    assert!(transfer.get("text/html").is_none());
    assert_eq!(transfer.iter().next().unwrap().0, "text/plain");
}

#[test]
fn append_looks_up_the_type_without_normalizing_the_case() {
    // Como no original: `append('Text/Plain')` não acha `text/plain` e acaba substituindo as entradas.
    let mut transfer = VSDataTransfer::new();
    transfer.append("text/plain", text_item("a"));
    transfer.append("text/plain", text_item("b"));
    transfer.append("Text/Plain", text_item("c"));
    assert_eq!(transfer.iter().count(), 1);
}

#[test]
fn matches_wildcards_and_files() {
    let mut transfer = VSDataTransfer::new();
    assert!(!transfer.matches("*/*"));
    transfer.append("text/plain", text_item("a"));
    assert!(transfer.matches("*/*"));
    assert!(transfer.matches("text/plain"));
    assert!(transfer.matches("TEXT/*"));
    assert!(!transfer.matches("image/*"));
    assert!(!transfer.matches("files"));
    transfer.append("image/png", file_item("a.png"));
    assert!(transfer.matches("files"));
    assert!(transfer.matches("image/*"));
    assert!(!transfer.matches("image/jpeg"));
    assert!(!transfer.matches("not a mime"));
}

#[test]
fn matches_mime_type_function() {
    assert!(matches_mime_type("image/*", &["IMAGE/PNG"]));
    assert!(!matches_mime_type("image/*", &["text/plain"]));
    assert!(matches_mime_type("*/*", &["a/b"]));
    assert!(!matches_mime_type("*/*", &[]));
    assert!(matches_mime_type("text/plain", &["Text/Plain"]));
    // `application/x-foo` não casa com a regex do curinga (o hífen não é letra) e não é igual.
    assert!(!matches_mime_type("application/x-*", &["application/x-foo"]));
}

#[test]
fn uri_list() {
    let uri = URI::file("/a b", Platform::Posix);
    let created = uri_list::create(&[
        UriListEntry::String("http://x".to_string()),
        UriListEntry::Uri(uri.clone()),
        UriListEntry::String("http://x".to_string()),
    ]);
    assert_eq!(created, format!("http://x\r\n{}", uri.to_string(false)));
    assert_eq!(uri_list::split("a\r\n#c\r\nb"), vec!["a", "#c", "b"]);
    assert_eq!(uri_list::parse("a\r\n#c\r\nb"), vec!["a", "b"]);
    assert_eq!(uri_list::split(""), vec![""]);
}
