//! Casos próprios do porte de `marshalling.ts`: o original só testa `RegExp` e `URI` (ver `marshalling.rs`);
//! aqui entram o `Date` revivido, o `$mid` desconhecido, a profundidade e o JSON inválido.

use je_base::buffer::VSBuffer;
use je_base::errors;
use je_base::js_string::JsString;
use je_base::marshalling::{Marshalled, parse, revive, stringify};
use je_base::marshalling_ids::MarshalledId;
use je_base::path::Platform;
use je_base::uri::{UriComponents, URI};

fn parse_str(text: &str) -> Marshalled {
    parse(&text.encode_utf16().collect::<Vec<_>>(), Platform::Posix)
}

fn stringify_str(value: &Marshalled) -> String {
    stringify(value).to_string_lossy()
}

#[test]
fn date_is_revived_from_a_number_or_an_iso_string() {
    let Marshalled::Date(zero) = parse_str("{\"$mid\":17,\"source\":0}") else { panic!("esperava Date") };
    assert_eq!(zero, 0.0);
    let Marshalled::Date(iso) = parse_str("{\"$mid\":17,\"source\":\"2020-01-02T03:04:05.006Z\"}") else { panic!("esperava Date") };
    assert_eq!(iso, 1_577_934_245_006.0);
    let Marshalled::Date(invalid) = parse_str("{\"$mid\":17,\"source\":\"quando\"}") else { panic!("esperava Date") };
    assert!(invalid.is_nan());
    assert_eq!(stringify_str(&Marshalled::Date(0.0)), "\"1970-01-01T00:00:00.000Z\"");
    assert_eq!(stringify_str(&Marshalled::Date(f64::NAN)), "null");
}

#[test]
fn unknown_mid_is_walked_like_a_plain_object() {
    let value = parse_str("{\"$mid\":3,\"a\":[1,{\"$mid\":1,\"scheme\":\"file\",\"path\":\"/x\"}],\"b\":null}");
    let Marshalled::Object(entries) = &value else { panic!("esperava objeto") };
    assert_eq!(entries.iter().map(|(k, _)| k.to_string_lossy()).collect::<Vec<_>>(), vec!["$mid", "a", "b"]);
    let Marshalled::Array(items) = &entries[1].1 else { panic!("esperava lista") };
    let Marshalled::Uri(uri) = &items[1] else { panic!("o URI aninhado devia ser revivido") };
    assert_eq!(uri.to_string(false), "file:///x");
    assert_eq!(MarshalledId::from_i32(3), Some(MarshalledId::ScmResource));
    assert_eq!(MarshalledId::from_i32(0), None);
    assert_eq!(MarshalledId::from_i32(27), None);
}

#[test]
fn uri_and_buffer_stringify_like_json_stringify() {
    let uri = URI::from(&UriComponents { scheme: Some(JsString::from("file")), path: Some(JsString::from("/a b")), ..UriComponents::default() }, false);
    assert_eq!(stringify_str(&Marshalled::Uri(uri)), "{\"$mid\":1,\"path\":\"/a b\",\"scheme\":\"file\"}");
    assert_eq!(stringify_str(&Marshalled::Buffer(VSBuffer::wrap(vec![1, 2]))), "{\"buffer\":{\"0\":1,\"1\":2},\"byteLength\":2}");
}

#[test]
fn falsy_and_deep_values_pass_through() {
    assert!(matches!(revive(Marshalled::Number(0.0), 0, Platform::Posix), Marshalled::Number(n) if n == 0.0));
    // Além da profundidade 200 o `$mid` não é mais lido.
    let mut deep = Marshalled::Object(vec![(JsString::from("$mid"), Marshalled::Number(1.0)), (JsString::from("scheme"), Marshalled::String(JsString::from("file")))]);
    assert!(matches!(revive(deep.clone(), 201, Platform::Posix), Marshalled::Object(_)));
    assert!(matches!(revive(deep.clone(), 200, Platform::Posix), Marshalled::Uri(_)));
    deep = Marshalled::Array(vec![deep]);
    assert!(matches!(revive(deep, 0, Platform::Posix), Marshalled::Array(items) if matches!(items[0], Marshalled::Uri(_))));
}

#[test]
fn invalid_json_and_invalid_regexp_throw() {
    let text = |t: &str| t.encode_utf16().collect::<Vec<_>>();
    assert!(errors::catch(|| parse(&text("{"), Platform::Posix)).is_err());
    assert!(errors::catch(|| parse(&text("{\"a\":1,}"), Platform::Posix)).is_err());
    assert!(errors::catch(|| parse(&text("// c\n1"), Platform::Posix)).is_err());
    assert!(errors::catch(|| parse(&text(""), Platform::Posix)).is_err());
    assert!(errors::catch(|| parse(&text("{\"$mid\":2,\"source\":\"(\",\"flags\":\"\"}"), Platform::Posix)).is_err());
}
