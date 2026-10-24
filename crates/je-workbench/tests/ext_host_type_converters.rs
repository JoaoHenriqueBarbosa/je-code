//! Porte de `src/vs/workbench/api/test/common/extHostTypeConverters.test.ts` (suíte `extHostTypeConverters`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `IconPath.from` aceita no original textos e objetos sem a forma do tipo (`as any`): a entrada é o `IconPathInput`, com `Path` e
//!   `Object { light, dark }` (a chave ausente é `None`). `assert.strictEqual(IconPath.from(themeIcon), themeIcon)` (identidade) é a igualdade
//!   do ícone ou do `URI` devolvido. O `IconPathDto` de saída é conferido com `match`.
//! - `ChatRequestModeInstructions.to` recebe `IChatRequestModeInstructions | Dto<IChatRequestModeInstructions>`: aqui são o mesmo tipo (um
//!   `UriComponents` já é um `URI`), então os dois casos usam o mesmo `to`. `assert.deepStrictEqual(result, {...})` é conferido campo a campo,
//!   com `assert_deep_strict_equal` nos `metadata` (um `Dynamic`). `Record<string, boolean | string | number>` é um `Dynamic` objeto.
//! - `ChatResponseVoiceProgressPart` e `ChatRequestModeInstructions` (a classe da API e o conversor) estão em
//!   `ext_host_type_converters::chat`.
//! - NÃO PORTADOS (a camada de chat está fora do objetivo atual e não há o `IChatRequestVariableEntry`, o `ChatReferenceBinaryData` nem o
//!   `ChatToolInvocationPart`/`ChatSubagentToolInvocationData` no porte): `ChatPromptReference expands an element with a screenshot into text
//!   and binary references` e `ChatToolInvocationPart converts subagent data with its model name`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::themables::{ThemeColor, ThemeIcon};
use je_base::uri::{URI, UriComponents};
use je_editor::offset_range::OffsetRange;
use je_platform::resources::uri_file;
use je_workbench::api::common::ext_host_protocol::IconPathDto;
use je_workbench::api::common::ext_host_type_converters::chat::{
    ChatLanguageModelToolReferenceApi, ChatRequestModeInstructionsApi, ChatRequestToolEntry, ChatRequestToolReferenceEntry,
};
use je_workbench::api::common::ext_host_type_converters::{
    ChatRequestModeInstructions, ChatResponseVoiceProgressPart, ExtHostChatResponseVoiceProgressPart, IChatRequestModeInstructions, IconPath, IconPathInput, UriOrPath,
};
use je_workbench::api::common::ext_host_types as types;

const DATA_URI: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

fn text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

fn tool_entry(id: &str, range: Option<OffsetRange>) -> ChatRequestToolReferenceEntry {
    ChatRequestToolReferenceEntry::Tool(ChatRequestToolEntry { id: id.to_owned(), name: id.to_owned(), value: None, range })
}

fn metadata(key: &str, value: Dynamic) -> Dynamic {
    Dynamic::object([(key, value)])
}

#[test]
fn converts_voice_progress_to_hidden_chat_progress() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let dto = ChatResponseVoiceProgressPart::from(&ExtHostChatResponseVoiceProgressPart::new("investigating", "Investigating the relevant code."));
    assert_eq!(dto.kind, "voiceProgress");
    assert_eq!(dto.id, "investigating");
    assert_eq!(dto.value, "Investigating the relevant code.");
}

// ---- IconPath.from

#[test]
fn icon_path_from_undefined() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert!(IconPath::from(None).is_none());
}

#[test]
fn icon_path_from_theme_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let theme_icon = ThemeIcon { id: "account".to_owned(), color: Some(ThemeColor::new("testing.iconForeground")) };
    match IconPath::from(Some(IconPathInput::ThemeIcon(theme_icon.clone()))) {
        Some(IconPathDto::ThemeIcon(result)) => assert_eq!(result, theme_icon),
        other => panic!("esperava o próprio ThemeIcon: {other:?}"),
    }
}

#[test]
fn icon_path_from_uri() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let uri = URI::parse(DATA_URI, false);
    match IconPath::from(Some(IconPathInput::Uri(uri.clone()))) {
        Some(IconPathDto::Uri(result)) => assert!(URI::ptr_eq(&result, &uri)),
        other => panic!("esperava o próprio URI: {other:?}"),
    }
}

#[test]
fn icon_path_from_string() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let str = "/path/to/icon.png";
    let Some(IconPathDto::Uri(r1)) = IconPath::from(Some(IconPathInput::Path(str.to_owned()))) else {
        panic!("esperava um URI");
    };
    assert_eq!(r1.scheme(), "file");
    assert_eq!(r1.path(), str);
}

#[test]
fn icon_path_from_dark_only() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let input_dark = uri_file("/path/to/dark.png");
    let result = IconPath::from(Some(IconPathInput::Object { light: None, dark: Some(UriOrPath::Uri(input_dark.clone())) }));
    let Some(IconPathDto::Themed { light, dark }) = result else {
        panic!("esperava { light, dark }");
    };
    assert_eq!(text(&dark), text(&input_dark));
    assert_eq!(text(&light), text(&input_dark));
}

#[test]
fn icon_path_from_dark_light() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let input_light = uri_file("/path/to/light.png");
    let input_dark = uri_file("/path/to/dark.png");
    let result = IconPath::from(Some(IconPathInput::Object { light: Some(UriOrPath::Uri(input_light.clone())), dark: Some(UriOrPath::Uri(input_dark.clone())) }));
    let Some(IconPathDto::Themed { light, dark }) = result else {
        panic!("esperava { light, dark }");
    };
    assert_eq!(text(&dark), text(&input_dark));
    assert_eq!(text(&light), text(&input_light));
}

#[test]
fn icon_path_from_dark_light_strings() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let (input_light, input_dark) = ("/path/to/light.png", "/path/to/dark.png");
    let result = IconPath::from(Some(IconPathInput::Object {
        light: Some(UriOrPath::Path(input_light.to_owned())),
        dark: Some(UriOrPath::Path(input_dark.to_owned())),
    }));
    let Some(IconPathDto::Themed { light, dark }) = result else {
        panic!("esperava { light, dark }");
    };
    assert_eq!(dark.path(), input_dark);
    assert_eq!(light.path(), input_light);
}

#[test]
fn icon_path_from_invalid_object() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    // `{ foo: 'bar' }`: um objeto sem `dark`.
    let result = IconPath::from(Some(IconPathInput::Object { light: None, dark: None }));
    assert!(result.is_none());
}

#[test]
fn icon_path_from_light_only() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let input = uri_file("/path/to/light.png");
    let result = IconPath::from(Some(IconPathInput::Object { light: Some(UriOrPath::Uri(input)), dark: None }));
    assert!(result.is_none());
}

// ---- IconPath.to

#[test]
fn icon_path_to_undefined() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert!(IconPath::to(None).is_none());
}

#[test]
fn icon_path_to_theme_icon() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let theme_icon = ThemeIcon { id: "account".to_owned(), color: None };
    match IconPath::to(Some(IconPathDto::ThemeIcon(theme_icon.clone()))) {
        Some(types::IconPath::ThemeIcon(result)) => assert_eq!(result, theme_icon),
        _ => panic!("esperava o próprio ThemeIcon"),
    }
}

#[test]
fn icon_path_to_uri() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let uri = URI::from(
        &UriComponents {
            scheme: Some("data".into()),
            path: Some("image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==".into()),
            ..UriComponents::default()
        },
        false,
    );
    let Some(types::IconPath::Uri(result)) = IconPath::to(Some(IconPathDto::Uri(uri.clone()))) else {
        panic!("esperava um URI");
    };
    assert_eq!(text(&result), text(&uri));
}

#[test]
fn icon_path_to_dark_light() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let light_input = URI::from(&UriComponents { scheme: Some("file".into()), path: Some("/path/to/light.png".into()), ..UriComponents::default() }, false);
    let dark_input = URI::from(&UriComponents { scheme: Some("file".into()), path: Some("/path/to/dark.png".into()), ..UriComponents::default() }, false);
    let result = IconPath::to(Some(IconPathDto::Themed { light: light_input.clone(), dark: dark_input.clone() }));
    let Some(types::IconPath::LightDark { light, dark }) = result else {
        panic!("esperava { light, dark }");
    };
    assert_eq!(text(&dark), text(&dark_input));
    assert_eq!(text(&light), text(&light_input));
}

// ---- ChatRequestModeInstructions

#[test]
fn chat_request_mode_instructions_to_returns_undefined_for_undefined_input() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert!(ChatRequestModeInstructions::to(None).unwrap().is_none());
}

#[test]
fn chat_request_mode_instructions_from_returns_undefined_for_undefined_input() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert!(ChatRequestModeInstructions::from(None).is_none());
}

#[test]
fn chat_request_mode_instructions_to_converts_ichat_request_mode_instructions_to_api_type() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let uri = URI::parse("file:///custom-agent", false);
    let input = IChatRequestModeInstructions {
        uri: Some(uri.clone()),
        name: "test-mode".to_owned(),
        content: "test content".to_owned(),
        tool_references: vec![tool_entry("tool1", Some(OffsetRange { start: 0, end_exclusive: 5 }))],
        allowed_subagents: Some(vec!["agent1".to_owned(), "agent2".to_owned()]),
        metadata: Some(metadata("key", Dynamic::string("value"))),
        is_builtin: Some(false),
    };

    let result = ChatRequestModeInstructions::to(Some(&input)).unwrap().unwrap();
    assert!(URI::ptr_eq(result.uri.as_ref().unwrap(), &uri));
    assert_eq!(result.name, "test-mode");
    assert_eq!(result.content, "test content");
    assert_eq!(result.tool_references, Some(vec![ChatLanguageModelToolReferenceApi { name: "tool1".to_owned(), range: Some((0, 5)) }]));
    assert_eq!(result.allowed_subagents, Some(vec!["agent1".to_owned(), "agent2".to_owned()]));
    assert_deep_strict_equal(result.metadata.as_ref().unwrap(), &metadata("key", Dynamic::string("value")));
    assert_eq!(result.is_builtin, Some(false));
}

#[test]
fn chat_request_mode_instructions_to_handles_dto_with_uri_components() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let input = IChatRequestModeInstructions {
        uri: Some(URI::from(&UriComponents { scheme: Some("file".into()), path: Some("/custom-agent".into()), ..UriComponents::default() }, false)),
        name: "test-mode".to_owned(),
        content: "test content".to_owned(),
        tool_references: Vec::new(),
        allowed_subagents: None,
        metadata: None,
        is_builtin: Some(true),
    };

    let result = ChatRequestModeInstructions::to(Some(&input)).unwrap().unwrap();
    assert!(result.uri.is_some());
    assert_eq!(result.name, "test-mode");
    assert_eq!(result.is_builtin, Some(true));
    assert_eq!(result.tool_references, Some(Vec::new()));
}

#[test]
fn chat_request_mode_instructions_from_converts_api_type_to_ichat_request_mode_instructions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let uri = URI::parse("file:///custom-agent", false);
    let input = ChatRequestModeInstructionsApi {
        uri: Some(uri.clone()),
        name: "test-mode".to_owned(),
        content: "test content".to_owned(),
        tool_references: Some(vec![ChatLanguageModelToolReferenceApi { name: "tool1".to_owned(), range: Some((0, 5)) }]),
        allowed_subagents: None,
        metadata: Some(metadata("key", Dynamic::string("value"))),
        is_builtin: Some(false),
    };

    let result = ChatRequestModeInstructions::from(Some(&input)).unwrap();
    assert!(URI::ptr_eq(result.uri.as_ref().unwrap(), &uri));
    assert_eq!(result.name, "test-mode");
    assert_eq!(result.content, "test content");
    assert_eq!(result.tool_references.len(), 1);
    let ChatRequestToolReferenceEntry::Tool(tool) = &result.tool_references[0] else {
        panic!("esperava uma referência de ferramenta (kind: 'tool')");
    };
    assert_eq!(tool.id, "tool1");
    assert_eq!(tool.name, "tool1");
    assert!(tool.value.is_none());
    assert_eq!(tool.range, Some(OffsetRange { start: 0, end_exclusive: 5 }));
    assert!(result.allowed_subagents.is_none());
    assert_deep_strict_equal(result.metadata.as_ref().unwrap(), &metadata("key", Dynamic::string("value")));
    assert_eq!(result.is_builtin, Some(false));
}

#[test]
fn chat_request_mode_instructions_from_handles_missing_tool_references() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let input = ChatRequestModeInstructionsApi {
        uri: None,
        name: "test-mode".to_owned(),
        content: "test content".to_owned(),
        tool_references: None,
        allowed_subagents: None,
        metadata: None,
        is_builtin: None,
    };

    let result = ChatRequestModeInstructions::from(Some(&input)).unwrap();
    assert!(result.tool_references.is_empty());
}

#[test]
fn chat_request_mode_instructions_roundtrip_from_to_preserves_data() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let uri = URI::parse("file:///custom-agent", false);
    let api_input = ChatRequestModeInstructionsApi {
        uri: Some(uri.clone()),
        name: "roundtrip-mode".to_owned(),
        content: "roundtrip content".to_owned(),
        tool_references: Some(vec![
            ChatLanguageModelToolReferenceApi { name: "tool1".to_owned(), range: None },
            ChatLanguageModelToolReferenceApi { name: "tool2".to_owned(), range: Some((10, 20)) },
        ]),
        allowed_subagents: None,
        metadata: Some(metadata("flag", Dynamic::Bool(true))),
        is_builtin: Some(false),
    };

    let internal = ChatRequestModeInstructions::from(Some(&api_input)).unwrap();
    let back_to_api = ChatRequestModeInstructions::to(Some(&internal)).unwrap().unwrap();

    assert_eq!(back_to_api.name, api_input.name);
    assert_eq!(back_to_api.content, api_input.content);
    assert_eq!(back_to_api.is_builtin, api_input.is_builtin);
    assert_eq!(text(back_to_api.uri.as_ref().unwrap()), text(&uri));
    let tool_references = back_to_api.tool_references.as_ref().unwrap();
    assert_eq!(tool_references.len(), 2);
    assert_eq!(tool_references[0].name, "tool1");
    assert_eq!(tool_references[0].range, None);
    assert_eq!(tool_references[1].name, "tool2");
    assert_eq!(tool_references[1].range, Some((10, 20)));
}
