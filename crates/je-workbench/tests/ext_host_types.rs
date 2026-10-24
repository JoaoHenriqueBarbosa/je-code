//! Porte de `src/vs/workbench/api/test/browser/extHostTypes.test.ts` (suíte `ExtHostTypes`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` não tem o que vigiar aqui: nenhum caso cria um
//!   `DisposableStore` ou registra um disposable rastreado além do que o próprio caso descarta.
//! - `assertToJSON(a, expected)` faz `JSON.parse(JSON.stringify(a))` e `deepStrictEqual`: o `toJSON` do porte
//!   já devolve o valor JSON, e a comparação ignora a ordem das chaves (`normalize`), como o
//!   `deepStrictEqual`.
//! - `assert.throws(() => ...)` vira `is_err()` do `Result` do construtor/método.
//! - Os casos de `null!`/`undefined!` passados onde o tipo não aceita (`new Range(pos, undefined!)`,
//!   `p1.translate(null!)`, `range.intersection(null!)`) não compilam em Rust, então a conferência
//!   correspondente é garantida pelo tipo; as demais conferências do caso ficam iguais. As escritas em campos
//!   somente leitura (`pos.line = -1`, `range.start = null`) também são erro de compilação.
//! - `translate(x)`/`with(x)` com o objeto `{ lineDelta }`/`{ line }` coincidem com as formas de dois
//!   argumentos (`Option`), então os dois estilos do original usam a mesma chamada.
//! - `=== p1` (a mesma instância) vira igualdade de valor: `Position`/`Range` são valores.
//! - `Range, contains (no instanceof)`: os objetos "com a forma de" `Position`/`Range` do original viram
//!   posições e intervalos reconstruídos a partir de `line`/`character`.
//! - `SemanticTokensBuilder#push(range, tokenType, modifiers)` é `push_range`; `Uint32Array` é `Vec<u32>`.
//! - `new CancellationError()` é `Error::cancellation()` do `je-base`.

use std::cell::Cell;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::json::{Object, Value};
use je_base::lifecycle::{IDisposable, to_disposable};
use je_base::path::Platform;
use je_base::uri::{MARSHALLED_ID_URI, URI, UriComponents, UriState};
use je_base::JsString;
use je_workbench::api::common::ext_host_types::{
    ChatMessageContent, CodeActionKind, CodeLens, Command, CompletionItem, CompletionItemKind, CompletionLabel, Diagnostic, Disposable, DocumentHighlight,
    DocumentHighlightKind, DocumentLink, ErrorLike, FileDecoration, FileDecorationBadge, FileSystemError, LanguageModelChatMessage,
    LanguageModelChatMessagePart, LanguageModelChatMessageRole, LanguageModelError, LanguageModelTextPart, Location, MarkdownString,
    MessageOrUri, NotebookCellOutputItem, Position, Range, Selection, SemanticTokensBuilder, SemanticTokensLegend, SnippetContent,
    SnippetString, SymbolInformation, SymbolKind, TextEdit, WorkspaceEdit, WorkspaceEditEntry, WorkspaceEditEntryMetadata,
    serialize_language_model_error, uri_to_json,
};

fn platform() -> Platform {
    if cfg!(windows) { Platform::Win32 } else { Platform::Posix }
}

fn p(line: f64, character: f64) -> Position {
    Position::new(line, character).unwrap()
}

fn r(start_line: f64, start_column: f64, end_line: f64, end_column: f64) -> Range {
    Range::from_coordinates(start_line, start_column, end_line, end_column).unwrap()
}

/// Reordena as chaves dos objetos (o `deepStrictEqual` ignora a ordem das propriedades).
fn normalize(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
        Value::Object(object) => {
            let mut entries: Vec<(JsString, Value)> = object.iter().map(|(key, item)| (key.clone(), normalize(item))).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().collect::<Object>())
        }
        other => other.clone(),
    }
}

fn assert_to_json(actual: Value, expected: Value) {
    assert_eq!(normalize(&actual), normalize(&expected));
}

fn obj(entries: Vec<(&str, Value)>) -> Value {
    Value::Object(entries.into_iter().collect::<Object>())
}

fn num(value: f64) -> Value {
    Value::Number(value)
}

fn text(value: &str) -> Value {
    Value::from(value)
}

fn jpos(line: f64, character: f64) -> Value {
    obj(vec![("line", num(line)), ("character", num(character))])
}

fn jrange(start_line: f64, start_character: f64, end_line: f64, end_character: f64) -> Value {
    Value::Array(vec![jpos(start_line, start_character), jpos(end_line, end_character)])
}

fn file(path: &str) -> URI {
    URI::file(path, platform())
}

fn parse(value: &str) -> URI {
    URI::parse(value, false)
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn uri_to_json_test() {
    let uri = parse("file:///path/test.file");
    let components = UriComponents { scheme: Some("file".into()), path: Some("/path/test.file".into()), ..UriComponents::default() };
    assert_eq!(uri.to_json(), UriState { mid: Some(MARSHALLED_ID_URI), components: components.clone(), ..UriState::default() });

    let fs_path = uri.fs_path(platform());
    assert!(!fs_path.is_empty());
    let expected_fs_path = JsString::from("/path/test.file".replace('/', if cfg!(windows) { "\\" } else { "/" }));
    let with_fs_path = UriState {
        mid: Some(MARSHALLED_ID_URI),
        fs_path: Some(expected_fs_path),
        sep: if cfg!(windows) { Some(1) } else { None },
        components: components.clone(),
        ..UriState::default()
    };
    assert_eq!(uri.to_json(), with_fs_path);

    assert!(!uri.to_string(false).is_empty());
    assert_eq!(uri.to_json(), UriState { external: Some("file:///path/test.file".into()), ..with_fs_path });
}

#[test]
fn disposable() {
    let count = Rc::new(Cell::new(0));
    let counter = Rc::clone(&count);
    let d = Disposable::new(move || counter.set(counter.get() + 1));
    d.dispose();
    assert_eq!(count.get(), 1);

    d.dispose();
    assert_eq!(count.get(), 1);

    let counter = Rc::clone(&count);
    Disposable::from(vec![None, Some(to_disposable(move || counter.set(counter.get() + 1)))]).dispose();
    assert_eq!(count.get(), 2);

    assert!(errors::catch(|| Disposable::new(|| errors::throw(Error::new("falha"))).dispose()).is_err());

    Disposable::default().dispose();
}

#[test]
fn position() {
    assert!(Position::new(-1.0, 0.0).is_err());
    assert!(Position::new(0.0, -1.0).is_err());

    let pos = p(0.0, 0.0);
    assert_to_json(pos.to_json(), jpos(0.0, 0.0));
    assert_eq!(pos.line(), 0.0);
    assert_eq!(pos.character(), 0.0);
}

#[test]
fn position_to_json() {
    assert_to_json(p(4.0, 2.0).to_json(), jpos(4.0, 2.0));
}

#[test]
fn position_is_before_or_equal() {
    let p1 = p(1.0, 3.0);
    let p2 = p(1.0, 2.0);
    let p3 = p(0.0, 4.0);

    assert!(p1.is_before_or_equal(&p1));
    assert!(!p1.is_before(&p1));
    assert!(p2.is_before(&p1));
    assert!(p3.is_before(&p2));
}

#[test]
fn position_is_after_or_equal() {
    let p1 = p(1.0, 3.0);
    let p2 = p(1.0, 2.0);
    let p3 = p(0.0, 4.0);

    assert!(p1.is_after_or_equal(&p1));
    assert!(!p1.is_after(&p1));
    assert!(p1.is_after(&p2));
    assert!(p2.is_after(&p3));
    assert!(p1.is_after(&p3));
}

#[test]
fn position_compare_to() {
    let p1 = p(1.0, 3.0);
    let p2 = p(1.0, 2.0);
    let p3 = p(0.0, 4.0);

    assert_eq!(p1.compare_to(&p1), 0);
    assert_eq!(p2.compare_to(&p1), -1);
    assert_eq!(p1.compare_to(&p2), 1);
    assert_eq!(p2.compare_to(&p3), 1);
    assert_eq!(p1.compare_to(&p3), 1);
}

#[test]
fn position_translate() {
    let p1 = p(1.0, 3.0);

    assert_eq!(p1.translate(None, None).unwrap(), p1);
    assert_eq!(p1.translate(Some(0.0), Some(0.0)).unwrap(), p1);
    assert_eq!(p1.translate(Some(0.0), None).unwrap(), p1);
    assert_eq!(p1.translate(None, Some(0.0)).unwrap(), p1);

    let mut res = p1.translate(Some(-1.0), None).unwrap();
    assert_eq!(res.line(), 0.0);
    assert_eq!(res.character(), 3.0);

    res = p1.translate(None, Some(-1.0)).unwrap();
    assert_eq!(res.line(), 1.0);
    assert_eq!(res.character(), 2.0);

    res = p1.translate(Some(11.0), None).unwrap();
    assert_eq!(res.line(), 12.0);
    assert_eq!(res.character(), 3.0);

    assert!(p1.translate(Some(-2.0), None).is_err());
    assert!(p1.translate(Some(-2.0), Some(0.0)).is_err());
    assert!(p1.translate(Some(0.0), Some(-4.0)).is_err());
}

#[test]
fn position_with() {
    let p1 = p(1.0, 3.0);

    assert_eq!(p1.with(None, None).unwrap(), p1);
    assert_eq!(p1.with(Some(1.0), None).unwrap(), p1);
    assert_eq!(p1.with(None, Some(3.0)).unwrap(), p1);
    assert_eq!(p1.with(Some(1.0), Some(3.0)).unwrap(), p1);

    let p2 = p1.with(Some(0.0), Some(11.0)).unwrap();
    assert_eq!(p2.line(), 0.0);
    assert_eq!(p2.character(), 11.0);

    assert!(p1.with(Some(-9.0), None).is_err());
    assert!(p1.with(Some(0.0), Some(-9.0)).is_err());
    assert!(p1.with(Some(-1.0), None).is_err());
    assert!(p1.with(None, Some(-1.0)).is_err());
}

#[test]
fn range() {
    assert!(Range::from_coordinates(-1.0, 0.0, 0.0, 0.0).is_err());
    assert!(Range::from_coordinates(0.0, -1.0, 0.0, 0.0).is_err());
}

#[test]
fn range_to_json() {
    assert_to_json(r(1.0, 2.0, 3.0, 4.0).to_json(), jrange(1.0, 2.0, 3.0, 4.0));
}

#[test]
fn range_sorting() {
    // ordena início e fim
    let mut range = r(1.0, 0.0, 0.0, 0.0);
    assert_eq!(range.start().line(), 0.0);
    assert_eq!(range.end().line(), 1.0);

    range = r(0.0, 0.0, 1.0, 0.0);
    assert_eq!(range.start().line(), 0.0);
    assert_eq!(range.end().line(), 1.0);
}

#[test]
fn range_is_empty_is_single_line() {
    let mut range = r(1.0, 0.0, 0.0, 0.0);
    assert!(!range.is_empty());
    assert!(!range.is_single_line());

    range = r(1.0, 1.0, 1.0, 1.0);
    assert!(range.is_empty());
    assert!(range.is_single_line());

    range = r(0.0, 1.0, 0.0, 11.0);
    assert!(!range.is_empty());
    assert!(range.is_single_line());

    range = r(0.0, 0.0, 1.0, 1.0);
    assert!(!range.is_empty());
    assert!(!range.is_single_line());
}

#[test]
fn range_contains() {
    let range = r(1.0, 1.0, 2.0, 11.0);

    assert!(range.contains_position(&range.start()));
    assert!(range.contains_position(&range.end()));
    assert!(range.contains_range(&range));

    assert!(!range.contains_range(&r(1.0, 0.0, 2.0, 11.0)));
    assert!(!range.contains_range(&r(0.0, 1.0, 2.0, 11.0)));
    assert!(!range.contains_range(&r(1.0, 1.0, 2.0, 12.0)));
    assert!(!range.contains_range(&r(1.0, 1.0, 3.0, 11.0)));
}

#[test]
fn range_contains_no_instanceof() {
    let range = r(1.0, 1.0, 2.0, 11.0);

    let start_like = p(range.start().line(), range.start().character());
    let end_like = p(range.end().line(), range.end().character());
    let range_like = Range::new(start_like, end_like);

    assert!(range.contains_position(&start_like));
    assert!(range.contains_position(&end_like));
    assert!(range.contains_range(&range_like));
}

#[test]
fn range_intersection() {
    let range = r(1.0, 1.0, 2.0, 11.0);

    let mut res = range.intersection(&range).unwrap();
    assert_eq!(res.start().line(), 1.0);
    assert_eq!(res.start().character(), 1.0);
    assert_eq!(res.end().line(), 2.0);
    assert_eq!(res.end().character(), 11.0);

    assert!(range.intersection(&r(2.0, 12.0, 4.0, 0.0)).is_none());

    assert!(range.intersection(&r(0.0, 0.0, 1.0, 0.0)).is_none());

    res = range.intersection(&r(0.0, 0.0, 1.0, 1.0)).unwrap();
    assert!(res.is_empty());
    assert_eq!(res.start().line(), 1.0);
    assert_eq!(res.start().character(), 1.0);

    res = range.intersection(&r(2.0, 11.0, 61.0, 1.0)).unwrap();
    assert!(res.is_empty());
    assert_eq!(res.start().line(), 2.0);
    assert_eq!(res.start().character(), 11.0);
}

#[test]
fn range_union() {
    let mut ran1 = r(0.0, 0.0, 5.0, 5.0);
    assert_eq!(ran1.union(&r(0.0, 0.0, 1.0, 1.0)), ran1);

    let mut res = ran1.union(&r(2.0, 2.0, 9.0, 9.0));
    assert_eq!(res.start(), ran1.start());
    assert_eq!(res.end().line(), 9.0);
    assert_eq!(res.end().character(), 9.0);

    ran1 = r(2.0, 1.0, 5.0, 3.0);
    res = ran1.union(&r(1.0, 0.0, 4.0, 2.0));
    assert_eq!(res.end(), ran1.end());
    assert_eq!(res.start().line(), 1.0);
    assert_eq!(res.start().character(), 0.0);
}

#[test]
fn range_with() {
    let range = r(1.0, 1.0, 2.0, 11.0);

    assert_eq!(range.with(Some(range.start()), None), range);
    assert_eq!(range.with(None, Some(range.end())), range);
    assert_eq!(range.with(Some(range.start()), Some(range.end())), range);
    assert_eq!(range.with(Some(p(1.0, 1.0)), None), range);
    assert_eq!(range.with(None, Some(p(2.0, 11.0))), range);
    assert_eq!(range.with(None, None), range);

    let mut res = range.with(None, Some(p(9.0, 8.0)));
    assert_eq!(res.end().line(), 9.0);
    assert_eq!(res.end().character(), 8.0);
    assert_eq!(res.start().line(), 1.0);
    assert_eq!(res.start().character(), 1.0);

    res = range.with(Some(p(2.0, 3.0)), Some(p(9.0, 8.0)));
    assert_eq!(res.end().line(), 9.0);
    assert_eq!(res.end().character(), 8.0);
    assert_eq!(res.start().line(), 2.0);
    assert_eq!(res.start().character(), 3.0);
}

#[test]
fn text_edit() {
    let range = r(1.0, 1.0, 2.0, 11.0);
    let mut edit = TextEdit::new(range, None);
    assert_eq!(edit.new_text(), "");
    assert_to_json(edit.to_json(), obj(vec![("range", jrange(1.0, 1.0, 2.0, 11.0)), ("newText", text(""))]));

    edit = TextEdit::new(range, None);
    assert_eq!(edit.new_text(), "");

    edit = TextEdit::new(range, Some(""));
    assert_eq!(edit.new_text(), "");
}

#[test]
fn workspace_edit() {
    let a = file("a.ts");
    let b = file("b.ts");

    let mut edit = WorkspaceEdit::new();
    assert!(!edit.has(&a));

    edit.set(&a, vec![TextEdit::insert(p(0.0, 0.0), "fff")]);
    assert!(edit.has(&a));
    assert_eq!(edit.size(), 1);
    let inserted = |line: f64, character: f64, new_text: &str| obj(vec![("range", jrange(line, character, line, character)), ("newText", text(new_text))]);
    assert_to_json(edit.to_json(), Value::Array(vec![Value::Array(vec![uri_to_json(&a), Value::Array(vec![inserted(0.0, 0.0, "fff")])])]));

    edit.insert(&b, p(1.0, 1.0), "fff", None);
    edit.delete(&b, r(0.0, 0.0, 0.0, 0.0), None);
    assert!(edit.has(&b));
    assert_eq!(edit.size(), 2);
    assert_to_json(
        edit.to_json(),
        Value::Array(vec![
            Value::Array(vec![uri_to_json(&a), Value::Array(vec![inserted(0.0, 0.0, "fff")])]),
            Value::Array(vec![uri_to_json(&b), Value::Array(vec![inserted(1.0, 1.0, "fff"), inserted(0.0, 0.0, "")])]),
        ]),
    );

    edit.unset(&b);
    assert!(!edit.has(&b));
    assert_eq!(edit.size(), 1);

    edit.set(&b, vec![TextEdit::insert(p(0.0, 0.0), "ffff")]);
    assert_eq!(edit.get(&b).len(), 1);
}

#[test]
fn workspace_edit_keep_order_of_text_and_file_changes() {
    let mut edit = WorkspaceEdit::new();
    edit.replace(&parse("foo:a"), r(1.0, 1.0, 1.0, 1.0), "foo", None);
    edit.rename_file(parse("foo:a"), parse("foo:b"), None, None);
    edit.replace(&parse("foo:a"), r(2.0, 1.0, 2.0, 1.0), "bar", None);
    edit.replace(&parse("foo:b"), r(3.0, 1.0, 3.0, 1.0), "bazz", None);

    let all = edit.all_entries();
    assert_eq!(all.len(), 4);

    let WorkspaceEditEntry::Text(first) = &all[0] else { panic!("o primeiro deveria ser Text") };
    assert_eq!(first.uri.to_string(false).to_string_lossy(), "foo:a");

    let WorkspaceEditEntry::File(second) = &all[1] else { panic!("o segundo deveria ser File") };
    assert_eq!(second.from.as_ref().unwrap().to_string(false).to_string_lossy(), "foo:a");
    assert_eq!(second.to.as_ref().unwrap().to_string(false).to_string_lossy(), "foo:b");

    let WorkspaceEditEntry::Text(third) = &all[2] else { panic!("o terceiro deveria ser Text") };
    assert_eq!(third.uri.to_string(false).to_string_lossy(), "foo:a");

    let WorkspaceEditEntry::Text(fourth) = &all[3] else { panic!("o quarto deveria ser Text") };
    assert_eq!(fourth.uri.to_string(false).to_string_lossy(), "foo:b");
}

#[test]
fn workspace_edit_two_edits_for_one_resource() {
    let mut edit = WorkspaceEdit::new();
    let uri = parse("foo:bar");
    edit.insert(&uri, p(0.0, 0.0), "Hello", None);
    edit.insert(&uri, p(0.0, 0.0), "Foo", None);

    assert_eq!(edit.all_entries().len(), 2);
    let all = edit.all_entries();

    let WorkspaceEditEntry::Text(first) = &all[0] else { panic!("o primeiro deveria ser Text") };
    let WorkspaceEditEntry::Text(second) = &all[1] else { panic!("o segundo deveria ser Text") };
    assert_eq!(first.edit.new_text(), "Hello");
    assert_eq!(second.edit.new_text(), "Foo");
}

#[test]
fn workspace_edit_set_with_metadata_accepts_undefined() {
    let mut edit = WorkspaceEdit::new();
    let uri = parse("foo:bar");

    let metadata = WorkspaceEditEntryMetadata { needs_confirmation: true, label: "foo".to_owned(), description: None, icon_path: None };
    edit.set(&uri, vec![(TextEdit::insert(p(0.0, 0.0), "Hello"), Some(metadata)), (TextEdit::insert(p(0.0, 0.0), "Hello"), None)]);

    let all = edit.all_entries();
    assert_eq!(all.len(), 2);
    assert!(all[0].metadata().is_some());
    assert!(all[1].metadata().is_none());
}

#[test]
fn document_link() {
    assert!(DocumentLink::new(r(1.0, 1.0, 1.0, 1.0), None).is_err());
}

#[test]
fn to_json_and_stringify() {
    assert_to_json(
        Selection::from_coordinates(3.0, 4.0, 2.0, 1.0).unwrap().to_json(),
        obj(vec![("start", jpos(2.0, 1.0)), ("end", jpos(3.0, 4.0)), ("anchor", jpos(3.0, 4.0)), ("active", jpos(2.0, 1.0))]),
    );

    assert_to_json(
        Location::new(Some(file("u.ts")), Some(p(3.0, 4.0).into())).to_json(),
        obj(vec![("uri", uri_to_json(&parse("file:///u.ts"))), ("range", jrange(3.0, 4.0, 3.0, 4.0))]),
    );
    assert_to_json(
        Location::new(Some(file("u.ts")), Some(r(1.0, 2.0, 3.0, 4.0).into())).to_json(),
        obj(vec![("uri", uri_to_json(&parse("file:///u.ts"))), ("range", jrange(1.0, 2.0, 3.0, 4.0))]),
    );

    let mut diag = Diagnostic::new(r(0.0, 1.0, 2.0, 3.0), "hello", None).unwrap();
    assert_to_json(diag.to_json(), obj(vec![("severity", text("Error")), ("message", text("hello")), ("range", jrange(0.0, 1.0, 2.0, 3.0))]));
    diag.source = Some("me".to_owned());
    assert_to_json(
        diag.to_json(),
        obj(vec![("severity", text("Error")), ("message", text("hello")), ("range", jrange(0.0, 1.0, 2.0, 3.0)), ("source", text("me"))]),
    );

    assert_to_json(
        DocumentHighlight::new(r(2.0, 3.0, 4.0, 5.0), None).to_json(),
        obj(vec![("range", jrange(2.0, 3.0, 4.0, 5.0)), ("kind", text("Text"))]),
    );
    assert_to_json(
        DocumentHighlight::new(r(2.0, 3.0, 4.0, 5.0), Some(DocumentHighlightKind::Read)).to_json(),
        obj(vec![("range", jrange(2.0, 3.0, 4.0, 5.0)), ("kind", text("Read"))]),
    );

    assert_to_json(
        SymbolInformation::from_range("test", SymbolKind::Boolean, r(0.0, 1.0, 2.0, 3.0), None, None).unwrap().to_json(),
        obj(vec![
            ("name", text("test")),
            ("kind", text("Boolean")),
            ("location", obj(vec![("range", jrange(0.0, 1.0, 2.0, 3.0))])),
        ]),
    );

    assert_to_json(CodeLens::new(r(7.0, 8.0, 9.0, 10.0), None).to_json(), obj(vec![("range", jrange(7.0, 8.0, 9.0, 10.0))]));
    assert_to_json(
        CodeLens::new(r(7.0, 8.0, 9.0, 10.0), Some(Command::new("title", "id"))).to_json(),
        obj(vec![
            ("range", jrange(7.0, 8.0, 9.0, 10.0)),
            ("command", obj(vec![("command", text("id")), ("title", text("title"))])),
        ]),
    );

    assert_to_json(CompletionItem::new(CompletionLabel::Text("complete".to_owned()), None).to_json(), obj(vec![("label", text("complete"))]));

    let mut item = CompletionItem::new(CompletionLabel::Text("complete".to_owned()), None);
    item.kind = Some(CompletionItemKind::Interface);
    assert_to_json(item.to_json(), obj(vec![("label", text("complete")), ("kind", text("Interface"))]));
}

#[test]
fn symbol_information_old_ctor() {
    let info = SymbolInformation::from_range("foo", SymbolKind::Array, r(1.0, 1.0, 2.0, 3.0), None, None).unwrap();
    assert!(info.location.uri.is_none());
    assert!(info.location.range.is_some());
}

#[test]
fn snippet_string_builder_methods() {
    let mut string = SnippetString::new(None);
    assert_eq!(string.append_text("I need $ and $").value, "I need \\$ and \\$");

    string = SnippetString::new(None);
    assert_eq!(string.append_text("I need \\$").value, "I need \\\\\\$");

    string = SnippetString::new(None);
    string.append_placeholder(SnippetContent::Text("fo$o}"), None);
    assert_eq!(string.value, "${1:fo\\$o\\}}");

    string = SnippetString::new(None);
    string.append_text("foo").append_tabstop(Some(0.0)).append_text("bar");
    assert_eq!(string.value, "foo$0bar");

    string = SnippetString::new(None);
    string.append_text("foo").append_tabstop(None).append_text("bar");
    assert_eq!(string.value, "foo$1bar");

    string = SnippetString::new(None);
    string.append_text("foo").append_tabstop(Some(42.0)).append_text("bar");
    assert_eq!(string.value, "foo$42bar");

    string = SnippetString::new(None);
    string.append_text("foo").append_placeholder(SnippetContent::Text("farboo"), None).append_text("bar");
    assert_eq!(string.value, "foo${1:farboo}bar");

    string = SnippetString::new(None);
    string.append_text("foo").append_placeholder(SnippetContent::Text("far$boo"), None).append_text("bar");
    assert_eq!(string.value, "foo${1:far\\$boo}bar");

    string = SnippetString::new(None);
    string
        .append_text("foo")
        .append_placeholder(
            SnippetContent::Build(&|b: &mut SnippetString| {
                b.append_text("abc").append_placeholder(SnippetContent::Text("nested"), None);
            }),
            None,
        )
        .append_text("bar");
    assert_eq!(string.value, "foo${1:abc${2:nested}}bar");

    string = SnippetString::new(None);
    string.append_variable("foo", None);
    assert_eq!(string.value, "${foo}");

    string = SnippetString::new(None);
    string.append_text("foo").append_variable("TM_SELECTED_TEXT", None).append_text("bar");
    assert_eq!(string.value, "foo${TM_SELECTED_TEXT}bar");

    string = SnippetString::new(None);
    string.append_variable(
        "BAR",
        Some(SnippetContent::Build(&|b: &mut SnippetString| {
            b.append_placeholder(SnippetContent::Text("ops"), None);
        })),
    );
    assert_eq!(string.value, "${BAR:${1:ops}}");

    string = SnippetString::new(None);
    string.append_variable("BAR", Some(SnippetContent::Build(&|_b: &mut SnippetString| {})));
    assert_eq!(string.value, "${BAR}");

    string = SnippetString::new(None);
    string.append_choice(&["b", "a", "r"], None);
    assert_eq!(string.value, "${1|b,a,r|}");

    string = SnippetString::new(None);
    string.append_choice(&["b,1", "a,2", "r,3"], None);
    assert_eq!(string.value, "${1|b\\,1,a\\,2,r\\,3|}");

    string = SnippetString::new(None);
    string.append_choice(&["b", "a", "r"], Some(0.0));
    assert_eq!(string.value, "${0|b,a,r|}");

    string = SnippetString::new(None);
    string.append_text("foo").append_choice(&["far", "boo"], None).append_text("bar");
    assert_eq!(string.value, "foo${1|far,boo|}bar");

    string = SnippetString::new(None);
    string.append_text("foo").append_choice(&["far", "$boo"], None).append_text("bar");
    assert_eq!(string.value, "foo${1|far,$boo|}bar");

    string = SnippetString::new(None);
    string.append_text("foo").append_placeholder(SnippetContent::Text("farboo"), None).append_choice(&["far", "boo"], None).append_text("bar");
    assert_eq!(string.value, "foo${1:farboo}${2|far,boo|}bar");
}

#[test]
fn snippet_choices_are_incorrectly_escaped_applied_180132() {
    {
        let mut s = SnippetString::new(None);
        s.append_choice(&["aaa$aaa"], None);
        s.append_text("bbb$bbb");
        assert_eq!(s.value, "${1|aaa$aaa|}bbb\\$bbb");
    }
    {
        let mut s = SnippetString::new(None);
        s.append_choice(&["aaa,aaa"], None);
        s.append_text("bbb$bbb");
        assert_eq!(s.value, "${1|aaa\\,aaa|}bbb\\$bbb");
    }
    {
        let mut s = SnippetString::new(None);
        s.append_choice(&["aaa|aaa"], None);
        s.append_text("bbb$bbb");
        assert_eq!(s.value, "${1|aaa\\|aaa|}bbb\\$bbb");
    }
    {
        let mut s = SnippetString::new(None);
        s.append_choice(&["aaa\\aaa"], None);
        s.append_text("bbb$bbb");
        assert_eq!(s.value, "${1|aaa\\\\aaa|}bbb\\$bbb");
    }
}

#[test]
fn instanceof_does_not_work_for_file_system_error_49386() {
    let error = FileSystemError::unavailable(Some(MessageOrUri::Message("foo".to_owned())));
    // `instanceof Error`: o `Error` que a classe estende; `instanceof FileSystemError`: o próprio tipo
    let base: &Error = error.error();
    assert_eq!(base.message, "foo");
    assert_eq!(base.name, "Unavailable (FileSystemError)");
    assert_eq!(error.code, "Unavailable");
}

#[test]
fn cancellation_error() {
    // O `CancellationError` é usado internamente e exportado como API: o nome e a mensagem são `Canceled`
    let err = Error::cancellation();
    assert_eq!(err.name, "Canceled");
    assert_eq!(err.message, "Canceled");
}

#[test]
fn code_action_kind_contains() {
    assert!(CodeActionKind::refactor_extract().contains(&CodeActionKind::refactor_extract()));
    assert!(CodeActionKind::refactor_extract().contains(&CodeActionKind::refactor_extract().append("other")));

    assert!(!CodeActionKind::refactor_extract().contains(&CodeActionKind::refactor()));
    assert!(!CodeActionKind::refactor_extract().contains(&CodeActionKind::refactor().append("other")));
    assert!(!CodeActionKind::refactor_extract().contains(&CodeActionKind::empty().append("other").append("refactor")));
    assert!(!CodeActionKind::refactor_extract().contains(&CodeActionKind::empty().append("refactory")));
}

#[test]
fn code_action_kind_intersects() {
    assert!(CodeActionKind::refactor_extract().intersects(&CodeActionKind::refactor_extract()));
    assert!(CodeActionKind::refactor_extract().intersects(&CodeActionKind::refactor()));
    assert!(CodeActionKind::refactor_extract().intersects(&CodeActionKind::refactor_extract().append("other")));

    assert!(!CodeActionKind::refactor_extract().intersects(&CodeActionKind::refactor().append("other")));
    assert!(!CodeActionKind::refactor_extract().intersects(&CodeActionKind::empty().append("other").append("refactor")));
    assert!(!CodeActionKind::refactor_extract().intersects(&CodeActionKind::empty().append("refactory")));
}

#[test]
fn semantic_tokens_builder_simple() {
    let mut builder = SemanticTokensBuilder::new(None);
    builder.push(1, 0, 5, 1, Some(1));
    builder.push(1, 10, 4, 2, Some(2));
    builder.push(2, 2, 3, 2, Some(2));
    assert_eq!(builder.build(None).data, vec![1, 0, 5, 1, 1, 0, 10, 4, 2, 2, 1, 2, 3, 2, 2]);
}

#[test]
fn semantic_tokens_builder_no_modifier() {
    let mut builder = SemanticTokensBuilder::new(None);
    builder.push(1, 0, 5, 1, None);
    builder.push(1, 10, 4, 2, None);
    builder.push(2, 2, 3, 2, None);
    assert_eq!(builder.build(None).data, vec![1, 0, 5, 1, 0, 0, 10, 4, 2, 0, 1, 2, 3, 2, 0]);
}

#[test]
fn semantic_tokens_builder_out_of_order_1() {
    let mut builder = SemanticTokensBuilder::new(None);
    builder.push(2, 0, 5, 1, Some(1));
    builder.push(2, 10, 1, 2, Some(2));
    builder.push(2, 15, 2, 3, Some(3));
    builder.push(1, 0, 4, 4, Some(4));
    assert_eq!(builder.build(None).data, vec![1, 0, 4, 4, 4, 1, 0, 5, 1, 1, 0, 10, 1, 2, 2, 0, 5, 2, 3, 3]);
}

#[test]
fn semantic_tokens_builder_out_of_order_2() {
    let mut builder = SemanticTokensBuilder::new(None);
    builder.push(2, 10, 5, 1, Some(1));
    builder.push(2, 2, 4, 2, Some(2));
    assert_eq!(builder.build(None).data, vec![2, 2, 4, 2, 2, 0, 8, 5, 1, 1]);
}

#[test]
fn semantic_tokens_builder_with_legend() {
    let legend = SemanticTokensLegend::new(
        strings(&["aType", "bType", "cType", "dType"]),
        Some(strings(&["mod0", "mod1", "mod2", "mod3", "mod4", "mod5"])),
    );
    let mut builder = SemanticTokensBuilder::new(Some(&legend));
    builder.push_range(&r(1.0, 0.0, 1.0, 5.0), "bType", None).unwrap();
    builder.push_range(&r(2.0, 0.0, 2.0, 4.0), "cType", Some(&["mod0", "mod5"][..])).unwrap();
    builder.push_range(&r(3.0, 0.0, 3.0, 3.0), "dType", Some(&["mod2", "mod4"][..])).unwrap();
    assert_eq!(builder.build(None).data, vec![1, 0, 5, 1, 0, 1, 0, 4, 2, 1 | (1 << 5), 1, 0, 3, 3, (1 << 2) | (1 << 4)]);
}

#[test]
fn markdown_codeblock_rendering_is_swapped_111604() {
    let mut md = MarkdownString::new(None, false);
    md.append_codeblock("<img src=0 onerror=\"alert(1)\">", Some("html"));
    assert_eq!(md.value.to_string_lossy(), "\n```html\n<img src=0 onerror=\"alert(1)\">\n```\n");
}

#[test]
fn notebook_cell_output_item_factories() {
    // tipo MIME inválido
    assert!(NotebookCellOutputItem::new(Vec::new(), "invalid").is_err());

    // --- err

    let mut item = NotebookCellOutputItem::error(&ErrorLike::from(&Error::new("")));
    assert_eq!(item.mime(), "application/vnd.code.notebook.error");
    item = NotebookCellOutputItem::error(&ErrorLike { name: "Hello".to_owned(), ..ErrorLike::default() });
    assert_eq!(item.mime(), "application/vnd.code.notebook.error");

    // --- JSON

    item = NotebookCellOutputItem::json(&num(1.0), None);
    assert_eq!(item.mime(), "text/x-json");
    assert_eq!(item.data, b"1".to_vec());

    item = NotebookCellOutputItem::json(&num(1.0), Some("foo/bar"));
    assert_eq!(item.mime(), "foo/bar");
    assert_eq!(item.data, b"1".to_vec());

    item = NotebookCellOutputItem::json(&Value::Bool(true), None);
    assert_eq!(item.mime(), "text/x-json");
    assert_eq!(item.data, b"true".to_vec());

    item = NotebookCellOutputItem::json(&Value::Array(vec![Value::Bool(true), num(1.0), text("ddd")]), None);
    assert_eq!(item.mime(), "text/x-json");
    assert_eq!(item.data, "[\n\ttrue,\n\t1,\n\t\"ddd\"\n]".as_bytes().to_vec());

    // --- text

    item = NotebookCellOutputItem::text("Hęłlö", None);
    assert_eq!(item.mime(), "text/plain");
    assert_eq!(item.data, "Hęłlö".as_bytes().to_vec());

    item = NotebookCellOutputItem::text("Hęłlö", Some("foo/bar"));
    assert_eq!(item.mime(), "foo/bar");
    assert_eq!(item.data, "Hęłlö".as_bytes().to_vec());
}

#[test]
fn file_decoration_validate() {
    let badge = |value: &str| FileDecoration::new(Some(FileDecorationBadge::Text(value.to_owned())), None, None);

    assert!(FileDecoration::validate(&badge("u")).unwrap());
    assert!(FileDecoration::validate(&badge("ü")).unwrap());
    assert!(FileDecoration::validate(&badge("1")).unwrap());
    assert!(FileDecoration::validate(&badge("ãã")).unwrap());
    assert!(FileDecoration::validate(&badge("👋")).unwrap());
    assert!(FileDecoration::validate(&badge("👋👋")).unwrap());
    assert!(FileDecoration::validate(&badge("👩‍👩‍👧‍👧")).unwrap());
    assert!(FileDecoration::validate(&badge("போ")).unwrap());
    assert!(FileDecoration::validate(&badge("hel")).is_err());
    assert!(FileDecoration::validate(&badge("👋👋👋")).is_err());
    assert!(FileDecoration::validate(&badge("புன்சிரிப்போடு")).is_err());
    assert!(FileDecoration::validate(&badge("ããã")).is_err());
}

#[test]
fn runtime_stable_type_def_changed() {
    // ver https://github.com/microsoft/vscode/issues/231938
    let mut m = LanguageModelChatMessage::new(LanguageModelChatMessageRole::User, ChatMessageContent::Parts(Vec::new()), None);
    assert!(m.content().is_empty());
    m.set_content(ChatMessageContent::Text("Hello".to_owned()));
    assert_eq!(m.content().len(), 1);
    let LanguageModelChatMessagePart::Text(part) = &m.content()[0] else { panic!("deveria ser uma parte de texto") };
    assert_eq!(part, &LanguageModelTextPart::new("Hello", None));
}

#[test]
fn language_model_error_rate_limited_keeps_retry_after_across_serialization() {
    let error = LanguageModelError::try_deserialize(&serialize_language_model_error(&LanguageModelError::rate_limited(Some("Too many requests"), Some(1500.0))));
    let error = error.expect("o nome é LanguageModelError");
    assert_eq!((error.error().name.as_str(), error.code(), error.retry_after()), ("LanguageModelError", "RateLimited", Some(1500.0)));
}
