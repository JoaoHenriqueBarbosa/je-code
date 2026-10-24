//! Casos próprios de `ext_host_type_converters/`: os conversores de geometria, marcadores, markdown, edições de
//! texto e símbolos. Não há suíte equivalente no original (o `extHostTypeConverters.ts` é testado de forma
//! indireta pelas suítes dos `extHost*.ts`), então os casos são nossos: cada um confere a ida, a volta e o que o
//! original faz de não óbvio (base 0 contra base 1, `code` falso descartado, `'!!MISSING: name!!'`).

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::html_content::MarkdownStringTrustedOptions;
use je_base::uri::URI;
use je_editor::model::EndOfLineSequence;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_editor::selection::Selection as EditorSelection;
use je_platform::markers::markers::{MarkerCode, MarkerSeverity, MarkerTag};
use je_workbench::api::common::ext_host_type_converters as converters;
use je_workbench::api::common::ext_host_type_converters::geometry::LocationDto;
use je_workbench::api::common::ext_host_types as types;

fn position(line: f64, character: f64) -> types::Position {
    types::Position::new(line, character).expect("posição válida")
}

fn range(start_line: f64, start_character: f64, end_line: f64, end_character: f64) -> types::Range {
    types::Range::from_coordinates(start_line, start_character, end_line, end_character).expect("intervalo válido")
}

fn uri(text: &str) -> URI {
    URI::parse(text, true)
}

#[test]
fn position_soma_um_na_ida_e_tira_na_volta() {
    let editor = converters::Position::from(&position(3.0, 4.0));
    assert_eq!(editor, EditorPosition::new(4, 5));
    assert_eq!(converters::Position::to(&editor).unwrap(), position(3.0, 4.0));
}

#[test]
fn position_de_dto_invalido_falha_como_o_construtor() {
    let error = converters::Position::to(&EditorPosition::new(0, 1)).unwrap_err();
    assert_eq!(error.message, "Illegal argument: line must be non-negative");
}

#[test]
fn range_from_nao_ordena_e_to_ordena() {
    let editor = converters::Range::from(&range(1.0, 2.0, 3.0, 4.0));
    assert_eq!(editor, EditorRange { start_line_number: 2, start_column: 3, end_line_number: 4, end_column: 5 });
    assert_eq!(converters::Range::to(&editor).unwrap(), range(1.0, 2.0, 3.0, 4.0));

    let inverted = EditorRange { start_line_number: 4, start_column: 5, end_line_number: 2, end_column: 3 };
    assert_eq!(converters::Range::to(&inverted).unwrap(), range(1.0, 2.0, 3.0, 4.0));
}

#[test]
fn range_opcional_passa_none() {
    assert_eq!(converters::Range::from_optional(None), None);
    assert_eq!(converters::Range::to_optional(None).unwrap(), None);
    assert_eq!(converters::Range::from_optional(Some(&range(0.0, 0.0, 0.0, 1.0))).map(|r| r.end_column), Some(2));
}

#[test]
fn dto_de_range_e_posicao_ida_e_volta_e_rejeita_o_que_nao_tem_a_forma() {
    let editor = EditorRange { start_line_number: 1, start_column: 2, end_line_number: 3, end_column: 4 };
    assert_eq!(converters::Range::from_dto(&converters::Range::to_dto(&editor)), Some(editor));
    let editor_position = EditorPosition::new(7, 8);
    assert_eq!(converters::Position::from_dto(&converters::Position::to_dto(&editor_position)), Some(editor_position));

    for value in [Dynamic::Undefined, Dynamic::Null, Dynamic::Number(1.0), Dynamic::string("x"), Dynamic::array([]), Dynamic::object::<&str>([])] {
        assert_eq!(converters::Range::from_dto(&value), None);
        assert_eq!(converters::Position::from_dto(&value), None);
    }
    let text_in_field = Dynamic::object([("lineNumber", Dynamic::string("1")), ("column", Dynamic::Number(1.0))]);
    assert_eq!(converters::Position::from_dto(&text_in_field), None);
}

#[test]
fn selection_ida_e_volta_preserva_a_direcao() {
    let original = types::Selection::new(position(5.0, 5.0), position(2.0, 1.0));
    let editor = converters::Selection::from(&original);
    assert_eq!(editor, EditorSelection::new(6, 6, 3, 2));
    let restored = converters::Selection::to(&editor).unwrap();
    assert_eq!(restored, original);
    assert!(restored.is_reversed());
    assert_eq!(converters::Selection::to_dto(&editor).get_str("positionLineNumber"), Dynamic::Number(3.0));
}

#[test]
fn location_com_e_sem_intervalo() {
    let complete = types::Location::new(Some(uri("file:///a.rs")), Some(range(0.0, 1.0, 2.0, 3.0).into()));
    let dto = converters::Location::from(&complete);
    assert_eq!(dto.uri, Some(uri("file:///a.rs")));
    assert_eq!(converters::Location::to(&dto).unwrap(), complete);

    let empty = converters::Location::from(&types::Location::new(None, None));
    assert_eq!(empty, LocationDto { uri: None, range: None });
    assert_eq!(converters::Location::to(&empty).unwrap(), types::Location::new(None, None));
}

#[test]
fn location_de_dto_dynamic_exige_uri_e_intervalo() {
    let dto = converters::Location::from(&types::Location::new(Some(uri("file:///a.rs")), Some(range(0.0, 1.0, 2.0, 3.0).into())));
    let value = converters::Location::to_dto(&dto);
    assert_eq!(converters::Location::from_dto(&value), Some(dto));

    let without_range = Dynamic::object([("uri", value.get_str("uri"))]);
    assert_eq!(converters::Location::from_dto(&without_range), None);
    let without_uri = Dynamic::object([("range", value.get_str("range"))]);
    assert_eq!(converters::Location::from_dto(&without_uri), None);
}

#[test]
fn notebook_range_ida_e_volta() {
    let original = types::NotebookRange::new(2.0, 5.0).unwrap();
    let cell_range = converters::NotebookRange::from(&original);
    assert_eq!((cell_range.start, cell_range.end), (2.0, 5.0));
    let restored = converters::NotebookRange::to(&cell_range).unwrap();
    assert_eq!((restored.start(), restored.end()), (2.0, 5.0));
    assert_eq!(converters::NotebookRange::to_dto(&cell_range).get_str("end"), Dynamic::Number(5.0));
}

#[test]
fn severidade_e_tag_de_diagnostico() {
    let pairs = [
        (types::DiagnosticSeverity::Error, MarkerSeverity::ERROR),
        (types::DiagnosticSeverity::Warning, MarkerSeverity::WARNING),
        (types::DiagnosticSeverity::Information, MarkerSeverity::INFO),
        (types::DiagnosticSeverity::Hint, MarkerSeverity::HINT),
    ];
    for (api, marker) in pairs {
        assert_eq!(converters::DiagnosticSeverity::from(api), marker);
        assert_eq!(converters::DiagnosticSeverity::to(marker), api);
    }
    assert_eq!(converters::DiagnosticSeverity::to(MarkerSeverity(99)), types::DiagnosticSeverity::Error);

    assert_eq!(converters::DiagnosticTag::from(types::DiagnosticTag::Unnecessary), Some(MarkerTag::Unnecessary));
    assert_eq!(converters::DiagnosticTag::to(MarkerTag::Deprecated), Some(types::DiagnosticTag::Deprecated));
}

#[test]
fn diagnostic_ida_e_volta_com_codigo_como_texto() {
    let mut original = types::Diagnostic::new(range(1.0, 0.0, 1.0, 5.0), "variável não usada", Some(types::DiagnosticSeverity::Warning)).unwrap();
    original.source = Some("ts".to_owned());
    original.code = Some(types::DiagnosticCode::Number(6133.0));
    original.tags = Some(vec![types::DiagnosticTag::Unnecessary]);
    original.related_information = Some(vec![types::DiagnosticRelatedInformation::new(
        types::Location::new(Some(uri("file:///a.ts")), Some(range(0.0, 0.0, 0.0, 1.0).into())),
        "declarada aqui",
    )]);

    let marker = converters::Diagnostic::from(&original).unwrap();
    assert_eq!((marker.start_line_number, marker.start_column, marker.end_line_number, marker.end_column), (2, 1, 2, 6));
    assert_eq!(marker.severity, MarkerSeverity::WARNING);
    assert_eq!(marker.code, Some(MarkerCode::Text("6133".to_owned())));
    assert_eq!(marker.tags, Some(vec![MarkerTag::Unnecessary]));
    let related = marker.related_information.as_ref().unwrap();
    assert_eq!((related[0].start_line_number, related[0].start_column, related[0].message.as_str()), (1, 1, "declarada aqui"));

    let restored = converters::Diagnostic::to(&marker).unwrap();
    original.code = Some(types::DiagnosticCode::Text("6133".to_owned()));
    assert_eq!(restored, original);
}

#[test]
fn diagnostic_descarta_codigo_falso() {
    let mut diagnostic = types::Diagnostic::new(range(0.0, 0.0, 0.0, 1.0), "erro", None).unwrap();
    for code in [types::DiagnosticCode::Number(0.0), types::DiagnosticCode::Text(String::new())] {
        diagnostic.code = Some(code);
        assert_eq!(converters::Diagnostic::from(&diagnostic).unwrap().code, None);
    }
    diagnostic.code = Some(types::DiagnosticCode::Text("E1".to_owned()));
    assert_eq!(converters::Diagnostic::from(&diagnostic).unwrap().code, Some(MarkerCode::Text("E1".to_owned())));
}

#[test]
fn informacao_relacionada_sem_uri_ou_intervalo_falha() {
    let without_uri = types::DiagnosticRelatedInformation::new(types::Location::new(None, Some(range(0.0, 0.0, 0.0, 1.0).into())), "x");
    assert_eq!(converters::DiagnosticRelatedInformation::from(&without_uri).unwrap_err().name, "TypeError");
    let without_range = types::DiagnosticRelatedInformation::new(types::Location::new(Some(uri("file:///a")), None), "x");
    assert_eq!(converters::DiagnosticRelatedInformation::from(&without_range).unwrap_err().name, "TypeError");
}

#[test]
fn markdown_string_de_texto_bloco_de_codigo_e_markdown() {
    use je_workbench::api::common::ext_host_types::MarkedString;

    assert_eq!(converters::MarkdownString::from(&MarkedString::Text("**oi**".to_owned())).value, JsString::from("**oi**"));
    let code = converters::MarkdownString::from(&MarkedString::Code { language: "rs".to_owned(), value: "fn main() {}".to_owned() });
    assert_eq!(code.value, JsString::from("```rs\nfn main() {}\n```\n"));

    let mut markdown = types::MarkdownString::new(Some("texto $(zap)"), true);
    markdown.support_html = Some(true);
    markdown.base_uri = Some(uri("file:///base/"));
    markdown.is_trusted = Some(je_base::html_content::IsTrusted::Options(MarkdownStringTrustedOptions { enabled_commands: vec!["a.b".to_owned()] }));
    let dto = converters::MarkdownString::from(&MarkedString::Markdown(markdown.clone()));
    assert_eq!(dto.support_theme_icons, Some(true));
    assert_eq!(dto.support_html, Some(true));
    assert!(dto.uris.is_some(), "o original sempre define `uris` (um objeto, vazio quando o texto não tem links)");

    let restored = converters::MarkdownString::to(&dto);
    assert_eq!(restored.value, markdown.value);
    assert_eq!(restored.is_trusted, markdown.is_trusted);
    assert_eq!(restored.base_uri, markdown.base_uri);
}

#[test]
fn markdown_string_from_strict_trata_o_falso_como_ausente() {
    assert!(converters::MarkdownString::from_strict(None).is_none());
    assert!(converters::MarkdownString::from_strict(Some(&types::Documentation::Text(String::new()))).is_none());
    assert!(converters::MarkdownString::from_strict(Some(&types::Documentation::Text("a".to_owned()))).is_some());
    assert!(converters::MarkdownString::from_strict(Some(&types::Documentation::Markdown(types::MarkdownString::new(Some(""), false)))).is_some());
}

#[test]
fn text_edit_ida_e_volta_com_fim_de_linha() {
    let plain = types::TextEdit::replace(range(0.0, 0.0, 0.0, 3.0), "novo");
    let dto = converters::TextEdit::from(&plain);
    assert_eq!((dto.text.as_str(), dto.eol, dto.insert_as_snippet), ("novo", None, None));
    assert_eq!(converters::TextEdit::to(&dto).unwrap(), plain);

    let eol = types::TextEdit::set_end_of_line(types::EndOfLine::CRLF);
    let dto = converters::TextEdit::from(&eol);
    assert_eq!(dto.eol, Some(EndOfLineSequence::CrLf));
    assert_eq!(converters::TextEdit::to(&dto).unwrap().new_eol(), Some(types::EndOfLine::CRLF));
    assert_eq!(converters::EndOfLine::to(converters::EndOfLine::from(types::EndOfLine::LF)), types::EndOfLine::LF);
}

#[test]
fn symbol_kind_e_total_nos_dois_sentidos() {
    for value in 0..=25 {
        let api = types::SymbolKind::from_value(value).expect("membro da API");
        assert_eq!(converters::SymbolKind::to(converters::SymbolKind::from(api)), api);
        assert_eq!(converters::SymbolKind::from(api) as i32, value);
    }
    assert_eq!(converters::SymbolTag::to(converters::SymbolTag::from(types::SymbolTag::Deprecated)), types::SymbolTag::Deprecated);
}

#[test]
fn document_symbol_ida_e_volta_com_filhos() {
    let mut child = types::DocumentSymbol::new("filho", "", types::SymbolKind::Field, range(1.0, 0.0, 1.0, 5.0), range(1.0, 0.0, 1.0, 2.0)).unwrap();
    child.tags = Some(vec![types::SymbolTag::Deprecated]);
    let mut parent = types::DocumentSymbol::new("pai", "detalhe", types::SymbolKind::Class, range(0.0, 0.0, 9.0, 0.0), range(0.0, 0.0, 0.0, 3.0)).unwrap();
    parent.children = vec![child];

    let dto = converters::DocumentSymbol::from(&parent);
    assert_eq!(dto.detail, "detalhe");
    assert_eq!(dto.children.as_ref().map(Vec::len), Some(1));
    assert_eq!(dto.children.as_ref().unwrap()[0].tags.len(), 1);
    assert_eq!(converters::DocumentSymbol::to(&dto).unwrap(), parent);
}

#[test]
fn document_symbol_sem_nome_ganha_o_marcador() {
    let mut symbol = types::DocumentSymbol::new("x", "", types::SymbolKind::Class, range(0.0, 0.0, 1.0, 0.0), range(0.0, 0.0, 0.0, 1.0)).unwrap();
    symbol.name = String::new();
    assert_eq!(converters::DocumentSymbol::from(&symbol).name, "!!MISSING: name!!");
}

#[test]
fn document_highlight_ida_e_volta() {
    let original = types::DocumentHighlight::new(range(0.0, 0.0, 0.0, 4.0), Some(types::DocumentHighlightKind::Write));
    let dto = converters::DocumentHighlight::from(&original);
    assert_eq!(converters::DocumentHighlight::to(&dto).unwrap(), original);
}

#[test]
fn hierarquia_de_chamada_exige_os_identificadores() {
    let item = types::CallHierarchyItem::new(types::SymbolKind::Function, "f", "d", uri("file:///a.rs"), range(0.0, 0.0, 1.0, 0.0), range(0.0, 0.0, 0.0, 1.0));
    assert_eq!(converters::CallHierarchyItem::from(&item, None, None).unwrap_err().message, "invalid item");
    assert_eq!(converters::CallHierarchyItem::from(&item, Some("s"), None).unwrap_err().message, "invalid item");

    let dto = converters::CallHierarchyItem::from(&item, Some("sessão"), Some("item")).unwrap();
    assert_eq!((dto.session_id.as_str(), dto.item_id.as_str(), dto.name.as_str()), ("sessão", "item", "f"));
    assert_eq!(dto.detail.as_deref(), Some("d"));

    let restored = converters::CallHierarchyItem::to(&dto).unwrap();
    assert_eq!((restored.session_id.as_deref(), restored.item_id.as_deref()), (Some("sessão"), Some("item")));
    // os identificadores do próprio item valem quando os argumentos faltam
    assert!(converters::CallHierarchyItem::from(&restored, None, None).is_ok());
}

#[test]
fn hierarquia_de_tipos_troca_detalhe_ausente_por_vazio_e_a_de_chamada_repassa() {
    let mut call = types::CallHierarchyItem::new(types::SymbolKind::Class, "c", "", uri("file:///a"), range(0.0, 0.0, 1.0, 0.0), range(0.0, 0.0, 0.0, 1.0));
    call.detail = None;
    assert_eq!(converters::CallHierarchyItem::from(&call, Some("s"), Some("i")).unwrap().detail, None);

    let mut kind = types::TypeHierarchyItem::new(types::SymbolKind::Class, "t", "", uri("file:///a"), range(0.0, 0.0, 1.0, 0.0), range(0.0, 0.0, 0.0, 1.0));
    kind.detail = None;
    assert_eq!(converters::TypeHierarchyItem::from(&kind, Some("s"), Some("i")).unwrap().detail, Some(String::new()));
}

#[test]
fn hierarquia_dto_como_objeto_e_chamadas() {
    let item = types::CallHierarchyItem::new(types::SymbolKind::Function, "f", "d", uri("file:///a.rs"), range(0.0, 0.0, 1.0, 0.0), range(0.0, 0.0, 0.0, 1.0));
    let dto = converters::CallHierarchyItem::from(&item, Some("s"), Some("i")).unwrap();
    let value = dto.to_dynamic();
    assert_eq!(value.get_str("_sessionId"), Dynamic::string("s"));
    assert_eq!(value.get_str("kind"), Dynamic::Number(11.0));
    assert_eq!(value.get_str("range").get_str("endLineNumber"), Dynamic::Number(2.0));

    let incoming = converters::IncomingCallDto { from: dto.clone(), from_ranges: vec![dto.range] };
    let call = converters::CallHierarchyIncomingCall::to(&incoming).unwrap();
    assert_eq!(call.from_ranges.len(), 1);
    let outgoing = converters::OutgoingCallDto { to: dto.clone(), from_ranges: vec![] };
    assert_eq!(converters::CallHierarchyOutgoingCall::to(&outgoing).unwrap().to.name, "f");
}

#[test]
fn view_column_converte_as_colunas() {
    assert_eq!(converters::ViewColumn::from(Some(1.0)), 0.0);
    assert_eq!(converters::ViewColumn::from(Some(types::ViewColumn::BESIDE)), converters::SIDE_GROUP);
    assert_eq!(converters::ViewColumn::from(None), converters::ACTIVE_GROUP);
    assert_eq!(converters::ViewColumn::to(0.0), 1.0);
}
