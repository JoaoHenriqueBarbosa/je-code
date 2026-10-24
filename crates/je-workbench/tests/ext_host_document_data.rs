//! Porte de `src/vs/workbench/api/test/browser/extHostDocumentData.test.ts` (suítes `ExtHostDocumentData` e
//! `ExtHostDocumentData updates line mapping`), caso a caso e na mesma ordem. Cada suíte é um módulo.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` não tem o que vigiar: o `ExtHostDocumentData` não é um
//!   `Disposable` rastreado, e nenhum caso cria um `DisposableStore`.
//! - `new ExtHostDocumentData(undefined!, ...)`: o `proxy` nunca é chamado nesses casos, então vale o
//!   [`UnusedSaveDelegate`], que falha se for. O `mock<MainThreadDocumentsShape>()` do caso `save, when
//!   disposed` é o [`SaveRecorder`], com o `$trySaveDocument` do original.
//! - `readonly-ness`: o original confere que escrever `uri`, `fileName`, `isDirty`, `isUntitled`,
//!   `languageId` e `lineCount` lança. Aqui não há setter, então a escrita é erro de compilação e o caso
//!   confere que os valores seguem os do documento.
//! - Os eventos do original trazem `eol: undefined`, `versionId: undefined`, `rangeOffset: undefined`
//!   e `rangeLength: undefined`. `eol` sem valor (que o espelho ignora) é o fim de linha do próprio documento
//!   ([`TestEvent::to_event`]); os demais campos não são lidos pelo espelho e valem zero.
//! - `Number.MIN_VALUE` é `f64::from_bits(1)` (o menor subnormal) e `Number.MAX_VALUE` é `f64::MAX`.
//! - `data.onEvents(e)` e `data.getText()` (herdados de `MirrorTextModel`) são `mirror_mut().on_events(&e)`
//!   e `document().get_text(None)`, que devolve o mesmo texto do espelho.
//! - O `perfData._$_$_expensive` é lido de `ref/vscode` em tempo de execução (o literal tem mais de um megabyte):
//!   [`read_perf_data`] extrai o texto entre aspas do `export const` e resolve os escapes.
//! - `createChangeEvent(range, text, eol?)` é `TestEvent::change(range, text, eol)`.
//! - `assert.throws(...)` vira `is_err()` do `Result`; `assert.strictEqual(range, undefined)` vira `None`.

use std::cell::RefCell;
use std::rc::Rc;

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::run_with_faked_timers;
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_base::js_regexp::JsRegExp;
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::model::Eol;
use je_editor::range::Range as EditorRange;
use je_editor::text_model_events::{ModelContentChange, ModelContentChangedEvent};
use je_editor::word_helper::{GetWordAtTextConfig, set_default_get_word_at_text_config};
use je_workbench::api::common::ext_host_document_data::{ExtHostDocumentData, IExtHostDocumentSaveDelegate, LineOrPosition};
use je_workbench::api::common::ext_host_types::{Position, Range};

const PERF_DATA_FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ref/vscode/src/vs/workbench/api/test/browser/extHostDocumentData.test.perf-data.ts"
);

fn platform() -> Platform {
    if cfg!(windows) { Platform::Win32 } else { Platform::Posix }
}

/// O `proxy` de `undefined!`: nenhum caso o chama.
struct UnusedSaveDelegate;

impl IExtHostDocumentSaveDelegate for UnusedSaveDelegate {
    fn rpc_try_save_document(&self, _uri: &URI) -> Promise<bool> {
        panic!("o proxy não deveria ser chamado");
    }
}

/// O `new class extends mock<MainThreadDocumentsShape>()` do caso `save, when disposed`.
#[derive(Default)]
struct SaveRecorder {
    saved: RefCell<Option<URI>>,
}

impl IExtHostDocumentSaveDelegate for SaveRecorder {
    fn rpc_try_save_document(&self, uri: &URI) -> Promise<bool> {
        assert!(self.saved.borrow().is_none());
        *self.saved.borrow_mut() = Some(uri.clone());
        Promise::resolved(true)
    }
}

fn js_lines(lines: &[&str]) -> Vec<JsString> {
    lines.iter().map(|line| JsString::from(*line)).collect()
}

/// `new ExtHostDocumentData(undefined!, URI.file(''), lines, eol, 1, 'text', false, 'utf8')`.
fn new_data(lines: Vec<JsString>, eol: Eol) -> ExtHostDocumentData {
    ExtHostDocumentData::new(Rc::new(UnusedSaveDelegate), URI::file("", platform()), lines, eol, 1, "text", false, "utf8")
}

/// O documento do `setup` da primeira suíte.
fn setup() -> ExtHostDocumentData {
    new_data(
        js_lines(&["This is line one", "and this is line number two", "it is followed by #3", "and finished with the fourth."]),
        Eol::Lf,
    )
}

fn p(line: f64, character: f64) -> Position {
    Position::new(line, character).unwrap()
}

fn text_of(text: &JsString) -> String {
    text.to_string_lossy()
}

/// `{ changes: [{ range, rangeOffset: undefined, rangeLength: undefined, text }], eol, versionId: undefined, ... }`.
struct TestEvent {
    changes: Vec<(EditorRange, &'static str)>,
    eol: Option<Eol>,
    version_id: i64,
}

impl TestEvent {
    fn change(range: EditorRange, text: &'static str, eol: Option<Eol>) -> TestEvent {
        TestEvent { changes: vec![(range, text)], eol, version_id: 1 }
    }

    /// O evento do modelo; sem `eol`, o do documento (o `e.eol &&` do espelho ignora o que não existe).
    fn to_event(&self, document_eol: Eol) -> ModelContentChangedEvent {
        ModelContentChangedEvent {
            changes: self
                .changes
                .iter()
                .map(|(range, text)| ModelContentChange { range: *range, range_offset: 0, range_length: 0, text: JsString::from(*text) })
                .collect(),
            eol: self.eol.unwrap_or(document_eol),
            version_id: self.version_id,
            is_undoing: false,
            is_redoing: false,
            is_flush: false,
            is_eol_change: false,
            detailed_reasons: Vec::new(),
            detailed_reasons_change_lengths: Vec::new(),
        }
    }
}

fn apply(data: &ExtHostDocumentData, event: &TestEvent) {
    let eol = data.mirror().eol();
    data.mirror_mut().on_events(&event.to_event(eol));
}

fn reg_exp(pattern: &str, flags: &str) -> Rc<JsRegExp> {
    Rc::new(JsRegExp::from_str(pattern, flags).unwrap())
}

fn assert_range(range: Option<Range>, start_line: f64, start_character: f64, end_line: f64, end_character: f64) {
    let range = range.expect("deveria haver uma palavra");
    assert_eq!(range.start().line(), start_line);
    assert_eq!(range.start().character(), start_character);
    assert_eq!(range.end().line(), end_line);
    assert_eq!(range.end().character(), end_character);
}

/// O texto do `export const _$_$_expensive = '...';` com os escapes do JavaScript resolvidos.
fn read_perf_data() -> JsString {
    let source = std::fs::read_to_string(PERF_DATA_FILE).unwrap_or_else(|error| panic!("não foi possível ler {PERF_DATA_FILE}: {error}"));
    let start = source.find("= '").expect("literal ausente") + "= '".len();
    let end = source.rfind('\'').expect("fim do literal ausente");
    let mut chars = source[start..end].chars();
    let mut units: Vec<u16> = Vec::new();
    let hex = |chars: &mut std::str::Chars<'_>, digits: usize| -> u16 {
        let text: String = chars.by_ref().take(digits).collect();
        u16::from_str_radix(&text, 16).expect("escape hexadecimal inválido")
    };
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buffer = [0u16; 2];
            units.extend_from_slice(c.encode_utf16(&mut buffer));
            continue;
        }
        match chars.next().expect("escape incompleto") {
            'n' => units.push(10),
            't' => units.push(9),
            'r' => units.push(13),
            'b' => units.push(8),
            'f' => units.push(12),
            'v' => units.push(11),
            '0' => units.push(0),
            'x' => units.push(hex(&mut chars, 2)),
            'u' => units.push(hex(&mut chars, 4)),
            '\n' => {}
            other => {
                let mut buffer = [0u16; 2];
                units.extend_from_slice(other.encode_utf16(&mut buffer));
            }
        }
    }
    JsString::from_units(units)
}

mod ext_host_document_data {
    use super::*;

    fn assert_position_at(data: &ExtHostDocumentData, offset: f64, line: f64, character: f64) {
        let position = data.document().position_at(offset);
        assert_eq!(position.line(), line);
        assert_eq!(position.character(), character);
    }

    fn assert_offset_at(data: &ExtHostDocumentData, line: f64, character: f64, offset: f64) {
        let actual = data.document().offset_at(&p(line, character));
        assert_eq!(actual, offset);
    }

    #[test]
    fn readonly_ness() {
        let data = setup();
        let document = data.document();
        assert_eq!(document.uri().scheme(), "file");
        assert_eq!(text_of(&document.file_name()), text_of(&document.uri().fs_path(platform())));
        assert!(!document.is_dirty());
        assert!(!document.is_untitled());
        assert_eq!(document.language_id(), "text");
        assert_eq!(document.line_count(), 4.0);
    }

    #[test]
    fn save_when_disposed() {
        run_with_faked_timers(async {
            let recorder = Rc::new(SaveRecorder::default());
            let proxy: Rc<dyn IExtHostDocumentSaveDelegate> = recorder.clone();
            let data = ExtHostDocumentData::new(proxy, URI::parse("foo:bar", false), Vec::new(), Eol::Lf, 1, "text", true, "utf8");

            data.document().save().await.unwrap();
            assert_eq!(recorder.saved.borrow().as_ref().unwrap().to_string(false), "foo:bar");

            data.dispose();

            let error = data.document().save().await;
            assert!(error.is_err(), "expected failure");
        });
    }

    #[test]
    fn read_when_disposed() {
        let data = setup();
        data.dispose();

        let document = data.document();
        assert_eq!(document.line_count(), 4.0);
        assert_eq!(text_of(document.line_at(LineOrPosition::Line(0.0)).unwrap().text()), "This is line one");
    }

    #[test]
    fn lines() {
        let data = setup();

        assert_eq!(data.document().line_count(), 4.0);

        assert!(data.document().line_at(LineOrPosition::Line(-1.0)).is_err());
        assert!(data.document().line_at(LineOrPosition::Line(data.document().line_count())).is_err());
        assert!(data.document().line_at(LineOrPosition::Line(f64::MAX)).is_err());
        assert!(data.document().line_at(LineOrPosition::Line(f64::from_bits(1))).is_err());
        assert!(data.document().line_at(LineOrPosition::Line(0.8)).is_err());

        let line = data.document().line_at(LineOrPosition::Line(0.0)).unwrap();
        assert_eq!(line.line_number(), 0.0);
        assert_eq!(line.text().len(), 16);
        assert_eq!(text_of(line.text()), "This is line one");
        assert!(!line.is_empty_or_whitespace());
        assert_eq!(line.first_non_whitespace_character_index(), 0.0);

        apply(&data, &TestEvent::change(EditorRange::new(1, 1, 1, 1), "\t ", None));

        // line didn't change
        assert_eq!(text_of(line.text()), "This is line one");
        assert_eq!(line.first_non_whitespace_character_index(), 0.0);

        // fetch line again
        let line = data.document().line_at(LineOrPosition::Line(0.0)).unwrap();
        assert_eq!(text_of(line.text()), "\t This is line one");
        assert_eq!(line.first_non_whitespace_character_index(), 2.0);
    }

    #[test]
    fn line_issue_5704() {
        let data = setup();

        let line = data.document().line_at(LineOrPosition::Line(0.0)).unwrap();
        let (range, range_including_line_break) = (line.range(), line.range_including_line_break());
        assert_eq!(range.end().line(), 0.0);
        assert_eq!(range.end().character(), 16.0);
        assert_eq!(range_including_line_break.end().line(), 1.0);
        assert_eq!(range_including_line_break.end().character(), 0.0);

        let line = data.document().line_at(LineOrPosition::Line(data.document().line_count() - 1.0)).unwrap();
        let (range, range_including_line_break) = (line.range(), line.range_including_line_break());
        assert_eq!(range.end().line(), 3.0);
        assert_eq!(range.end().character(), 29.0);
        assert_eq!(range_including_line_break.end().line(), 3.0);
        assert_eq!(range_including_line_break.end().character(), 29.0);
    }

    #[test]
    fn offset_at() {
        let data = setup();
        assert_offset_at(&data, 0.0, 0.0, 0.0);
        assert_offset_at(&data, 0.0, 1.0, 1.0);
        assert_offset_at(&data, 0.0, 16.0, 16.0);
        assert_offset_at(&data, 1.0, 0.0, 17.0);
        assert_offset_at(&data, 1.0, 3.0, 20.0);
        assert_offset_at(&data, 2.0, 0.0, 45.0);
        assert_offset_at(&data, 4.0, 29.0, 95.0);
        assert_offset_at(&data, 4.0, 30.0, 95.0);
        assert_offset_at(&data, 4.0, f64::MAX, 95.0);
        assert_offset_at(&data, 5.0, 29.0, 95.0);
        assert_offset_at(&data, f64::MAX, 29.0, 95.0);
        assert_offset_at(&data, f64::MAX, f64::MAX, 95.0);
    }

    #[test]
    fn offset_at_after_remove() {
        let data = setup();

        apply(&data, &TestEvent::change(EditorRange::new(1, 3, 1, 6), "", None));

        assert_offset_at(&data, 0.0, 1.0, 1.0);
        assert_offset_at(&data, 0.0, 13.0, 13.0);
        assert_offset_at(&data, 1.0, 0.0, 14.0);
    }

    #[test]
    fn offset_at_after_replace() {
        let data = setup();

        apply(&data, &TestEvent::change(EditorRange::new(1, 3, 1, 6), "is could be", None));

        assert_offset_at(&data, 0.0, 1.0, 1.0);
        assert_offset_at(&data, 0.0, 24.0, 24.0);
        assert_offset_at(&data, 1.0, 0.0, 25.0);
    }

    #[test]
    fn offset_at_after_insert_line() {
        let data = setup();

        apply(&data, &TestEvent::change(EditorRange::new(1, 3, 1, 6), "is could be\na line with number", None));

        assert_offset_at(&data, 0.0, 1.0, 1.0);
        assert_offset_at(&data, 0.0, 13.0, 13.0);
        assert_offset_at(&data, 1.0, 0.0, 14.0);
        assert_offset_at(&data, 1.0, 18.0, 13.0 + 1.0 + 18.0);
        assert_offset_at(&data, 1.0, 29.0, 13.0 + 1.0 + 29.0);
        assert_offset_at(&data, 2.0, 0.0, 13.0 + 1.0 + 29.0 + 1.0);
    }

    #[test]
    fn offset_at_after_remove_line() {
        let data = setup();

        apply(&data, &TestEvent::change(EditorRange::new(1, 3, 2, 6), "", None));

        assert_offset_at(&data, 0.0, 1.0, 1.0);
        assert_offset_at(&data, 0.0, 2.0, 2.0);
        assert_offset_at(&data, 1.0, 0.0, 25.0);
    }

    #[test]
    fn position_at() {
        let data = setup();
        assert_position_at(&data, 0.0, 0.0, 0.0);
        assert_position_at(&data, f64::from_bits(1), 0.0, 0.0);
        assert_position_at(&data, 1.0, 0.0, 1.0);
        assert_position_at(&data, 16.0, 0.0, 16.0);
        assert_position_at(&data, 17.0, 1.0, 0.0);
        assert_position_at(&data, 20.0, 1.0, 3.0);
        assert_position_at(&data, 45.0, 2.0, 0.0);
        assert_position_at(&data, 95.0, 3.0, 29.0);
        assert_position_at(&data, 96.0, 3.0, 29.0);
        assert_position_at(&data, 99.0, 3.0, 29.0);
        assert_position_at(&data, f64::MAX, 3.0, 29.0);
    }

    #[test]
    fn get_word_range_at_position() {
        let data = new_data(js_lines(&["aaaa bbbb+cccc abc"]), Eol::Lf);
        let document = data.document();

        let range = document.get_word_range_at_position(&p(0.0, 2.0), None).unwrap();
        assert_range(range, 0.0, 0.0, 0.0, 4.0);

        // ignore bad regular expresson /.*/
        assert!(document.get_word_range_at_position(&p(0.0, 2.0), Some(&reg_exp(".*", ""))).is_err());

        let range = document.get_word_range_at_position(&p(0.0, 5.0), Some(&reg_exp("[a-z+]+", ""))).unwrap();
        assert_range(range, 0.0, 5.0, 0.0, 14.0);

        let range = document.get_word_range_at_position(&p(0.0, 17.0), Some(&reg_exp("[a-z+]+", ""))).unwrap();
        assert_range(range, 0.0, 15.0, 0.0, 18.0);

        let range = document.get_word_range_at_position(&p(0.0, 11.0), Some(&reg_exp("yy", ""))).unwrap();
        assert_eq!(range, None);
    }

    #[test]
    fn get_word_range_at_position_doesnt_quite_use_the_regex_as_expected_29102() {
        let data = new_data(js_lines(&["some text here", "/** foo bar */", "function() {", "\t\"far boo\"", "}"]), Eol::Lf);
        let document = data.document();

        let comment = reg_exp(r"\/\*.+\*\/", "");
        let range = document.get_word_range_at_position(&p(0.0, 0.0), Some(&comment)).unwrap();
        assert_eq!(range, None);

        let range = document.get_word_range_at_position(&p(1.0, 0.0), Some(&comment)).unwrap();
        assert_range(range, 1.0, 0.0, 1.0, 14.0);

        let quoted = reg_exp(r#"("|').*\1"#, "");
        let range = document.get_word_range_at_position(&p(3.0, 0.0), Some(&quoted)).unwrap();
        assert_eq!(range, None);

        let range = document.get_word_range_at_position(&p(3.0, 1.0), Some(&quoted)).unwrap();
        assert_range(range, 3.0, 1.0, 3.0, 10.0);
    }

    #[test]
    fn get_word_range_at_position_can_freeze_the_extension_host_95319() {
        let regex = reg_exp(
            r"(https?:\/\/github\.com\/(([^\s]+)\/([^\s]+))\/([^\s]+\/)?(issues|pull)\/([0-9]+))|(([^\s]+)\/([^\s]+))?#([1-9][0-9]*)($|[\s\:\;\-\(\=])",
            "",
        );

        let data = new_data(vec![read_perf_data()], Eol::Lf);
        let document = data.document();

        // this test only ensures that we eventually give and timeout (when searching "funny" words and long lines)
        // for the sake of speedy tests we lower the timeBudget here
        let config = set_default_get_word_at_text_config(GetWordAtTextConfig { max_len: 1000, window_size: 15, time_budget: 30 });
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let range = document.get_word_range_at_position(&p(0.0, 1_177_170.0), Some(&regex)).unwrap();
            assert_eq!(range, None);

            let pos = p(0.0, 1_177_170.0);
            let range = document.get_word_range_at_position(&pos, None).unwrap().expect("deveria haver uma palavra");
            assert!(range.contains_position(&pos));
            assert_eq!(text_of(&document.get_text(Some(&range))), "TaskDefinition");
        }));
        config.dispose();
        if let Err(payload) = outcome {
            std::panic::resume_unwind(payload);
        }
    }

    #[test]
    fn rename_popup_sometimes_populates_with_text_on_the_left_side_omitted_96013() {
        let regex = reg_exp(
            r#"(-?\d*\.\d\w*)|([^\`\~\!\@\#\$\%\^\&\*\(\)\-\=\+\[\{\]\}\\\|\;\:\'\"\,\.\<\>\/\?\s]+)"#,
            "g",
        );
        let line = "int abcdefhijklmnopqwvrstxyz;";

        let data = new_data(js_lines(&[line]), Eol::Lf);

        let range = data.document().get_word_range_at_position(&p(0.0, 27.0), Some(&regex)).unwrap().expect("deveria haver uma palavra");
        assert_eq!(range.start().line(), 0.0);
        assert_eq!(range.end().line(), 0.0);
        assert_eq!(range.start().character(), 4.0);
        assert_eq!(range.end().character(), 28.0);
    }

    #[test]
    fn custom_snippet_tm_selected_text_not_show_suggestion_108892() {
        let data = new_data(
            js_lines(&[r#"        <p><span xml:lang="en">Sheldon</span>, soprannominato "<span xml:lang="en">Shelly</span> dalla madre e dalla sorella, è nato a <span xml:lang="en">Galveston</span>, in <span xml:lang="en">Texas</span>, il 26 febbraio 1980 in un supermercato. È stato un bambino prodigio, come testimoniato dal suo quoziente d'intelligenza (187, di molto superiore alla norma) e dalla sua rapida carriera scolastica: si è diplomato all'eta di 11 anni approdando alla stessa età alla formazione universitaria e all'età di 16 anni ha ottenuto il suo primo dottorato di ricerca. All'inizio della serie e per gran parte di essa vive con il coinquilino Leonard nell'appartamento 4A al 2311 <span xml:lang="en">North Los Robles Avenue</span> di <span xml:lang="en">Pasadena</span>, per poi trasferirsi nell'appartamento di <span xml:lang="en">Penny</span> con <span xml:lang="en">Amy</span> nella decima stagione. Come più volte afferma lui stesso possiede una memoria eidetica e un orecchio assoluto. È stato educato da una madre estremamente religiosa e, in più occasioni, questo aspetto contrasta con il rigore scientifico di <span xml:lang="en">Sheldon</span>; tuttavia la donna sembra essere l'unica persona in grado di comandarlo a bacchetta.</p>"#]),
            Eol::Lf,
        );
        let document = data.document();

        let pos = p(0.0, 55.0);
        let range = document.get_word_range_at_position(&pos, None).unwrap().expect("deveria haver uma palavra");
        assert_eq!(range.start().line(), 0.0);
        assert_eq!(range.end().line(), 0.0);
        assert_eq!(range.start().character(), 47.0);
        assert_eq!(range.end().character(), 61.0);
        assert_eq!(text_of(&document.get_text(Some(&range))), "soprannominato");
    }
}

mod updates_line_mapping {
    use super::*;

    #[derive(Clone, Copy)]
    enum AssertDocumentLineMappingDirection {
        OffsetToPosition,
        PositionToOffset,
    }

    fn position_to_str(position: &Position) -> String {
        format!("({},{})", position.line(), position.character())
    }

    fn assert_document_line_mapping(doc: &ExtHostDocumentData, direction: AssertDocumentLineMappingDirection) {
        let all_text = doc.document().get_text(None);
        let units = all_text.units();

        let (mut line, mut character, mut previous_is_carriage_return) = (0.0, 0.0, false);
        for offset in 0..=units.len() {
            // The position coordinate system cannot express the position between \r and \n
            let position = p(line, character + if previous_is_carriage_return { -1.0 } else { 0.0 });

            match direction {
                AssertDocumentLineMappingDirection::OffsetToPosition => {
                    let actual_position = doc.document().position_at(offset as f64);
                    assert_eq!(position_to_str(&actual_position), position_to_str(&position), "positionAt mismatch for offset {offset}");
                }
                AssertDocumentLineMappingDirection::PositionToOffset => {
                    // The position coordinate system cannot express the position between \r and \n
                    let expected_offset = offset as f64 + if previous_is_carriage_return { -1.0 } else { 0.0 };
                    let actual_offset = doc.document().offset_at(&position);
                    assert_eq!(actual_offset, expected_offset, "offsetAt mismatch for position {}", position_to_str(&position));
                }
            }

            let unit = units.get(offset).copied();
            if unit == Some(u16::from(b'\n')) {
                line += 1.0;
                character = 0.0;
            } else {
                character += 1.0;
            }

            previous_is_carriage_return = unit == Some(u16::from(b'\r'));
        }
    }

    fn test_line_mapping_direction_after_events(
        lines: &[&str],
        eol: Eol,
        direction: AssertDocumentLineMappingDirection,
        e: &TestEvent,
    ) {
        let my_document = new_data(js_lines(lines), eol);
        assert_document_line_mapping(&my_document, direction);

        my_document.mirror_mut().on_events(&e.to_event(eol));
        assert_document_line_mapping(&my_document, direction);
    }

    fn test_line_mapping_after_events(lines: &[&str], e: &TestEvent) {
        test_line_mapping_direction_after_events(lines, Eol::Lf, AssertDocumentLineMappingDirection::PositionToOffset, e);
        test_line_mapping_direction_after_events(lines, Eol::Lf, AssertDocumentLineMappingDirection::OffsetToPosition, e);

        test_line_mapping_direction_after_events(lines, Eol::CrLf, AssertDocumentLineMappingDirection::PositionToOffset, e);
        test_line_mapping_direction_after_events(lines, Eol::CrLf, AssertDocumentLineMappingDirection::OffsetToPosition, e);
    }

    const LINES: [&str; 4] = ["This is line one", "and this is line number two", "it is followed by #3", "and finished with the fourth."];

    #[test]
    fn line_mapping() {
        test_line_mapping_after_events(&LINES, &TestEvent { changes: Vec::new(), eol: None, version_id: 7 });
    }

    #[test]
    fn after_remove() {
        test_line_mapping_after_events(&LINES, &TestEvent::change(EditorRange::new(1, 3, 1, 6), "", None));
    }

    #[test]
    fn after_replace() {
        test_line_mapping_after_events(&LINES, &TestEvent::change(EditorRange::new(1, 3, 1, 6), "is could be", None));
    }

    #[test]
    fn after_insert_line() {
        test_line_mapping_after_events(&LINES, &TestEvent::change(EditorRange::new(1, 3, 1, 6), "is could be\na line with number", None));
    }

    #[test]
    fn after_insert_two_lines() {
        test_line_mapping_after_events(
            &LINES,
            &TestEvent::change(EditorRange::new(1, 3, 1, 6), "is could be\na line with number\nyet another line", None),
        );
    }

    #[test]
    fn after_remove_line() {
        test_line_mapping_after_events(&LINES, &TestEvent::change(EditorRange::new(1, 3, 2, 6), "", None));
    }

    #[test]
    fn after_remove_two_lines() {
        test_line_mapping_after_events(&LINES, &TestEvent::change(EditorRange::new(1, 3, 3, 6), "", None));
    }

    #[test]
    fn after_deleting_entire_content() {
        test_line_mapping_after_events(&LINES, &TestEvent::change(EditorRange::new(1, 3, 4, 30), "", None));
    }

    #[test]
    fn after_replacing_entire_content() {
        test_line_mapping_after_events(
            &LINES,
            &TestEvent::change(EditorRange::new(1, 3, 4, 30), "some new text\nthat\nspans multiple lines", None),
        );
    }

    #[test]
    fn after_changing_eol_to_crlf() {
        test_line_mapping_after_events(&LINES, &TestEvent::change(EditorRange::new(1, 1, 1, 1), "", Some(Eol::CrLf)));
    }

    #[test]
    fn after_changing_eol_to_lf() {
        test_line_mapping_after_events(&LINES, &TestEvent::change(EditorRange::new(1, 1, 1, 1), "", Some(Eol::Lf)));
    }
}
