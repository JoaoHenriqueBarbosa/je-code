//! Testes próprios de `uri_ipc.rs` e `uri_transformer.rs`, que o VS Code não testa diretamente. Não é porte de
//! teste do VS Code. Todos os resultados esperados foram obtidos rodando o `uriIpc.ts`, o `uriTransformer.ts`
//! e o `marshalling.ts` reais no bun, com as mesmas entradas.

use je_base::json;
use je_base::js_string::JsString;
use je_base::marshalling::{Marshalled, from_json, stringify};
use je_base::path::Platform;
use je_base::uri::URI;
use je_base::uri_ipc::{DefaultURITransformer, IURITransformer, transform_and_revive_incoming_uris, transform_incoming_uris, transform_outgoing_uris};
use je_base::uri_transformer::create_uri_transformer;

fn obj(entries: Vec<(&str, Marshalled)>) -> Marshalled {
    Marshalled::Object(entries.into_iter().map(|(k, v)| (JsString::from(k), v)).collect())
}

fn text(s: &str) -> Marshalled {
    Marshalled::String(JsString::from(s))
}

fn file(path: &str) -> Marshalled {
    Marshalled::Uri(URI::file(path, Platform::Posix))
}

fn parse_uri(value: &str) -> Marshalled {
    Marshalled::Uri(URI::parse(value, false))
}

fn outgoing_input() -> Marshalled {
    obj(vec![
        ("a", file("/x/y")),
        ("b", Marshalled::Array(vec![parse_uri("vscode-local:/y?q#f"), Marshalled::Number(1.0), text("str"), Marshalled::Null, Marshalled::Number(0.0)])),
        ("c", obj(vec![("d", parse_uri("https://h/z"))])),
        ("e", parse_uri("vscode-remote://h/p")),
    ])
}

fn incoming_input() -> Marshalled {
    // O JSON de `stringify({ a: URI.parse('vscode-remote://host/a/b?q#f'), ... })`, ainda sem reviver.
    let source = r#"{"a":{"$mid":1,"path":"/a/b","scheme":"vscode-remote","authority":"host","query":"q","fragment":"f"},"b":[{"$mid":1,"path":"/c","scheme":"file"},{"z":{"$mid":1,"path":"/z","scheme":"https","authority":"h"}}],"n":5,"s":"x"}"#;
    from_json(&json::parse_str(source, &mut Vec::new()).unwrap())
}

fn render(value: &Marshalled) -> String {
    stringify(value).to_string_lossy()
}

#[test]
fn transform_outgoing_uris_maps_schemes_and_authority() {
    let t = create_uri_transformer("my-host");
    assert_eq!(
        render(&transform_outgoing_uris(outgoing_input(), &t)),
        r#"{"a":{"$mid":1,"path":"/x/y","scheme":"vscode-remote","authority":"my-host"},"b":[{"$mid":1,"path":"/y","scheme":"file","query":"q","fragment":"f"},1,"str",null,0],"c":{"d":{"$mid":1,"path":"/z","scheme":"https","authority":"h"}},"e":{"$mid":1,"path":"/p","scheme":"vscode-remote","authority":"h"}}"#
    );
}

#[test]
fn default_transformer_changes_nothing() {
    assert_eq!(
        render(&transform_outgoing_uris(outgoing_input(), &DefaultURITransformer)),
        r#"{"a":{"$mid":1,"path":"/x/y","scheme":"file"},"b":[{"$mid":1,"path":"/y","scheme":"vscode-local","query":"q","fragment":"f"},1,"str",null,0],"c":{"d":{"$mid":1,"path":"/z","scheme":"https","authority":"h"}},"e":{"$mid":1,"path":"/p","scheme":"vscode-remote","authority":"h"}}"#
    );
    let before = render(&incoming_input());
    assert_eq!(render(&transform_incoming_uris(incoming_input(), &DefaultURITransformer)), before);
}

#[test]
fn transform_incoming_uris_keeps_plain_objects_and_drops_the_authority() {
    let t = create_uri_transformer("my-host");
    let expected = r#"{"a":{"$mid":1,"path":"/a/b","scheme":"file","query":"q","fragment":"f"},"b":[{"$mid":1,"path":"/c","scheme":"vscode-local"},{"z":{"$mid":1,"path":"/z","scheme":"https","authority":"h"}}],"n":5,"s":"x"}"#;
    // Sem reviver o resultado continua sendo objetos `$mid`: a serialização por `stringify` dos objetos
    // plenos é a do JSON, e a de um `URI` revivido é a mesma (sem cache).
    assert_eq!(render(&transform_incoming_uris(incoming_input(), &t)), expected);
}

#[test]
fn transform_and_revive_incoming_uris_returns_uris() {
    let t = create_uri_transformer("my-host");
    let revived = transform_and_revive_incoming_uris(incoming_input(), &t, Platform::Posix);
    let Marshalled::Object(entries) = &revived else { panic!("devia ser um objeto") };
    let Marshalled::Uri(a) = &entries[0].1 else { panic!("`a` devia ser um URI") };
    assert_eq!(a.scheme().to_string_lossy(), "file");
    let Marshalled::Array(b) = &entries[1].1 else { panic!("`b` devia ser um array") };
    let Marshalled::Uri(b0) = &b[0] else { panic!("`b[0]` devia ser um URI") };
    assert_eq!(b0.scheme().to_string_lossy(), "vscode-local");
    let Marshalled::Object(inner) = &b[1] else { panic!("`b[1]` devia ser um objeto") };
    let Marshalled::Uri(z) = &inner[0].1 else { panic!("`z` devia ser um URI") };
    assert_eq!(z.scheme().to_string_lossy(), "https");
    assert_eq!(
        render(&revived),
        r#"{"a":{"$mid":1,"path":"/a/b","scheme":"file","query":"q","fragment":"f"},"b":[{"$mid":1,"path":"/c","scheme":"vscode-local"},{"z":{"$mid":1,"path":"/z","scheme":"https","authority":"h"}}],"n":5,"s":"x"}"#
    );
}

#[test]
fn transform_outgoing_scheme_and_uri() {
    let t = create_uri_transformer("my-host");
    assert_eq!(t.transform_outgoing_scheme("file"), "vscode-remote");
    assert_eq!(t.transform_outgoing_scheme("vscode-local"), "file");
    assert_eq!(t.transform_outgoing_scheme("x"), "x");
    assert_eq!(t.transform_outgoing_uri(&URI::file("/a", Platform::Posix)).to_string(false).to_string_lossy(), "vscode-remote://my-host/a");
    let https = URI::parse("https://h/", false);
    let result = t.transform_outgoing_uri(&https);
    assert!(URI::ptr_eq(&result, &https), "um esquema que não muda devolve o mesmo URI");
    assert_eq!(result.to_string(false).to_string_lossy(), "https://h/");
}

#[test]
fn cached_state_is_dropped_when_the_uri_is_transformed() {
    let t = create_uri_transformer("my-host");
    let cached = URI::file("/cached", Platform::Posix);
    let _ = cached.fs_path(Platform::Posix);
    let input = obj(vec![("u", Marshalled::Uri(cached.clone()))]);
    assert_eq!(render(&transform_outgoing_uris(input, &t)), r#"{"u":{"$mid":1,"path":"/cached","scheme":"vscode-remote","authority":"my-host"}}"#);
    // Na volta, o objeto `$mid` com `fsPath` e `_sep` (o JSON do URI com cache) perde o cache ao ser trocado.
    let wire = from_json(&json::parse_str(&render(&obj(vec![("u", Marshalled::Uri(cached))])), &mut Vec::new()).unwrap());
    assert_eq!(render(&transform_incoming_uris(wire, &t)), r#"{"u":{"$mid":1,"path":"/cached","scheme":"vscode-local"}}"#);
}
