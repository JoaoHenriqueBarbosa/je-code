//! Porte de `src/vs/workbench/api/test/browser/extHostTypeConverter.test.ts` (suíte `ExtHostTypeConverter`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `size(data.uris!)` é `len()` do `OrderedMap`; `!!data.uris['foo:path']` é `get(...).is_some()`. O `uris` do `MarkdownString.from` usa o léxico
//!   `markdown_links` (um subconjunto do `marked`, ver o módulo).
//! - `LanguageSelector.from({ language, notebookType })` devolve o `LanguageFilter` do editor, que tem `exclusive: bool` (o `undefined` do original
//!   é `false`) e o `pattern` ausente; `assert.deepStrictEqual` é a igualdade do `LanguageSelector`.
//! - `NotebookData.from(d)` devolve o `NotebookDataDto` (`cells[0].language` é `language`, `source` é o texto, `metadata` um `Dynamic` conferido
//!   com `assert_deep_strict_equal`). `Array.from(dto.valueBytes.buffer)` é `VSBuffer::to_vec`.
//! - `WorkspaceEdit.from(ws)` sem `versionInfo` é `from(&ws, None)`; `<IWorkspaceTextEditDto>dto.edits[0]` é o `match` na variante `Text`.
//! - `ChatAgentResult.to({ metadata })`: a parte revivida é um `Dynamic` objeto com a carga nativa (`ObjectRef::host`), que faz o `instanceof` do
//!   original; `part.toJSON()` é `to_json()` convertido com `Dynamic::from_json`. O caso `does not throw` confere que `to` volta (qualquer
//!   pânico reprova o teste).
//! - NÃO PORTADOS (dependem da camada de chat, fora do objetivo atual, que não existe no porte): `LanguageModelChatMessage2 converters
//!   preserve tool-result data parts across the provider boundary #313920` (`LanguageModelChatMessage2.from/to` e o `IChatMessage`) e
//!   `ChatAgentRequest tool invocation token identifies the request it was issued for` (`ChatAgentRequest.to` e o `IChatAgentRequest`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::marshalling_ids::MarshalledId;
use je_base::uri::URI;
use je_editor::language_selector::{LanguageFilter, LanguageSelector as EditorLanguageSelector};
use je_workbench::api::common::ext_host_protocol::WorkspaceEditEntryDto;
use je_workbench::api::common::ext_host_type_converters::language_selector::DocumentFilterInput;
use je_workbench::api::common::ext_host_type_converters::{
    ChatAgentResult, DocumentSelectorInput, IChatAgentResult, LanguageSelector, MarkdownString, NotebookCellOutputItem, NotebookData, WorkspaceEdit,
};
use je_workbench::api::common::ext_host_types as types;
use je_workbench::api::common::ext_host_types::{LanguageModelDataPart, Position, SnippetString, SnippetTextEdit};

fn markdown(text: &str) -> je_base::html_content::IMarkdownString {
    MarkdownString::from(&types::MarkedString::Text(text.to_owned()))
}

fn size(data: &je_base::html_content::IMarkdownString) -> usize {
    data.uris.as_ref().expect("uris").len()
}

fn has_uri(data: &je_base::html_content::IMarkdownString, href: &str) -> bool {
    data.uris.as_ref().expect("uris").get(&href.into()).is_some()
}

#[test]
fn markdown_convert_uris() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let mut data = markdown("Hello");
    assert_eq!(size(&data), 0);
    assert_eq!(data.value, "Hello");

    data = markdown("Hello [link](foo)");
    assert_eq!(data.value, "Hello [link](foo)");
    assert_eq!(size(&data), 0); // no scheme, no uri

    data = markdown("Hello [link](www.noscheme.bad)");
    assert_eq!(data.value, "Hello [link](www.noscheme.bad)");
    assert_eq!(size(&data), 0); // no scheme, no uri

    data = markdown("Hello [link](foo:path)");
    assert_eq!(data.value, "Hello [link](foo:path)");
    assert_eq!(size(&data), 1);
    assert!(has_uri(&data, "foo:path"));

    data = markdown("hello@foo.bar");
    assert_eq!(data.value, "hello@foo.bar");
    assert_eq!(size(&data), 1);
    // assert.ok(!!data.uris!['mailto:hello@foo.bar']);

    data = markdown("*hello* [click](command:me)");
    assert_eq!(data.value, "*hello* [click](command:me)");
    assert_eq!(size(&data), 1);
    assert!(has_uri(&data, "command:me"));

    data = markdown("*hello* [click](file:///somepath/here). [click](file:///somepath/here)");
    assert_eq!(data.value, "*hello* [click](file:///somepath/here). [click](file:///somepath/here)");
    assert_eq!(size(&data), 1);
    assert!(has_uri(&data, "file:///somepath/here"));

    data = markdown("*hello* [click](file:///somepath/here). [click](file:///somepath/here)");
    assert_eq!(data.value, "*hello* [click](file:///somepath/here). [click](file:///somepath/here)");
    assert_eq!(size(&data), 1);
    assert!(has_uri(&data, "file:///somepath/here"));

    data = markdown("*hello* [click](file:///somepath/here). [click](file:///somepath/here2)");
    assert_eq!(data.value, "*hello* [click](file:///somepath/here). [click](file:///somepath/here2)");
    assert_eq!(size(&data), 2);
    assert!(has_uri(&data, "file:///somepath/here"));
    assert!(has_uri(&data, "file:///somepath/here2"));
}

// NÃO PORTADO: `LanguageModelChatMessage2 converters preserve tool-result data parts across the provider boundary #313920` (ver o topo).

#[test]
fn npm_script_explorer_running_a_script_from_the_hover_does_not_work_65561() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let data = markdown("*hello* [click](command:npm.runScriptFromHover?%7B%22documentUri%22%3A%7B%22%24mid%22%3A1%2C%22external%22%3A%22file%3A%2F%2F%2Fc%253A%2Ffoo%2Fbaz.ex%22%2C%22path%22%3A%22%2Fc%3A%2Ffoo%2Fbaz.ex%22%2C%22scheme%22%3A%22file%22%7D%2C%22script%22%3A%22dev%22%7D)");
    // assert that both uri get extracted but that the latter is only decoded once...
    assert_eq!(size(&data), 2);
    for value in data.uris.as_ref().expect("uris").values() {
        if value.scheme() == "file" {
            assert!(value.to_string(false).to_string_lossy().starts_with("file:///c%3A"));
        } else {
            assert_eq!(value.scheme(), "command");
        }
    }
}

#[test]
fn notebook_metadata_is_ignored_when_using_notebook_serializer_125716() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let mut d = types::NotebookData::new(Vec::new());
    d.cells.push(types::NotebookCellData::new(types::NotebookCellKind::Code, "hello", "fooLang", None, None, None, None));
    d.metadata = Some(Dynamic::object([("foo", Dynamic::string("bar")), ("bar", Dynamic::Number(123.0))]));

    let dto = NotebookData::from(&d);

    assert_eq!(dto.cells.len(), 1);
    assert_eq!(dto.cells[0].language, "fooLang");
    assert_eq!(dto.cells[0].source, "hello");
    assert_deep_strict_equal(&dto.metadata, d.metadata.as_ref().unwrap());
}

#[test]
fn notebook_cell_output_item() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let item = types::NotebookCellOutputItem::text("Hello", Some("foo/bar"));

    let dto = NotebookCellOutputItem::from(&item);

    assert_eq!(dto.mime, "foo/bar");
    assert_eq!(dto.value_bytes.to_vec(), "Hello".as_bytes().to_vec());

    let item2 = NotebookCellOutputItem::to(&dto).unwrap();

    assert_eq!(item2.mime(), item.mime());
    assert_eq!(item2.data, item.data);
}

#[test]
fn language_selector() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let out = LanguageSelector::from(Some(&DocumentSelectorInput::Filter(DocumentFilterInput {
        language: Some("bat".to_owned()),
        notebook_type: Some("xxx".to_owned()),
        ..DocumentFilterInput::default()
    })));
    assert_eq!(
        out,
        Some(EditorLanguageSelector::Filter(LanguageFilter {
            language: Some("bat".to_owned()),
            notebook_type: Some("xxx".to_owned()),
            scheme: None,
            pattern: None,
            ..LanguageFilter::default()
        }))
    );
}

#[test]
fn workspace_edit_round_trip_preserves_insert_as_snippet_and_keep_whitespace_178654_325990() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let uri = URI::parse("file:///foo/bar", false);
    let mut ws = types::WorkspaceEdit::new();
    let mut snippet = SnippetTextEdit::insert(Position::new(1.0, 1.0).unwrap(), SnippetString::new(Some("foo$0bar")));
    snippet.keep_whitespace = Some(true);
    ws.set(&uri, vec![snippet]);

    let dto = WorkspaceEdit::from(&ws, None);
    let WorkspaceEditEntryDto::Text(first) = &dto.edits[0] else {
        panic!("esperava uma edição de texto");
    };
    assert_eq!(first.text_edit.insert_as_snippet, Some(true));

    let ws2 = WorkspaceEdit::to(&dto).unwrap();
    let dto2 = WorkspaceEdit::from(&ws2, None);
    let WorkspaceEditEntryDto::Text(first2) = &dto2.edits[0] else {
        panic!("esperava uma edição de texto");
    };
    assert_eq!(first2.text_edit.insert_as_snippet, Some(true));
    assert_eq!(first2.text_edit.keep_whitespace, Some(true));
}

// ---- ChatAgentResult

fn revived_data_part(value: &Dynamic) -> std::rc::Rc<LanguageModelDataPart> {
    let Dynamic::Object(object) = value else {
        panic!("esperava a parte revivida");
    };
    object.host::<LanguageModelDataPart>().expect("revived instanceof LanguageModelDataPart")
}

fn result_with_part(part: Dynamic) -> IChatAgentResult {
    IChatAgentResult { metadata: Some(Dynamic::object([("part", part)])), ..IChatAgentResult::default() }
}

#[test]
fn chat_agent_result_revive_metadata_roundtrips_base64_encoded_language_model_data_part() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let original_data = vec![10u8, 20, 30, 40];
    let part = LanguageModelDataPart::new(original_data.clone(), "image/png", None);
    let serialized = part.to_json();

    let result = ChatAgentResult::to(&result_with_part(Dynamic::from_json(&serialized)));
    let revived = revived_data_part(&result.metadata.as_ref().unwrap().get_str("part"));

    assert_eq!(revived.data, original_data);
    assert_eq!(revived.mime_type, "image/png");
}

#[test]
fn chat_agent_result_revive_metadata_decodes_legacy_buffer_like_language_model_data_part_shape() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let legacy_serialized = Dynamic::object([
        ("$mid", Dynamic::Number(f64::from(MarshalledId::LanguageModelDataPart as i32))),
        ("data", Dynamic::object([("type", Dynamic::string("Buffer")), ("data", Dynamic::array([Dynamic::Number(1.0), Dynamic::Number(2.0), Dynamic::Number(3.0)]))])),
        ("mimeType", Dynamic::string("image/jpeg")),
    ]);

    let result = ChatAgentResult::to(&result_with_part(legacy_serialized));
    let revived = revived_data_part(&result.metadata.as_ref().unwrap().get_str("part"));

    assert_eq!(revived.data, vec![1, 2, 3]);
    assert_eq!(revived.mime_type, "image/jpeg");
}

#[test]
fn chat_agent_result_revive_metadata_does_not_throw_on_malformed_language_model_data_part_shape() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let malformed = Dynamic::object([
        ("$mid", Dynamic::Number(f64::from(MarshalledId::LanguageModelDataPart as i32))),
        ("data", Dynamic::Null),
        ("mimeType", Dynamic::string("image/png")),
    ]);

    // assert.doesNotThrow: qualquer pânico reprova o caso
    let _ = ChatAgentResult::to(&result_with_part(malformed));
}

// NÃO PORTADO: `ChatAgentRequest tool invocation token identifies the request it was issued for` (ver o topo).
