//! Porte de `src/vs/editor/test/browser/controller/textAreaInput.test.ts`, caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - As gravações de eventos IME (`IRecorded`, de `imeRecordedTypes.ts`) e as saídas esperadas de cada caso não
//!   são copiadas: o teste as lê do próprio `ref/vscode` em tempo de execução. Cada caso procura o seu
//!   `test('<nome>', ...)` no arquivo original, lê com um leitor mínimo de literais JavaScript (objetos, listas,
//!   textos com aspas simples, duplas ou crase, números, `true`/`false`/`null`/`undefined` e
//!   `OperatingSystem.X`) o objeto de `const recorded: IRecorded = ...` e a lista de
//!   `assert.deepStrictEqual(actualOutgoingEvents, ...)`. Assim o dado do oráculo não pode divergir do porte.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - O `TestTextAreaWrapper` do original (um `Disposable` com `Emitter`s) é [`TestTextAreaWrapper`]: guarda o
//!   estado e confere que o `TextAreaInput` não mexe na textarea durante um `compositionstart`. Os eventos que
//!   ele emitia viram chamadas aos métodos `handle_*` do `TextAreaInput` (ver "Divergências" do módulo de
//!   produção); `keypress` e `beforeinput` não têm método, porque o `TextAreaInput` nunca se inscreve neles.
//!   `onCut`, `onCopy`, `onPaste`, `onFocus`, `onBlur` e `onSyntheticTap` eram `Event.None` e nunca disparam.
//! - `await yieldNow()` entre os eventos (um `queueMicrotask`) deixaria rodar tarefas agendadas pelo
//!   `TextAreaInput`; nenhum caso agenda algo (o leitor de tela está desligado e não há recortar), então não
//!   há equivalente.
//! - O host devolve o mesmo `TextAreaState` vazio, e `deduceModelPosition` (e o `context: null!`, que aqui é o
//!   `create_clipboard_copy_event`) falham se alguém os chamar.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::event::Event;
use je_base::js_string::substring;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::platform::OperatingSystem;
use je_base::runtime::date_now;
use je_editor::edit_context::clipboard_utils::{ClipboardCopyEvent, ClipboardEvent};
use je_editor::edit_context::text_area_input::{
    CompositionEvent, IAccessibilityService, IBrowser, ICompleteTextAreaWrapper, ITextAreaInputHost, InputEvent,
    KeyboardEventData, TextAreaInput, TextAreaSelectionDirection,
};
use je_editor::edit_context::text_area_state::{ITextAreaWrapper, TextAreaState};
use je_editor::position::Position;

/// O arquivo de teste do VS Code (o oráculo dos dados).
const ORACLE_TEST: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/vscode/src/vs/editor/test/browser/controller/textAreaInput.test.ts");

// #region leitor de literais JavaScript

/// Um valor de literal JavaScript.
#[derive(Clone, Debug)]
enum Js {
    Null,
    Undefined,
    Bool(bool),
    Num(f64),
    Str(JsString),
    Arr(Vec<Js>),
    Obj(Vec<(String, Js)>),
    /// Um identificador com pontos (`OperatingSystem.Macintosh`).
    Ident(String),
}

impl Js {
    fn get(&self, key: &str) -> &Js {
        match self {
            Js::Obj(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value)
                .unwrap_or_else(|| panic!("campo `{key}` ausente em {self:?}")),
            other => panic!("esperava objeto para ler `{key}`, veio {other:?}"),
        }
    }

    fn string(&self) -> &JsString {
        match self {
            Js::Str(text) => text,
            other => panic!("esperava texto, veio {other:?}"),
        }
    }

    fn number(&self) -> f64 {
        match self {
            Js::Num(value) => *value,
            other => panic!("esperava número, veio {other:?}"),
        }
    }

    fn boolean(&self) -> bool {
        match self {
            Js::Bool(value) => *value,
            other => panic!("esperava booleano, veio {other:?}"),
        }
    }

    fn array(&self) -> &[Js] {
        match self {
            Js::Arr(items) => items,
            other => panic!("esperava lista, veio {other:?}"),
        }
    }

    fn ident(&self) -> &str {
        match self {
            Js::Ident(name) => name,
            other => panic!("esperava identificador, veio {other:?}"),
        }
    }
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, pos: usize) -> Self {
        Parser { src, pos }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn rest(&self) -> &str {
        &self.src[self.pos..]
    }

    fn skip_ws(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.rest().starts_with("//") => {
                    while let Some(c) = self.bump() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                Some('/') if self.rest().starts_with("/*") => {
                    let end = self.rest().find("*/").expect("comentário de bloco sem fim");
                    self.pos += end + 2;
                }
                _ => break,
            }
        }
    }

    fn expect(&mut self, expected: char) {
        self.skip_ws();
        let found = self.bump();
        assert_eq!(found, Some(expected), "esperava `{expected}` perto de: {:.60}", self.rest());
    }

    fn value(&mut self) -> Js {
        self.skip_ws();
        match self.peek().expect("fim inesperado do literal") {
            '{' => self.object(),
            '[' => self.array(),
            '\'' | '"' | '`' => Js::Str(self.string()),
            '(' => {
                self.bump();
                let inner = self.value();
                self.expect(')');
                inner
            }
            c if c == '-' || c.is_ascii_digit() => self.number(),
            _ => self.ident(),
        }
    }

    fn object(&mut self) -> Js {
        self.expect('{');
        let mut entries = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some('}') {
                self.bump();
                break;
            }
            let key = match self.peek() {
                Some('\'' | '"') => self.string().to_string_lossy(),
                _ => self.word(),
            };
            self.expect(':');
            let value = self.value();
            entries.push((key, value));
            self.skip_ws();
            if self.peek() == Some(',') {
                self.bump();
            }
        }
        Js::Obj(entries)
    }

    fn array(&mut self) -> Js {
        self.expect('[');
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(']') {
                self.bump();
                break;
            }
            items.push(self.value());
            self.skip_ws();
            if self.peek() == Some(',') {
                self.bump();
            }
        }
        Js::Arr(items)
    }

    fn string(&mut self) -> JsString {
        let quote = self.bump().expect("texto sem aspas");
        let mut units: Vec<u16> = Vec::new();
        let mut buffer = [0u16; 2];
        loop {
            let c = self.bump().expect("texto sem fim");
            if c == quote {
                break;
            }
            if c != '\\' {
                units.extend_from_slice(c.encode_utf16(&mut buffer));
                continue;
            }
            match self.bump().expect("escape sem fim") {
                'n' => units.push(10),
                't' => units.push(9),
                'r' => units.push(13),
                '0' => units.push(0),
                'u' => {
                    let hex: String = (0..4).map(|_| self.bump().expect("\\u sem dígitos")).collect();
                    units.push(u16::from_str_radix(&hex, 16).expect("\\u inválido"));
                }
                other => units.extend_from_slice(other.encode_utf16(&mut buffer)),
            }
        }
        JsString::from_units(units)
    }

    fn number(&mut self) -> Js {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E')) {
            self.bump();
        }
        Js::Num(self.src[start..self.pos].parse().expect("número inválido"))
    }

    /// Um nome sem pontos (chave de objeto).
    fn word(&mut self) -> String {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_' || c == '$') {
            self.bump();
        }
        assert!(self.pos > start, "esperava um nome perto de: {:.60}", self.rest());
        self.src[start..self.pos].to_string()
    }

    fn ident(&mut self) -> Js {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_' || c == '$' || c == '.') {
            self.bump();
        }
        assert!(self.pos > start, "esperava um valor perto de: {:.60}", self.rest());
        match &self.src[start..self.pos] {
            "null" => Js::Null,
            "undefined" => Js::Undefined,
            "true" => Js::Bool(true),
            "false" => Js::Bool(false),
            name => Js::Ident(name.to_string()),
        }
    }
}

// #endregion

// #region IRecorded e eventos de saída

/// `IRecordedTextareaState`.
#[derive(Clone, Debug, PartialEq)]
struct RecordedTextareaState {
    selection_direction: TextAreaSelectionDirection,
    selection_end: usize,
    selection_start: usize,
    value: JsString,
}

impl RecordedTextareaState {
    fn from_js(js: &Js) -> Self {
        let selection_direction = match js.get("selectionDirection").string() {
            direction if *direction == "forward" => TextAreaSelectionDirection::Forward,
            direction if *direction == "backward" => TextAreaSelectionDirection::Backward,
            direction if *direction == "none" => TextAreaSelectionDirection::None,
            other => panic!("selectionDirection desconhecida: {other}"),
        };
        Self {
            selection_direction,
            selection_end: js.get("selectionEnd").number() as usize,
            selection_start: js.get("selectionStart").number() as usize,
            value: js.get("value").string().clone(),
        }
    }
}

/// `IRecorded`.
struct Recorded {
    os: OperatingSystem,
    browser: IBrowser,
    initial: RecordedTextareaState,
    events: Vec<Js>,
    final_state: RecordedTextareaState,
}

/// `OutoingEvent`.
#[derive(Clone, Debug, PartialEq)]
enum OutgoingEvent {
    Type { text: JsString, replace_prev_char_cnt: usize, replace_next_char_cnt: usize, position_delta: i64 },
    CompositionStart { data: JsString },
    CompositionUpdate { data: JsString },
    CompositionEnd,
}

impl OutgoingEvent {
    fn from_js(js: &Js) -> Self {
        match js.get("type").string().to_string_lossy().as_str() {
            "type" => OutgoingEvent::Type {
                text: js.get("text").string().clone(),
                replace_prev_char_cnt: js.get("replacePrevCharCnt").number() as usize,
                replace_next_char_cnt: js.get("replaceNextCharCnt").number() as usize,
                position_delta: js.get("positionDelta").number() as i64,
            },
            "compositionStart" => OutgoingEvent::CompositionStart { data: js.get("data").string().clone() },
            "compositionUpdate" => OutgoingEvent::CompositionUpdate { data: js.get("data").string().clone() },
            "compositionEnd" => OutgoingEvent::CompositionEnd,
            other => panic!("evento de saída desconhecido: {other}"),
        }
    }
}

fn operating_system(js: &Js) -> OperatingSystem {
    match js.ident() {
        "OperatingSystem.Windows" => OperatingSystem::Windows,
        "OperatingSystem.Macintosh" => OperatingSystem::Macintosh,
        "OperatingSystem.Linux" => OperatingSystem::Linux,
        other => panic!("sistema operacional desconhecido: {other}"),
    }
}

/// Lê, do arquivo de teste do VS Code, a gravação e a saída esperada do caso `test('<name>', ...)`.
fn load_case(name: &str) -> (Recorded, Vec<OutgoingEvent>) {
    let source = std::fs::read_to_string(ORACLE_TEST).expect("ref/vscode (textAreaInput.test.ts) ausente");
    let marker = format!("test('{name}', async () => {{");
    let start = source.find(&marker).unwrap_or_else(|| panic!("caso `{name}` não está no arquivo de teste"));
    let end = source[start + marker.len()..].find("\n\ttest('").map_or(source.len(), |offset| start + marker.len() + offset);
    let block = &source[start..end];

    let recorded_at = block.find("const recorded: IRecorded = ").expect("gravação ausente") + "const recorded: IRecorded = ".len();
    let recorded_js = Parser::new(block, recorded_at).value();
    let recorded = Recorded {
        os: operating_system(recorded_js.get("env").get("OS")),
        browser: {
            let browser = recorded_js.get("env").get("browser");
            IBrowser {
                is_android: browser.get("isAndroid").boolean(),
                is_firefox: browser.get("isFirefox").boolean(),
                is_chrome: browser.get("isChrome").boolean(),
                is_safari: browser.get("isSafari").boolean(),
            }
        },
        initial: RecordedTextareaState::from_js(recorded_js.get("initial")),
        events: recorded_js.get("events").array().to_vec(),
        final_state: RecordedTextareaState::from_js(recorded_js.get("final")),
    };

    let expected_marker = "assert.deepStrictEqual(actualOutgoingEvents, ";
    let expected_at = block.find(expected_marker).expect("saída esperada ausente") + expected_marker.len();
    let expected_js = Parser::new(block, expected_at).value();
    let expected = expected_js.array().iter().map(OutgoingEvent::from_js).collect();
    (recorded, expected)
}

// #endregion

// #region dublês

/// O `TestTextAreaWrapper` do original: o estado da textarea gravada.
struct TestTextAreaWrapper {
    state: RecordedTextareaState,
    /// O `_currDispatchingEvent?.type === 'compositionstart'` do original.
    dispatching_composition_start: bool,
}

impl TestTextAreaWrapper {
    fn new() -> Self {
        Self {
            state: RecordedTextareaState {
                selection_direction: TextAreaSelectionDirection::None,
                selection_end: 0,
                selection_start: 0,
                value: JsString::new(),
            },
            dispatching_composition_start: false,
        }
    }

    /// `_initialize(state)`: só valor e seleção, a direção continua `none`.
    fn initialize(&mut self, state: &RecordedTextareaState) {
        self.state.value = state.value.clone();
        self.state.selection_start = state.selection_start;
        self.state.selection_end = state.selection_end;
    }
}

impl ITextAreaWrapper for TestTextAreaWrapper {
    fn get_value(&self) -> JsString {
        self.state.value.clone()
    }

    fn set_value(&mut self, _reason: &str, value: &[u16]) {
        assert!(!self.dispatching_composition_start, "should not change the state of the textarea in a compositionstart");
        self.state.value = JsString::from(value);
    }

    fn get_selection_start(&self) -> usize {
        if self.state.selection_direction == TextAreaSelectionDirection::Backward {
            self.state.selection_end
        } else {
            self.state.selection_start
        }
    }

    fn get_selection_end(&self) -> usize {
        if self.state.selection_direction == TextAreaSelectionDirection::Backward {
            self.state.selection_start
        } else {
            self.state.selection_end
        }
    }

    fn set_selection_range(&mut self, _reason: &str, selection_start: usize, selection_end: usize) {
        assert!(!self.dispatching_composition_start, "should not change the state of the textarea in a compositionstart");
        self.state.selection_start = selection_start;
        self.state.selection_end = selection_end;
        self.state.selection_direction =
            if selection_start != selection_end { TextAreaSelectionDirection::Forward } else { TextAreaSelectionDirection::None };
    }
}

impl ICompleteTextAreaWrapper for TestTextAreaWrapper {
    fn set_ignore_selection_change_time(&mut self, _reason: &str) {}

    fn get_ignore_selection_change_time(&self) -> u64 {
        date_now()
    }

    fn reset_selection_change_time(&mut self) {}

    fn has_focus(&self) -> bool {
        true
    }
}

struct TestHost;

impl ITextAreaInputHost for TestHost {
    fn get_screen_reader_content(&self) -> TextAreaState {
        TextAreaState::new(JsString::new(), 0, 0, None, None)
    }

    fn deduce_model_position(&self, _view_anchor_position: Position, _delta_offset: i64, _line_feed_cnt: usize) -> Position {
        panic!("Function not implemented.");
    }

    fn create_clipboard_copy_event(&self, _e: Rc<ClipboardEvent>, _is_cut: bool, _is_firefox: bool) -> ClipboardCopyEvent {
        panic!("context: null!");
    }
}

/// `TestAccessibilityService`: sem leitor de tela, e o evento nunca dispara.
struct TestAccessibilityService;

impl IAccessibilityService for TestAccessibilityService {
    fn is_screen_reader_optimized(&self) -> bool {
        false
    }

    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }
}

// #endregion

/// `_dispatchRecordedEvent(event)`: aplica o estado gravado na textarea e entrega o evento ao `TextAreaInput`.
fn dispatch_recorded_event(wrapper: &Rc<RefCell<TestTextAreaWrapper>>, input: &TextAreaInput, event: &Js) {
    let event_type = event.get("type").string().to_string_lossy();
    {
        let mut wrapper = wrapper.borrow_mut();
        wrapper.dispatching_composition_start = event_type == "compositionstart";
        wrapper.state = RecordedTextareaState::from_js(event.get("state"));
    }

    match event_type.as_str() {
        "keydown" | "keyup" => {
            // o `mockEvent` do original (`getModifierState` devolve `false`)
            let mock_event = Rc::new(KeyboardEventData {
                key_code: event.get("keyCode").number() as i32,
                char_code: event.get("charCode").number() as u32,
                code: event.get("code").string().to_string_lossy(),
                ctrl_key: event.get("ctrlKey").boolean(),
                shift_key: event.get("shiftKey").boolean(),
                alt_key: event.get("altKey").boolean(),
                meta_key: event.get("metaKey").boolean(),
                is_composing: event.get("isComposing").boolean(),
                ..Default::default()
            });
            if event_type == "keydown" {
                input.handle_key_down(mock_event);
            } else {
                input.handle_key_up(mock_event);
            }
        }
        // O `TextAreaInput` nunca se inscreve em `keypress` nem em `beforeinput`.
        "keypress" | "beforeinput" => {}
        "compositionstart" => input.handle_composition_start(&CompositionEvent { data: event.get("data").string().clone() }),
        "compositionupdate" => input.handle_composition_update(&CompositionEvent { data: event.get("data").string().clone() }),
        "compositionend" => input.handle_composition_end(&CompositionEvent { data: event.get("data").string().clone() }),
        "input" => input.handle_input(&InputEvent { input_type: event.get("inputType").string().to_string_lossy() }),
        _ => panic!("Not Implemented"),
    }
    wrapper.borrow_mut().dispatching_composition_start = false;
}

/// `simulateInteraction(recorded)`.
fn simulate_interaction(recorded: &Recorded) -> Vec<OutgoingEvent> {
    let disposables = DisposableStore::new();
    let wrapper = Rc::new(RefCell::new(TestTextAreaWrapper::new()));
    let input = TextAreaInput::new(
        Rc::new(TestHost),
        wrapper.clone(),
        recorded.os,
        recorded.browser,
        Rc::new(TestAccessibilityService),
    );

    wrapper.borrow_mut().initialize(&recorded.initial);
    input.initialize_from_test();

    let outgoing_events: Rc<RefCell<Vec<OutgoingEvent>>> = Rc::default();

    let sink = outgoing_events.clone();
    disposables.add(input.on_type.listen(move |e| {
        sink.borrow_mut().push(OutgoingEvent::Type {
            text: e.text.clone(),
            replace_prev_char_cnt: e.replace_prev_char_cnt,
            replace_next_char_cnt: e.replace_next_char_cnt,
            position_delta: e.position_delta,
        });
    }));
    let sink = outgoing_events.clone();
    disposables.add(
        input.on_composition_start.listen(move |e| sink.borrow_mut().push(OutgoingEvent::CompositionStart { data: e.data.clone() })),
    );
    let sink = outgoing_events.clone();
    disposables.add(
        input.on_composition_update.listen(move |e| sink.borrow_mut().push(OutgoingEvent::CompositionUpdate { data: e.data.clone() })),
    );
    let sink = outgoing_events.clone();
    disposables.add(input.on_composition_end.listen(move |_| sink.borrow_mut().push(OutgoingEvent::CompositionEnd)));

    for event in &recorded.events {
        dispatch_recorded_event(&wrapper, &input, event);
    }

    input.dispose();
    disposables.dispose();

    std::mem::take(&mut *outgoing_events.borrow_mut())
}

/// `interpretTypeEvents(OS, browser, initialState, events)`: aplica os `type` ao estado inicial.
fn interpret_type_events(
    os: OperatingSystem,
    browser: &IBrowser,
    initial_state: &RecordedTextareaState,
    events: &[OutgoingEvent],
) -> RecordedTextareaState {
    let mut text: Vec<u16> = initial_state.value.units().to_vec();
    let mut selection_start = initial_state.selection_start as i64;
    let mut selection_end = initial_state.selection_end as i64;
    for event in events {
        if let OutgoingEvent::Type { text: typed, replace_prev_char_cnt, replace_next_char_cnt, position_delta } = event {
            let mut next: Vec<u16> = substring(&text, 0, selection_start - *replace_prev_char_cnt as i64).to_vec();
            next.extend_from_slice(typed.units());
            next.extend_from_slice(substring(&text, selection_end + *replace_next_char_cnt as i64, text.len() as i64));
            text = next;
            selection_start = selection_start - *replace_prev_char_cnt as i64 + typed.len() as i64;
            selection_end = selection_start;

            if *position_delta != 0 {
                selection_start += position_delta;
                selection_end += position_delta;
            }
        }
    }
    let uses_forward_direction = browser.is_firefox || os == OperatingSystem::Windows || os == OperatingSystem::Linux;
    RecordedTextareaState {
        selection_direction: if uses_forward_direction { TextAreaSelectionDirection::Forward } else { TextAreaSelectionDirection::None },
        selection_end: selection_end as usize,
        selection_start: selection_start as usize,
        value: JsString::from_units(text),
    }
}

/// O corpo comum dos casos: simula, confere os eventos de saída e o estado resultante.
fn run_case(name: &str) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (recorded, expected) = load_case(name);

    let actual_outgoing_events = simulate_interaction(&recorded);
    assert_eq!(actual_outgoing_events, expected);

    let actual_resulting_state =
        interpret_type_events(recorded.os, &recorded.browser, &recorded.initial, &actual_outgoing_events);
    assert_eq!(actual_resulting_state, recorded.final_state);
}

#[test]
fn mac_os_chrome_korean_using_2_set_korean_1() {
    // macOS, 2-Set Korean, type 'dkrk' and click
    run_case("macOS - Chrome - Korean using 2-Set Korean (1)");
}

#[test]
fn mac_os_chrome_korean_using_2_set_korean_2() {
    // macOS, 2-Set Korean, type 'qud' and click
    // See https://github.com/microsoft/vscode/issues/134254
    run_case("macOS - Chrome - Korean using 2-Set Korean (2)");
}

#[test]
fn mac_os_chrome_japanese_using_hiragana_google() {
    // macOS, Hiragana (Google), type 'sennsei' and Enter
    run_case("macOS - Chrome - Japanese using Hiragana (Google)");
}

#[test]
fn mac_os_chrome_chinese_using_pinyin_traditional() {
    // macOS, Pinyin - Traditional, type 'xu' and '1'
    run_case("macOS - Chrome - Chinese using Pinyin - Traditional");
}

#[test]
fn mac_os_chrome_long_press_with_arrow_keys() {
    // macOS, English, long press o, press arrow right twice and then press Enter
    // See https://github.com/microsoft/vscode/issues/67739
    run_case("macOS - Chrome - long press with arrow keys");
}

#[test]
fn mac_os_chrome_pressing_quotes_on_us_intl() {
    // macOS, US International - PC, press ', ', ;
    run_case("macOS - Chrome - pressing quotes on US Intl");
}

#[test]
fn mac_os_chrome_inserting_emoji_using_ctrl_cmd_space() {
    // macOS, English, press ctrl+cmd+space, and then pick an emoji using the mouse
    // See https://github.com/microsoft/vscode/issues/4271
    run_case("macOS - Chrome - inserting emoji using ctrl+cmd+space");
}

#[test]
fn mac_os_firefox_long_press_with_mouse() {
    // macOS, English, long press e and choose using mouse
    // See https://github.com/microsoft/monaco-editor/issues/2358
    run_case("macOS - Firefox - long press with mouse");
}

#[test]
fn mac_os_firefox_inserting_emojis() {
    // macOS, English, from the edit menu, click Emoji & Symbols, select an emoji
    // See https://github.com/microsoft/vscode/issues/106392
    run_case("macOS - Firefox - inserting emojis");
}

#[test]
fn mac_os_safari_chinese_issue_119469() {
    run_case("macOS - Safari - Chinese - issue #119469");
}

#[test]
fn windows_chrome_japanese_using_hiragana() {
    // Windows, Japanese/Hiragana, type 'sennsei' and Enter
    run_case("Windows - Chrome - Japanese using Hiragana");
}

#[test]
fn windows_11_chrome_japanese_using_hiragana() {
    // Windows, Japanese/Hiragana, type 'sennsei' and Enter
    run_case("Windows 11 - Chrome - Japanese using Hiragana");
}

#[test]
fn windows_chrome_korean_1() {
    // Windows, Korean, type 'dkrk' and click
    run_case("Windows - Chrome - Korean (1)");
}

#[test]
fn windows_11_chrome_korean_1() {
    // Windows, Korean, type 'dkrk' and Space
    run_case("Windows 11 - Chrome - Korean (1)");
}

#[test]
fn windows_chrome_korean_2() {
    // Windows, Korean, type 'gksrmf' and Space
    run_case("Windows - Chrome - Korean (2)");
}

#[test]
fn windows_11_chrome_korean_2() {
    // Windows, Korean, type 'gksrmf' and Space
    run_case("Windows 11 - Chrome - Korean (2)");
}

#[test]
fn windows_chrome_chinese() {
    // Windows, Chinese, Type 'ni' press Space and then 'hao' and press Space.
    run_case("Windows - Chrome - Chinese");
}

#[test]
fn windows_11_chrome_chinese() {
    // Windows, Chinese, Type 'ni' press Space and then 'hao' and press Space.
    run_case("Windows 11 - Chrome - Chinese");
}

#[test]
fn linux_chrome_korean() {
    // Linux, fcitx Hangul, Type 'rkr' and then click.
    run_case("Linux - Chrome - Korean");
}
