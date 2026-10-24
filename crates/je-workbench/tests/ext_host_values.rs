//! Casos próprios de `ext_host_values.rs` (o alicerce `$$class` do extension host). Não há suíte equivalente
//! no original: o TypeScript decide por `instanceof`, e aqui o `instanceof` é a propriedade `$$class`.
//!
//! Cada tipo de `ext_host_types` que cruza como valor faz a ida e volta (`to_dynamic` e `from_dynamic`), a
//! classe é conferida (`class_of`, `is_instance_of`, a herança de `Selection` para `Range`), e o `revive`
//! de `marshalling.ts` sobre `Dynamic` troca `$mid` por `Uri`, `RegExp` e `Date`.

use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::JsString;
use je_base::html_content::{IsTrusted, MarkdownStringTrustedOptions};
use je_base::path::Platform;
use je_base::themables::{ThemeColor, ThemeIcon};
use je_base::uri::URI;
use je_workbench::api::common::ext_host_types::{
    Color, Command, Diagnostic, DiagnosticCode, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag, EndOfLine, Location, MarkdownString,
    NotebookRange, Position, Range, Selection, SnippetString, SymbolKind, SymbolTag, TextEdit, TypeHierarchyItem, CallHierarchyItem,
};
use je_workbench::api::common::ext_host_values::{
    ApiValue, CLASS_KEY, array_buffer_to_dynamic, bytes_of, class_of, is_instance_of, revive, tag, vs_buffer_from_dynamic, vs_buffer_to_dynamic,
};

fn position(line: f64, character: f64) -> Position {
    Position::new(line, character).expect("posição válida")
}

fn range(start_line: f64, start_character: f64, end_line: f64, end_character: f64) -> Range {
    Range::from_coordinates(start_line, start_character, end_line, end_character).expect("intervalo válido")
}

fn uri(value: &str) -> URI {
    URI::parse(value, true)
}

#[test]
fn tag_grava_a_classe_e_class_of_le() {
    let marked = tag(Dynamic::object([("a", Dynamic::Number(1.0))]), "Position");
    assert_eq!(class_of(&marked), Some("Position"));
    assert_eq!(marked.get_str(CLASS_KEY), Dynamic::string("Position"));
    assert_eq!(marked.get_str("a"), Dynamic::Number(1.0));
}

#[test]
fn class_of_ignora_o_que_nao_e_objeto_marcado() {
    assert_eq!(class_of(&Dynamic::Undefined), None);
    assert_eq!(class_of(&Dynamic::Null), None);
    assert_eq!(class_of(&Dynamic::string("Position")), None);
    assert_eq!(class_of(&Dynamic::array([])), None);
    assert_eq!(class_of(&Dynamic::object([("a", Dynamic::Number(1.0))])), None);
    // fora da tabela de classes conhecidas
    assert_eq!(class_of(&tag(Dynamic::object::<&str>([]), "Desconhecida")), None);
    // `$$class` que não é texto
    assert_eq!(class_of(&Dynamic::object([(CLASS_KEY, Dynamic::Number(1.0))])), None);
}

#[test]
fn tag_em_valor_que_nao_e_objeto_devolve_o_valor() {
    assert_eq!(tag(Dynamic::Number(3.0), "Position"), Dynamic::Number(3.0));
    assert_eq!(class_of(&tag(Dynamic::string("x"), "Range")), None);
}

#[test]
fn is_instance_of_confere_a_classe() {
    let value = position(1.0, 2.0).to_dynamic();
    assert!(is_instance_of(&value, "Position"));
    assert!(!is_instance_of(&value, "Range"));
    assert!(!is_instance_of(&Dynamic::Undefined, "Position"));
}

#[test]
fn selection_e_instancia_de_range_mas_range_nao_e_de_selection() {
    let selection = Selection::new(position(3.0, 1.0), position(1.0, 4.0)).to_dynamic();
    assert!(is_instance_of(&selection, "Selection"));
    assert!(is_instance_of(&selection, "Range"));
    let plain_range = range(0.0, 0.0, 1.0, 1.0).to_dynamic();
    assert!(is_instance_of(&plain_range, "Range"));
    assert!(!is_instance_of(&plain_range, "Selection"));
}

#[test]
fn position_ida_e_volta() {
    let original = position(4.0, 7.0);
    let value = original.to_dynamic();
    assert_eq!(value.get_str("line"), Dynamic::Number(4.0));
    assert_eq!(value.get_str("character"), Dynamic::Number(7.0));
    assert_eq!(Position::from_dynamic(&value).unwrap(), original);
}

#[test]
fn position_de_valor_invalido_falha_como_o_construtor() {
    let negative = tag(Dynamic::object([("line", Dynamic::Number(-1.0)), ("character", Dynamic::Number(0.0))]), "Position");
    assert_eq!(Position::from_dynamic(&negative).unwrap_err().message, "Illegal argument: line must be non-negative");
    let not_number = tag(Dynamic::object([("line", Dynamic::string("1")), ("character", Dynamic::Number(0.0))]), "Position");
    assert_eq!(Position::from_dynamic(&not_number).unwrap_err().name, "TypeError");
}

#[test]
fn from_dynamic_exige_a_classe() {
    let untagged = Dynamic::object([("line", Dynamic::Number(1.0)), ("character", Dynamic::Number(1.0))]);
    assert_eq!(Position::from_dynamic(&untagged).unwrap_err().name, "TypeError");
    let range_value = range(0.0, 0.0, 1.0, 1.0).to_dynamic();
    assert_eq!(Position::from_dynamic(&range_value).unwrap_err().name, "TypeError");
    assert_eq!(Selection::from_dynamic(&range_value).unwrap_err().name, "TypeError");
    assert_eq!(Position::from_dynamic(&Dynamic::Undefined).unwrap_err().name, "TypeError");
}

#[test]
fn range_ida_e_volta() {
    let original = range(1.0, 2.0, 3.0, 4.0);
    let value = original.to_dynamic();
    assert!(is_instance_of(&value.get_str("start"), "Position"));
    assert_eq!(Range::from_dynamic(&value).unwrap(), original);
}

#[test]
fn range_aceita_uma_selection() {
    let selection = Selection::new(position(5.0, 5.0), position(2.0, 1.0));
    let value = selection.to_dynamic();
    assert_eq!(Range::from_dynamic(&value).unwrap(), range(2.0, 1.0, 5.0, 5.0));
}

#[test]
fn selection_ida_e_volta_preserva_a_direcao() {
    let original = Selection::new(position(5.0, 5.0), position(2.0, 1.0));
    let restored = Selection::from_dynamic(&original.to_dynamic()).unwrap();
    assert_eq!(restored, original);
    assert!(restored.is_reversed());
    assert_eq!(restored.anchor(), position(5.0, 5.0));
    assert_eq!(restored.active(), position(2.0, 1.0));
}

#[test]
fn location_ida_e_volta() {
    let original = Location::new(Some(uri("file:///a/b.txt")), Some(range(0.0, 1.0, 2.0, 3.0).into()));
    let value = original.to_dynamic();
    assert!(is_instance_of(&value.get_str("uri"), "Uri"));
    assert_eq!(Location::from_dynamic(&value).unwrap(), original);
}

#[test]
fn location_sem_uri_e_sem_intervalo_omite_as_propriedades() {
    let original = Location::new(None, None);
    let value = original.to_dynamic();
    assert!(!value.has_property(&"uri".into()));
    assert!(!value.has_property(&"range".into()));
    assert_eq!(Location::from_dynamic(&value).unwrap(), original);
}

#[test]
fn text_edit_ida_e_volta() {
    let original = TextEdit::replace(range(0.0, 0.0, 0.0, 3.0), "novo");
    let value = original.to_dynamic();
    assert_eq!(value.get_str("newText"), Dynamic::string("novo"));
    assert!(!value.has_property(&"newEol".into()));
    assert_eq!(TextEdit::from_dynamic(&value).unwrap(), original);
}

#[test]
fn text_edit_de_fim_de_linha_guarda_o_new_eol() {
    let original = TextEdit::set_end_of_line(EndOfLine::CRLF);
    let value = original.to_dynamic();
    assert_eq!(value.get_str("newEol"), Dynamic::Number(2.0));
    let restored = TextEdit::from_dynamic(&value).unwrap();
    assert_eq!(restored.new_eol(), Some(EndOfLine::CRLF));
    assert_eq!(restored, original);
}

#[test]
fn text_edit_com_fim_de_linha_invalido_falha() {
    let mut value = TextEdit::insert(position(0.0, 0.0), "x").to_dynamic();
    if let Dynamic::Object(object) = &mut value {
        object.set(&"newEol".into(), Dynamic::Number(9.0));
    }
    assert_eq!(TextEdit::from_dynamic(&value).unwrap_err().name, "TypeError");
}

#[test]
fn uri_ida_e_volta() {
    for text in ["file:///home/user/a.txt", "https://exemplo.com:8080/p/q?x=1&y=2#frag", "untitled:Untitled-1", "vscode-notebook-cell://host/a.ipynb#W0sZmlsZQ%3D%3D"] {
        let original = uri(text);
        let value = original.to_dynamic();
        assert_eq!(class_of(&value), Some("Uri"));
        assert_eq!(value.get_str("$mid"), Dynamic::Number(1.0));
        assert_eq!(value.get_str("scheme"), Dynamic::String(original.scheme().clone()));
        assert_eq!(URI::from_dynamic(&value).unwrap(), original);
    }
}

#[test]
fn uri_de_componentes_ausentes_le_como_vazios() {
    let value = tag(Dynamic::object([("$mid", Dynamic::Number(1.0)), ("scheme", Dynamic::string("file")), ("path", Dynamic::string("/x"))]), "Uri");
    let restored = URI::from_dynamic(&value).unwrap();
    assert_eq!(restored.scheme(), &JsString::from("file"));
    assert!(restored.authority().is_empty());
    assert_eq!(restored.path(), &JsString::from("/x"));
}

#[test]
fn uri_de_arquivo_posix_ida_e_volta() {
    let original = URI::file("/tmp/a b/c.rs", Platform::Posix);
    assert_eq!(URI::from_dynamic(&original.to_dynamic()).unwrap(), original);
}

#[test]
fn theme_color_ida_e_volta() {
    let original = ThemeColor::new("editor.foreground");
    let value = original.to_dynamic();
    assert_eq!(class_of(&value), Some("ThemeColor"));
    assert_eq!(ThemeColor::from_dynamic(&value).unwrap(), original);
}

#[test]
fn theme_icon_ida_e_volta_com_e_sem_cor() {
    let plain = ThemeIcon::from_id("globe");
    assert!(!plain.to_dynamic().has_property(&"color".into()));
    assert_eq!(ThemeIcon::from_dynamic(&plain.to_dynamic()).unwrap(), plain);

    let colored = ThemeIcon { id: "error".to_owned(), color: Some(ThemeColor::new("errorForeground")) };
    let value = colored.to_dynamic();
    assert!(is_instance_of(&value.get_str("color"), "ThemeColor"));
    assert_eq!(ThemeIcon::from_dynamic(&value).unwrap(), colored);
}

#[test]
fn markdown_string_ida_e_volta() {
    let mut original = MarkdownString::new(Some("**oi** $(zap)"), true);
    original.support_html = Some(true);
    original.support_alert_syntax = Some(false);
    original.is_trusted = Some(IsTrusted::Options(MarkdownStringTrustedOptions { enabled_commands: vec!["a.b".to_owned(), "c.d".to_owned()] }));
    original.base_uri = Some(uri("file:///base/"));

    let value = original.to_dynamic();
    assert_eq!(class_of(&value), Some("MarkdownString"));
    let restored = MarkdownString::from_dynamic(&value).unwrap();
    assert_eq!(restored.value, original.value);
    assert_eq!(restored.is_trusted, original.is_trusted);
    assert_eq!(restored.support_theme_icons, Some(true));
    assert_eq!(restored.support_html, Some(true));
    assert_eq!(restored.support_alert_syntax, Some(false));
    assert_eq!(restored.base_uri, original.base_uri);
}

#[test]
fn markdown_string_confiavel_como_booleano() {
    let mut original = MarkdownString::new(Some("texto"), false);
    original.is_trusted = Some(IsTrusted::Bool(true));
    let value = original.to_dynamic();
    assert_eq!(value.get_str("isTrusted"), Dynamic::Bool(true));
    assert_eq!(MarkdownString::from_dynamic(&value).unwrap().is_trusted, Some(IsTrusted::Bool(true)));
}

#[test]
fn markdown_string_sem_value_texto_falha() {
    let value = tag(Dynamic::object([("value", Dynamic::Number(1.0))]), "MarkdownString");
    assert_eq!(MarkdownString::from_dynamic(&value).unwrap_err().name, "TypeError");
}

#[test]
fn diagnostic_ida_e_volta_completo() {
    let mut original = Diagnostic::new(range(1.0, 0.0, 1.0, 5.0), "variável não usada", Some(DiagnosticSeverity::Warning)).unwrap();
    original.source = Some("ts".to_owned());
    original.code = Some(DiagnosticCode::Number(6133.0));
    original.tags = Some(vec![DiagnosticTag::Unnecessary]);
    original.related_information = Some(vec![DiagnosticRelatedInformation::new(
        Location::new(Some(uri("file:///a.ts")), Some(range(0.0, 0.0, 0.0, 1.0).into())),
        "declarada aqui",
    )]);

    let value = original.to_dynamic();
    assert_eq!(value.get_str("severity"), Dynamic::Number(1.0));
    assert!(is_instance_of(&value.get_str("range"), "Range"));
    assert_eq!(Diagnostic::from_dynamic(&value).unwrap(), original);
}

#[test]
fn diagnostic_minimo_omite_o_opcional_e_codigo_de_texto_volta() {
    let mut original = Diagnostic::new(range(0.0, 0.0, 0.0, 1.0), "erro", None).unwrap();
    let value = original.to_dynamic();
    for key in ["source", "code", "relatedInformation", "tags"] {
        assert!(!value.has_property(&key.into()), "{key} deveria estar ausente");
    }
    assert_eq!(Diagnostic::from_dynamic(&value).unwrap(), original);

    original.code = Some(DiagnosticCode::Text("E0001".to_owned()));
    assert_eq!(Diagnostic::from_dynamic(&original.to_dynamic()).unwrap(), original);
}

#[test]
fn diagnostic_de_severidade_invalida_falha() {
    let mut value = Diagnostic::new(range(0.0, 0.0, 0.0, 1.0), "erro", None).unwrap().to_dynamic();
    if let Dynamic::Object(object) = &mut value {
        object.set(&"severity".into(), Dynamic::Number(7.0));
    }
    assert_eq!(Diagnostic::from_dynamic(&value).unwrap_err().name, "TypeError");
}

#[test]
fn diagnostic_related_information_ida_e_volta() {
    let original = DiagnosticRelatedInformation::new(Location::new(Some(uri("file:///x")), Some(range(0.0, 0.0, 1.0, 0.0).into())), "veja");
    let restored = DiagnosticRelatedInformation::from_dynamic(&original.to_dynamic()).unwrap();
    assert_eq!(restored, original);
}

#[test]
fn snippet_string_ida_e_volta_o_texto() {
    let mut original = SnippetString::new(None);
    original.append_text("a$b").append_tabstop(None).append_placeholder(je_workbench::api::common::ext_host_types::SnippetContent::Text("x"), None);
    let restored = SnippetString::from_dynamic(&original.to_dynamic()).unwrap();
    assert_eq!(restored.value, original.value);
}

#[test]
fn color_ida_e_volta() {
    let value = Color::new(0.1, 0.2, 0.3, 1.0).to_dynamic();
    let restored = Color::from_dynamic(&value).unwrap();
    assert_eq!((restored.red, restored.green, restored.blue, restored.alpha), (0.1, 0.2, 0.3, 1.0));
}

#[test]
fn notebook_range_ida_e_volta() {
    let original = NotebookRange::new(2.0, 5.0).unwrap();
    let value = original.to_dynamic();
    assert_eq!(class_of(&value), Some("NotebookRange"));
    let restored = NotebookRange::from_dynamic(&value).unwrap();
    assert_eq!((restored.start(), restored.end()), (2.0, 5.0));
}

#[test]
fn notebook_range_negativo_falha_como_o_construtor() {
    let value = tag(Dynamic::object([("start", Dynamic::Number(-1.0)), ("end", Dynamic::Number(2.0))]), "NotebookRange");
    assert_eq!(NotebookRange::from_dynamic(&value).unwrap_err().message, "Illegal argument: start must be positive");
}

#[test]
fn itens_de_hierarquia_ida_e_volta_com_identificadores_e_etiquetas() {
    let mut call = CallHierarchyItem::new(SymbolKind::Function, "f", "detalhe", uri("file:///a.rs"), range(0.0, 0.0, 2.0, 0.0), range(0.0, 0.0, 0.0, 1.0));
    call.session_id = Some("sessão".to_owned());
    call.item_id = Some("item".to_owned());
    call.tags = Some(vec![SymbolTag::Deprecated]);
    let value = call.to_dynamic();
    assert_eq!(class_of(&value), Some("CallHierarchyItem"));
    assert_eq!(value.get_str("kind"), Dynamic::Number(11.0));
    assert_eq!(CallHierarchyItem::from_dynamic(&value).unwrap(), call);

    let mut kind = TypeHierarchyItem::new(SymbolKind::Class, "t", "", uri("file:///b.rs"), range(0.0, 0.0, 1.0, 0.0), range(0.0, 0.0, 0.0, 1.0));
    kind.detail = None;
    let value = kind.to_dynamic();
    assert_eq!(class_of(&value), Some("TypeHierarchyItem"));
    assert!(!value.has_property(&"detail".into()));
    assert_eq!(TypeHierarchyItem::from_dynamic(&value).unwrap(), kind);
    assert_eq!(CallHierarchyItem::from_dynamic(&value).unwrap_err().name, "TypeError");
}

#[test]
fn command_e_objeto_literal_sem_classe() {
    let mut original = Command::new("Título", "ext.fazer");
    original.tooltip = Some("dica".to_owned());
    original.arguments = Some(vec![Dynamic::Number(1.0), position(0.0, 1.0).to_dynamic()]);
    let value = original.to_dynamic();
    assert_eq!(class_of(&value), None);
    assert_eq!(value.get_str("command"), Dynamic::string("ext.fazer"));
    let restored = Command::from_dynamic(&value).unwrap();
    assert_eq!(restored.title, "Título");
    assert_eq!(restored.command, "ext.fazer");
    assert_eq!(restored.tooltip.as_deref(), Some("dica"));
    let arguments = restored.arguments.expect("argumentos");
    assert_eq!(arguments.len(), 2);
    assert_eq!(arguments[0], Dynamic::Number(1.0));
    assert!(is_instance_of(&arguments[1], "Position"));
}

#[test]
fn command_sem_titulo_le_como_vazio_e_sem_comando_falha() {
    let value = Dynamic::object([("command", Dynamic::string("x"))]);
    assert_eq!(Command::from_dynamic(&value).unwrap().title, "");
    assert_eq!(Command::from_dynamic(&Dynamic::object([("title", Dynamic::string("t"))])).unwrap_err().name, "TypeError");
    assert_eq!(Command::from_dynamic(&Dynamic::Undefined).unwrap_err().name, "TypeError");
}

#[test]
fn vs_buffer_e_array_buffer_cruzam_como_bytes() {
    let buffer = VSBuffer::wrap(vec![1, 2, 3]);
    let value = vs_buffer_to_dynamic(&buffer);
    assert_eq!(class_of(&value), Some("VSBuffer"));
    assert_eq!(vs_buffer_from_dynamic(&value).unwrap().to_vec(), vec![1, 2, 3]);
    assert_eq!(bytes_of(&value), Some(vec![1, 2, 3]));

    let array_buffer = array_buffer_to_dynamic(vec![9, 8]);
    assert_eq!(class_of(&array_buffer), Some("ArrayBuffer"));
    assert_eq!(bytes_of(&array_buffer), Some(vec![9, 8]));

    assert_eq!(bytes_of(&Dynamic::typed_array(vec![5])), Some(vec![5]));
    assert_eq!(bytes_of(&Dynamic::Number(1.0)), None);
    assert_eq!(bytes_of(&position(0.0, 0.0).to_dynamic()), None);
    assert_eq!(vs_buffer_from_dynamic(&array_buffer).unwrap_err().name, "TypeError");
}

#[test]
fn revive_troca_o_mid_de_uri_por_um_uri_marcado() {
    let marshalled = Dynamic::object([
        ("$mid", Dynamic::Number(1.0)),
        ("scheme", Dynamic::string("file")),
        ("path", Dynamic::string("/a/b")),
    ]);
    let revived = revive(&marshalled);
    assert_eq!(class_of(&revived), Some("Uri"));
    assert_eq!(URI::from_dynamic(&revived).unwrap(), URI::file("/a/b", Platform::Posix));
}

#[test]
fn revive_desce_por_objetos_e_arrays_no_proprio_valor() {
    let inner = Dynamic::object([("$mid", Dynamic::Number(1.0)), ("scheme", Dynamic::string("https")), ("authority", Dynamic::string("a.b"))]);
    let list = Dynamic::array([inner.clone(), Dynamic::Number(7.0)]);
    let root = Dynamic::object([("lista", list.clone()), ("texto", Dynamic::string("x")), ("zero", Dynamic::Number(0.0))]);

    let revived = revive(&root);
    assert!(revived.strict_equals(&root), "o revive devolve o próprio objeto");
    let first = list.get_str("0");
    assert_eq!(class_of(&first), Some("Uri"));
    assert_eq!(list.get_str("1"), Dynamic::Number(7.0));
    assert_eq!(root.get_str("texto"), Dynamic::string("x"));
    assert_eq!(root.get_str("zero"), Dynamic::Number(0.0));
}

#[test]
fn revive_de_regexp_e_date() {
    let regexp = revive(&Dynamic::object([("$mid", Dynamic::Number(2.0)), ("source", Dynamic::string("a+b")), ("flags", Dynamic::string("gi"))]));
    match &regexp {
        Dynamic::RegExp(value) => {
            assert_eq!(value.source(), &JsString::from("a+b"));
            assert_eq!(value.flags(), &JsString::from("gi"));
        }
        other => panic!("esperava um RegExp, veio {other:?}"),
    }

    let from_number = revive(&Dynamic::object([("$mid", Dynamic::Number(17.0)), ("source", Dynamic::Number(86_400_000.0))]));
    assert!(matches!(from_number, Dynamic::Date(ref date) if date.time_value() == 86_400_000.0));
    let from_text = revive(&Dynamic::object([("$mid", Dynamic::Number(17.0)), ("source", Dynamic::string("1970-01-02T00:00:00.000Z"))]));
    assert!(matches!(from_text, Dynamic::Date(ref date) if date.time_value() == 86_400_000.0));
    let invalid = revive(&Dynamic::object([("$mid", Dynamic::Number(17.0)), ("source", Dynamic::string("não é data"))]));
    assert!(matches!(invalid, Dynamic::Date(ref date) if date.time_value().is_nan()));
}

#[test]
fn revive_deixa_primitivos_buffers_e_mid_desconhecido_como_estao() {
    for value in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(false), Dynamic::Number(0.0), Dynamic::string(""), Dynamic::Number(5.0), Dynamic::string("a")] {
        assert_eq!(revive(&value), value);
    }
    let bytes = Dynamic::typed_array(vec![1, 2]);
    assert!(revive(&bytes).strict_equals(&bytes));

    let buffer = vs_buffer_to_dynamic(&VSBuffer::wrap(vec![3, 4]));
    assert!(revive(&buffer).strict_equals(&buffer));
    assert_eq!(bytes_of(&buffer), Some(vec![3, 4]));

    let unknown = Dynamic::object([("$mid", Dynamic::Number(999.0)), ("a", Dynamic::Number(1.0))]);
    assert!(revive(&unknown).strict_equals(&unknown));
    assert_eq!(class_of(&unknown), None);
}

#[test]
fn revive_para_de_descer_alem_de_200_niveis() {
    let mut deepest = Dynamic::object([("$mid", Dynamic::Number(1.0)), ("scheme", Dynamic::string("file"))]);
    for _ in 0..205 {
        deepest = Dynamic::array([deepest]);
    }
    let revived = revive(&deepest);
    let mut current = revived;
    for _ in 0..205 {
        current = current.get_str("0");
    }
    // além do limite o objeto `$mid` não é revivido
    assert_eq!(class_of(&current), None);
}

#[test]
fn revive_de_uri_com_cache_usa_o_estado_do_to_json() {
    let original = URI::file("/x/y", Platform::Posix);
    let state = original.to_json();
    let marshalled = Dynamic::object([
        ("$mid", Dynamic::Number(1.0)),
        ("external", state.external.clone().map_or(Dynamic::Undefined, Dynamic::String)),
        ("path", Dynamic::string("/x/y")),
        ("scheme", Dynamic::string("file")),
    ]);
    let revived = revive(&marshalled);
    assert_eq!(URI::from_dynamic(&revived).unwrap(), original);
}
