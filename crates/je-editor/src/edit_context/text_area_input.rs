//! Porte de `src/vs/editor/browser/controller/editContext/textArea/textAreaEditContextInput.ts`: a máquina de
//! estados que traduz os eventos da textarea oculta (teclado, composição, `input`, colar, recortar, copiar, foco)
//! em dados para o editor ([`ITypeData`], [`IPasteData`], [`ICompositionData`]).
//!
//! O [`TextAreaInput`] escreve o conteúdo de leitor de tela na textarea e analisa os eventos dela para gerar:
//! `on_cut`, `on_paste` e `on_type`. Os eventos de composição são gerados para apresentação (a entrada da
//! composição já chega por `on_type`).
//!
//! O que o original traz além do `TextAreaInput`: `ICompleteTextAreaWrapper`, `IBrowser`, `ITextAreaInputHost`,
//! `ICompositionData`, `ICompositionStartEvent`, `IPasteData`, `TextAreaSyntethicEvents` e o `TextAreaWrapper`
//! sobre o `HTMLTextAreaElement`. O `ClipboardEventUtils` e o `InMemoryClipboardMetadataManager` moram em
//! `clipboardUtils.ts` (já portado em [`crate::edit_context::clipboard_utils`]).
//!
//! # Divergências
//!
//! Eventos DOM de entrada. No original o `TextAreaInput` se inscreve nos eventos que o
//! `ICompleteTextAreaWrapper` emite (`Event<KeyboardEvent>` etc.). Em Rust não há DOM: quem possui a textarea
//! (o winit/IME, ou o dublê do teste) entrega cada evento ao método de mesmo nome do [`TextAreaInput`]. O
//! mapeamento, com o tipo que carrega cada evento:
//!
//! | evento DOM                      | método                                              | dado                    |
//! |---------------------------------|-----------------------------------------------------|-------------------------|
//! | `keydown`                       | [`TextAreaInput::handle_key_down`]                  | [`KeyboardEventData`]   |
//! | `keyup`                         | [`TextAreaInput::handle_key_up`]                    | [`KeyboardEventData`]   |
//! | `compositionstart`              | [`TextAreaInput::handle_composition_start`]         | [`CompositionEvent`]    |
//! | `compositionupdate`             | [`TextAreaInput::handle_composition_update`]        | [`CompositionEvent`]    |
//! | `compositionend`                | [`TextAreaInput::handle_composition_end`]           | [`CompositionEvent`]    |
//! | `input`                         | [`TextAreaInput::handle_input`]                     | [`InputEvent`]          |
//! | `cut`                           | [`TextAreaInput::handle_cut`]                       | `Rc<ClipboardEvent>`    |
//! | `copy`                          | [`TextAreaInput::handle_copy`]                      | `Rc<ClipboardEvent>`    |
//! | `paste`                         | [`TextAreaInput::handle_paste`]                     | `Rc<ClipboardEvent>`    |
//! | `focus`                         | [`TextAreaInput::handle_focus`]                     | nenhum                  |
//! | `blur`                          | [`TextAreaInput::handle_blur`]                      | nenhum                  |
//! | `-monaco-textarea-synthetic-tap`| [`TextAreaInput::handle_synthetic_tap`]             | nenhum                  |
//! | `selectionchange` (no document) | [`TextAreaInput::handle_selection_change`]          | nenhum                  |
//!
//! - `keypress` e `beforeinput` existem no `ICompleteTextAreaWrapper`, mas o `TextAreaInput` nunca se inscreve
//!   neles (só o `TextAreaWrapper` os usa, para o `inputLatency`); por isso não há método para eles.
//! - O `selectionchange` é um evento do `document`, escutado só enquanto a textarea tem foco (o original instala
//!   e remove o listener em `_setHasFocus`). Aqui o listener é o método [`TextAreaInput::handle_selection_change`],
//!   que o dono do documento chama sempre, e a instalação é a bandeira que `set_has_focus` liga e desliga: com
//!   a bandeira desligada o método não faz nada, como um listener removido. `ownerDocument` deixa de existir.
//! - Os tipos DOM são só os campos que o código lê: do `KeyboardEvent` (`keyCode`, `charCode`, `code`,
//!   modificadores, `isComposing`, `getModifierState('AltGraph')`), do `CompositionEvent` (`data`) e do
//!   `InputEvent` (`inputType`); `preventDefault` e `stopPropagation` do teclado viram os marcadores de
//!   [`KeyboardEventData`], como em [`ClipboardEvent`].
//! - `StandardKeyboardEvent` mora em `je-widgets`, que depende de `je-ui` e não pode ser dependência do editor;
//!   a versão local ([`StandardKeyboardEvent`]) tem o mesmo cálculo (`extractKeyCode`, normalização de
//!   `isComposing` para `KEY_IN_COMPOSITION`, `equals`, `toKeyCodeChord`). O original lê o sistema e o navegador
//!   dos globais (`platform.isMacintosh`, `browser.isFirefox`...); aqui vêm do `OS` e do [`IBrowser`] do próprio
//!   `TextAreaInput`, o que torna a entrada gravada independente da máquina onde o teste roda. `isWebKit` é
//!   `isChrome || isSafari` ([`IBrowser::is_web_kit`]).
//! - `ITextAreaInputHost.context` (um `ViewContext`) só serve para `createClipboardCopyEvent(..., context, ...)`,
//!   que lê o `context.viewModel`. Como `IViewModel` tem tipos associados e não vira objeto de trait, o host
//!   expõe a operação ([`ITextAreaInputHost::create_clipboard_copy_event`]) em vez do contexto.
//! - `Event<IClipboardCopyEvent>` e `Event<IClipboardPasteEvent>` são `Event<Rc<ClipboardCopyEvent>>` e
//!   `Event<Rc<ClipboardPasteEvent>>`: o `TextAreaInput` consulta `isHandled` depois de disparar, e o `Emitter`
//!   entrega por valor.
//! - `IAccessibilityService` não existe no porte; [`IAccessibilityService`] tem só o que o `TextAreaInput`
//!   consome (`isScreenReaderOptimized` e `onDidChangeScreenReaderOptimized`).
//! - `ILogService.trace` e `_debugComposition` (só diagnóstico) não foram portados, e com eles o
//!   `isWrittenToTextArea` que só alimenta o log de `_setAndWriteTextAreaState`; `inputLatency.*` (telemetria)
//!   também não.
//! - `compositionstart` no Android e no caso comum faz o mesmo (só dispara `on_composition_start`): o ramo do
//!   Android do original existe só para registrar por que a textarea não pode ser limpa ali, e vira o
//!   comentário em [`TextAreaInput::handle_composition_start`].
//! - `TextAreaWrapper` envolve um `HTMLTextAreaElement`; aqui envolve um [`INativeTextArea`] (o que o código lê
//!   e escreve do elemento), com a mesma lógica de atalhos para "sem mudança" e de `ignoreSelectionChangeTime`.
//!   `shadowRoot.activeElement`, `isConnected` e `getActiveElement()` viram [`INativeTextArea::is_active_element`],
//!   [`INativeTextArea::is_in_shadow_root`] e [`INativeTextArea::is_connected`]; o `try` que salva a rolagem dos
//!   pais, foca, seleciona e restaura é [`INativeTextArea::focus_and_set_selection_range`] (a implementação
//!   engole o erro de uma textarea fora do DOM). `browser.isFirefox` vem do [`IBrowser`] dado ao construir.
//!   Os listeners `DomEmitter` do wrapper não existem: o dono entrega os eventos ao `TextAreaInput`.
//! - O `RunOnceScheduler` e o `MutableDisposable` são os de `je-base`; `Date.now()` é `je_base::runtime::date_now`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::RunOnceScheduler;
use je_base::event::{Emitter, Event};
use je_base::js_string::substring;
use je_base::key_codes::{KeyCode, KeyCodeUtils, KeyMod, event_key_code, is_modifier_key};
use je_base::keybindings::KeyCodeChord;
use je_base::lifecycle::{Disposable, IDisposable, MutableDisposable};
use je_base::platform::OperatingSystem;
use je_base::runtime::date_now;
use je_base::strings::is_high_surrogate;

use crate::edit_context::clipboard_utils::{
    ClipboardCopyEvent, ClipboardEvent, ClipboardPasteEvent, ClipboardStoredMetadata, CopyOptions,
    create_clipboard_paste_event, in_memory_clipboard_metadata_manager,
};
use crate::edit_context::text_area_state::{ITextAreaWrapper, ITypeData, TextAreaState};
use crate::position::Position;
use crate::selection::Selection;

/// `TextAreaSyntethicEvents`: o nome do evento sintético que o editor dispara na textarea num toque.
pub struct TextAreaSyntethicEvents;

impl TextAreaSyntethicEvents {
    /// `TextAreaSyntethicEvents.Tap`.
    pub const TAP: &'static str = "-monaco-textarea-synthetic-tap";
}

/// `ICompositionData` (e `ICompositionStartEvent`, de mesmo formato).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICompositionData {
    pub data: JsString,
}

/// `ICompositionStartEvent`.
pub type ICompositionStartEvent = ICompositionData;

/// O `CompositionEvent` do DOM, só com o que o código lê (`data`).
pub type CompositionEvent = ICompositionData;

/// `IPasteData`.
#[derive(Clone, Debug, PartialEq)]
pub struct IPasteData {
    pub text: JsString,
    pub metadata: Option<ClipboardStoredMetadata>,
}

/// O `InputEvent` do DOM, só com o que o código lê (`inputType`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub input_type: String,
}

/// O `KeyboardEvent` do DOM, só com o que o código lê. `key_code` é o `keyCode` do navegador (não o
/// [`KeyCode`]); `char_code` o `charCode`; `alt_graph_key` o `getModifierState('AltGraph')`.
#[derive(Debug, Default)]
pub struct KeyboardEventData {
    pub key_code: i32,
    pub char_code: u32,
    pub code: String,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub alt_graph_key: bool,
    pub is_composing: bool,
    /// Se `preventDefault()` foi chamado.
    pub default_prevented: Cell<bool>,
    /// Se `stopPropagation()` foi chamado.
    pub propagation_stopped: Cell<bool>,
}

impl KeyboardEventData {
    /// `preventDefault()`.
    pub fn prevent_default(&self) {
        self.default_prevented.set(true);
    }

    /// `defaultPrevented`.
    pub fn is_default_prevented(&self) -> bool {
        self.default_prevented.get()
    }

    /// `stopPropagation()`.
    pub fn stop_propagation(&self) {
        self.propagation_stopped.set(true);
    }

    /// Se `stopPropagation()` foi chamado.
    pub fn is_propagation_stopped(&self) -> bool {
        self.propagation_stopped.get()
    }
}

/// `IBrowser`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IBrowser {
    pub is_android: bool,
    pub is_firefox: bool,
    pub is_chrome: bool,
    pub is_safari: bool,
}

impl IBrowser {
    /// `browser.isWebKit` (o user agent com `AppleWebKit`): Chrome e Safari.
    pub fn is_web_kit(&self) -> bool {
        self.is_chrome || self.is_safari
    }
}

/// `extractKeyCode(e)`: o [`KeyCode`] de um `KeyboardEvent`, com as esquisitices de cada navegador.
fn extract_key_code(e: &KeyboardEventData, os: OperatingSystem, browser: &IBrowser) -> KeyCode {
    if e.char_code != 0 {
        // "keypress" events mostly; `String.fromCharCode` trunca para 16 bits
        let character = JsString::from_units(vec![e.char_code as u16]).to_upper_case().to_string_lossy();
        return KeyCodeUtils::from_string(&character);
    }

    let key_code = e.key_code;
    let is_macintosh = os == OperatingSystem::Macintosh;
    let is_linux = os == OperatingSystem::Linux;

    // browser quirks
    if key_code == 3 {
        return KeyCode::PauseBreak;
    }
    if browser.is_firefox {
        match key_code {
            59 => return KeyCode::Semicolon,
            60 if is_linux => return KeyCode::IntlBackslash,
            61 => return KeyCode::Equal,
            // based on: https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent/keyCode#numpad_keys
            107 => return KeyCode::NumpadAdd,
            109 => return KeyCode::NumpadSubtract,
            173 => return KeyCode::Minus,
            224 if is_macintosh => return KeyCode::Meta,
            _ => {}
        }
    } else if browser.is_web_kit() {
        if is_macintosh && key_code == 93 {
            // the two meta keys in the Mac have different key codes (91 and 93)
            return KeyCode::Meta;
        } else if !is_macintosh && key_code == 92 {
            return KeyCode::Meta;
        }
    }

    // cross browser keycodes:
    event_key_code(key_code).unwrap_or(KeyCode::Unknown)
}

/// `StandardKeyboardEvent` (`IKeyboardEvent`).
#[derive(Clone, Debug)]
pub struct StandardKeyboardEvent {
    pub browser_event: Rc<KeyboardEventData>,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub alt_graph_key: bool,
    pub key_code: KeyCode,
    pub code: String,
    as_keybinding: u32,
}

impl StandardKeyboardEvent {
    pub fn new(source: Rc<KeyboardEventData>, os: OperatingSystem, browser: &IBrowser) -> Self {
        // Browsers are inconsistent while an IME composition is in flight: most keystrokes arrive as
        // `keyCode: 229` (which maps to `KEY_IN_COMPOSITION`), but some platform/IME combinations
        // report the real key code for keys the IME owns. Normalize to `KEY_IN_COMPOSITION`.
        let key_code = if source.is_composing { KeyCode::KeyInComposition } else { extract_key_code(&source, os, browser) };

        let ctrl_key = source.ctrl_key || key_code == KeyCode::Ctrl;
        let alt_key = source.alt_key || key_code == KeyCode::Alt;
        let shift_key = source.shift_key || key_code == KeyCode::Shift;
        let meta_key = source.meta_key || key_code == KeyCode::Meta;

        // `_computeKeybinding`
        let (ctrl_key_mod, meta_key_mod) =
            if os == OperatingSystem::Macintosh { (KeyMod::WIN_CTRL, KeyMod::CTRL_CMD) } else { (KeyMod::CTRL_CMD, KeyMod::WIN_CTRL) };
        let key = if is_modifier_key(key_code) { KeyCode::Unknown } else { key_code };
        let mut as_keybinding = 0;
        if ctrl_key {
            as_keybinding |= ctrl_key_mod;
        }
        if alt_key {
            as_keybinding |= KeyMod::ALT;
        }
        if shift_key {
            as_keybinding |= KeyMod::SHIFT;
        }
        if meta_key {
            as_keybinding |= meta_key_mod;
        }
        as_keybinding |= key.bits();

        Self {
            ctrl_key,
            shift_key,
            alt_key,
            meta_key,
            alt_graph_key: source.alt_graph_key,
            key_code,
            code: source.code.clone(),
            as_keybinding,
            browser_event: source,
        }
    }

    /// `preventDefault()`.
    pub fn prevent_default(&self) {
        self.browser_event.prevent_default();
    }

    /// `stopPropagation()`.
    pub fn stop_propagation(&self) {
        self.browser_event.stop_propagation();
    }

    /// `toKeyCodeChord()`.
    pub fn to_key_code_chord(&self) -> KeyCodeChord {
        let key = if is_modifier_key(self.key_code) { KeyCode::Unknown } else { self.key_code };
        KeyCodeChord::new(self.ctrl_key, self.shift_key, self.alt_key, self.meta_key, key)
    }

    /// `equals(other)`: a tecla e os modificadores como número de `KeyMod | KeyCode`.
    pub fn equals(&self, other: u32) -> bool {
        self.as_keybinding == other
    }
}

/// `IAccessibilityService`: o que o `TextAreaInput` consome do serviço.
pub trait IAccessibilityService {
    /// `isScreenReaderOptimized()`.
    fn is_screen_reader_optimized(&self) -> bool;
    /// `onDidChangeScreenReaderOptimized`.
    fn on_did_change_screen_reader_optimized(&self) -> Event<()>;
}

/// `ITextAreaInputHost`.
pub trait ITextAreaInputHost {
    /// `getScreenReaderContent()`.
    fn get_screen_reader_content(&self) -> TextAreaState;

    /// `deduceModelPosition(viewAnchorPosition, deltaOffset, lineFeedCnt)`.
    fn deduce_model_position(&self, view_anchor_position: Position, delta_offset: i64, line_feed_cnt: usize) -> Position;

    /// `createClipboardCopyEvent(e, isCut, host.context, logService, isFirefox)`: o `context` do host só entra
    /// aqui (ver "Divergências").
    fn create_clipboard_copy_event(&self, e: Rc<ClipboardEvent>, is_cut: bool, is_firefox: bool) -> ClipboardCopyEvent;
}

/// `ICompleteTextAreaWrapper`: o [`ITextAreaWrapper`] mais o que o `TextAreaInput` precisa da textarea além de
/// valor e seleção. Os eventos do original (`onKeyDown`...) são entregues aos métodos `handle_*` do
/// [`TextAreaInput`] (ver "Divergências").
pub trait ICompleteTextAreaWrapper: ITextAreaWrapper {
    /// `setIgnoreSelectionChangeTime(reason)`.
    fn set_ignore_selection_change_time(&mut self, reason: &str);
    /// `getIgnoreSelectionChangeTime()`: o `Date.now()` da última vez que o `TextAreaInput` mexeu na textarea.
    fn get_ignore_selection_change_time(&self) -> u64;
    /// `resetSelectionChangeTime()`.
    fn reset_selection_change_time(&mut self);

    /// `hasFocus()`.
    fn has_focus(&self) -> bool;
}

/// `CompositionContext`.
struct CompositionContext {
    last_type_text_length: Cell<usize>,
}

impl CompositionContext {
    fn new() -> Self {
        Self { last_type_text_length: Cell::new(0) }
    }

    fn handle_composition_update(&self, text: &JsString) -> ITypeData {
        let type_input = ITypeData {
            text: text.clone(),
            replace_prev_char_cnt: self.last_type_text_length.get(),
            replace_next_char_cnt: 0,
            position_delta: 0,
        };
        self.last_type_text_length.set(text.len());
        type_input
    }
}

/// Um emissor registrado em `base` (o `this._register(new Emitter<T>())`) e o seu evento (o `onFoo`).
fn registered_emitter<T: 'static>(base: &Disposable) -> (Rc<Emitter<T>>, Event<T>) {
    let emitter = Emitter::new();
    base.register(emitter.clone());
    let event = emitter.event();
    (emitter, event)
}

/// `TextAreaInput`.
pub struct TextAreaInput {
    base: Rc<Disposable>,
    host: Rc<dyn ITextAreaInputHost>,
    text_area: Rc<RefCell<dyn ICompleteTextAreaWrapper>>,
    os: OperatingSystem,
    browser: IBrowser,
    accessibility_service: Rc<dyn IAccessibilityService>,
    weak_self: Weak<TextAreaInput>,

    focus_emitter: Rc<Emitter<()>>,
    blur_emitter: Rc<Emitter<()>>,
    key_down_emitter: Rc<Emitter<StandardKeyboardEvent>>,
    key_up_emitter: Rc<Emitter<StandardKeyboardEvent>>,
    cut_emitter: Rc<Emitter<()>>,
    paste_emitter: Rc<Emitter<IPasteData>>,
    will_copy_emitter: Rc<Emitter<Rc<ClipboardCopyEvent>>>,
    will_cut_emitter: Rc<Emitter<Rc<ClipboardCopyEvent>>>,
    will_paste_emitter: Rc<Emitter<Rc<ClipboardPasteEvent>>>,
    type_emitter: Rc<Emitter<ITypeData>>,
    composition_start_emitter: Rc<Emitter<ICompositionStartEvent>>,
    composition_update_emitter: Rc<Emitter<ICompositionData>>,
    composition_end_emitter: Rc<Emitter<()>>,
    selection_change_request_emitter: Rc<Emitter<Selection>>,

    /// `onFocus`.
    pub on_focus: Event<()>,
    /// `onBlur`.
    pub on_blur: Event<()>,
    /// `onKeyDown`.
    pub on_key_down: Event<StandardKeyboardEvent>,
    /// `onKeyUp`.
    pub on_key_up: Event<StandardKeyboardEvent>,
    /// `onCut`.
    pub on_cut: Event<()>,
    /// `onPaste`.
    pub on_paste: Event<IPasteData>,
    /// `onWillCopy`.
    pub on_will_copy: Event<Rc<ClipboardCopyEvent>>,
    /// `onWillCut`.
    pub on_will_cut: Event<Rc<ClipboardCopyEvent>>,
    /// `onWillPaste`.
    pub on_will_paste: Event<Rc<ClipboardPasteEvent>>,
    /// `onType`.
    pub on_type: Event<ITypeData>,
    /// `onCompositionStart`.
    pub on_composition_start: Event<ICompositionStartEvent>,
    /// `onCompositionUpdate`.
    pub on_composition_update: Event<ICompositionData>,
    /// `onCompositionEnd`.
    pub on_composition_end: Event<()>,
    /// `onSelectionChangeRequest`.
    pub on_selection_change_request: Event<Selection>,

    async_trigger_cut: Rc<RunOnceScheduler>,
    async_focus_gain_write_screen_reader_content: Rc<MutableDisposable<RunOnceScheduler>>,

    text_area_state: RefCell<TextAreaState>,
    /// Se o listener de `selectionchange` do `document` está instalado (o `_selectionChangeListener`).
    selection_change_listener_installed: Cell<bool>,
    /// O `previousSelectionChangeEventTime` do listener instalado.
    previous_selection_change_event_time: Cell<i64>,
    has_focus: Cell<bool>,
    current_composition: RefCell<Option<Rc<CompositionContext>>>,
    /// O `lastKeyDown` do construtor.
    last_key_down: RefCell<Option<StandardKeyboardEvent>>,
}

impl TextAreaInput {
    pub fn new(
        host: Rc<dyn ITextAreaInputHost>,
        text_area: Rc<RefCell<dyn ICompleteTextAreaWrapper>>,
        os: OperatingSystem,
        browser: IBrowser,
        accessibility_service: Rc<dyn IAccessibilityService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let (focus_emitter, on_focus) = registered_emitter(&base);
        let (blur_emitter, on_blur) = registered_emitter(&base);
        let (key_down_emitter, on_key_down) = registered_emitter(&base);
        let (key_up_emitter, on_key_up) = registered_emitter(&base);
        let (cut_emitter, on_cut) = registered_emitter(&base);
        let (paste_emitter, on_paste) = registered_emitter(&base);
        let (will_copy_emitter, on_will_copy) = registered_emitter(&base);
        let (will_cut_emitter, on_will_cut) = registered_emitter(&base);
        let (will_paste_emitter, on_will_paste) = registered_emitter(&base);
        let (type_emitter, on_type) = registered_emitter(&base);
        let (composition_start_emitter, on_composition_start) = registered_emitter(&base);
        let (composition_update_emitter, on_composition_update) = registered_emitter(&base);
        let (composition_end_emitter, on_composition_end) = registered_emitter(&base);
        let (selection_change_request_emitter, on_selection_change_request) = registered_emitter(&base);

        let async_trigger_cut = RunOnceScheduler::new(
            {
                let cut_emitter = cut_emitter.clone();
                move || cut_emitter.fire(())
            },
            0,
        );
        base.register(async_trigger_cut.clone());
        let async_focus_gain_write_screen_reader_content = MutableDisposable::<RunOnceScheduler>::new();
        base.register(async_focus_gain_write_screen_reader_content.clone());

        let this = Rc::new_cyclic(|weak_self| TextAreaInput {
            base,
            host,
            text_area,
            os,
            browser,
            accessibility_service,
            weak_self: weak_self.clone(),
            focus_emitter,
            blur_emitter,
            key_down_emitter,
            key_up_emitter,
            cut_emitter,
            paste_emitter,
            will_copy_emitter,
            will_cut_emitter,
            will_paste_emitter,
            type_emitter,
            composition_start_emitter,
            composition_update_emitter,
            composition_end_emitter,
            selection_change_request_emitter,
            on_focus,
            on_blur,
            on_key_down,
            on_key_up,
            on_cut,
            on_paste,
            on_will_copy,
            on_will_cut,
            on_will_paste,
            on_type,
            on_composition_start,
            on_composition_update,
            on_composition_end,
            on_selection_change_request,
            async_trigger_cut,
            async_focus_gain_write_screen_reader_content,
            text_area_state: RefCell::new(TextAreaState::EMPTY),
            selection_change_listener_installed: Cell::new(false),
            previous_selection_change_event_time: Cell::new(0),
            has_focus: Cell::new(false),
            current_composition: RefCell::new(None),
            last_key_down: RefCell::new(None),
        });

        if this.accessibility_service.is_screen_reader_optimized() {
            this.write_native_text_area_content("ctor");
        }
        let weak = Rc::downgrade(&this);
        let subscription = this.accessibility_service.on_did_change_screen_reader_optimized().run_and_subscribe(
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_async_focus_gain_scheduler();
                }
            },
            None,
        );
        this.base.register(subscription);
        this
    }

    /// O `textAreaState` (getter).
    pub fn text_area_state(&self) -> TextAreaState {
        self.text_area_state.borrow().clone()
    }

    /// O corpo do `runAndSubscribe` do construtor: liga o agendador de escrita ao ganhar o foco enquanto um leitor
    /// de tela está em uso, e o desliga nos demais casos (inclusive quando já estava ligado, como no original).
    fn update_async_focus_gain_scheduler(&self) {
        if self.accessibility_service.is_screen_reader_optimized()
            && self.async_focus_gain_write_screen_reader_content.value().is_none()
        {
            // Don't register the scheduler: the MutableDisposable already handles cleanup
            self.async_focus_gain_write_screen_reader_content.set(Some(self.create_async_focus_gain_scheduler()));
        } else {
            self.async_focus_gain_write_screen_reader_content.clear();
        }
    }

    /// `new RunOnceScheduler(() => this.writeNativeTextAreaContent('asyncFocusGain'), 0)`.
    fn create_async_focus_gain_scheduler(&self) -> Rc<RunOnceScheduler> {
        let weak = self.weak_self.clone();
        RunOnceScheduler::new(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.write_native_text_area_content("asyncFocusGain");
                }
            },
            0,
        )
    }

    /// `TextAreaState.readFromTextArea(this._textArea, this._textAreaState)`.
    fn read_text_area_state(&self) -> TextAreaState {
        let text_area = self.text_area.borrow();
        TextAreaState::read_from_text_area(&*text_area, Some(&self.text_area_state.borrow()))
    }

    /// `keydown`.
    pub fn handle_key_down(&self, source: Rc<KeyboardEventData>) {
        let e = StandardKeyboardEvent::new(source, self.os, &self.browser);
        if e.key_code == KeyCode::KeyInComposition
            || (self.current_composition.borrow().is_some() && e.key_code == KeyCode::Backspace)
        {
            // Stop propagation for keyDown events if the IME is processing key input
            e.stop_propagation();
        }

        if e.equals(KeyCode::Escape.bits()) {
            // Prevent default always for `Esc`, otherwise it will generate a keypress
            // See https://msdn.microsoft.com/en-us/library/ie/ms536939(v=vs.85).aspx
            e.prevent_default();
        }

        *self.last_key_down.borrow_mut() = Some(e.clone());
        self.key_down_emitter.fire(e);
    }

    /// `keyup`.
    pub fn handle_key_up(&self, source: Rc<KeyboardEventData>) {
        self.key_up_emitter.fire(StandardKeyboardEvent::new(source, self.os, &self.browser));
    }

    /// `compositionstart`.
    pub fn handle_composition_start(&self, e: &CompositionEvent) {
        let current_composition = Rc::new(CompositionContext::new());
        {
            let mut slot = self.current_composition.borrow_mut();
            if slot.is_some() {
                // simply reset the composition context
                *slot = Some(current_composition);
                return;
            }
            *slot = Some(current_composition.clone());
        }

        let is_long_press_with_arrow_key = self.os == OperatingSystem::Macintosh
            && self.last_key_down.borrow().as_ref().is_some_and(|last_key_down| {
                let state = self.text_area_state.borrow();
                last_key_down.equals(KeyCode::KeyInComposition.bits())
                    && state.selection_start == state.selection_end
                    && state.selection_start > 0
                    && substring(state.value.units(), state.selection_start as i64 - 1, state.selection_start as i64)
                        == e.data.units()
                    && (last_key_down.code == "ArrowRight" || last_key_down.code == "ArrowLeft")
            });
        if is_long_press_with_arrow_key {
            // Handling long press case on Chromium/Safari macOS + arrow key => pretend the character was selected.
            // Pretend the previous character was composed (in order to get it removed by subsequent compositionupdate events)
            current_composition.handle_composition_update(&JsString::from("x"));
            self.composition_start_emitter.fire(ICompositionStartEvent { data: e.data.clone() });
            return;
        }

        // On Android, tapping on the editor enters composition mode to edit the current word, so the textarea
        // cannot be cleared there and the current word must be treated as selected: nothing to do but report it.
        self.composition_start_emitter.fire(ICompositionStartEvent { data: e.data.clone() });
    }

    /// No Android o `data` de `compositionupdate` e de `compositionend` é inútil (por exemplo, com o cursor no
    /// meio de `Mic|osoft` e `Microsoft` escolhido nas sugestões, o `data` traz `Microsoft`, sem dizer onde a
    /// edição começou e terminou): o que foi digitado vem da diferença entre os estados da textarea.
    fn apply_android_composition_input(&self) {
        let new_state = self.read_text_area_state();
        let type_input = TextAreaState::deduce_android_composition_input(&self.text_area_state.borrow(), &new_state);
        *self.text_area_state.borrow_mut() = new_state;
        self.type_emitter.fire(type_input);
    }

    /// O que `compositionupdate` e `compositionend` fazem fora do Android: o que foi digitado vem do `data`.
    fn apply_composition_data(&self, current_composition: &CompositionContext, data: &JsString) {
        let type_input = current_composition.handle_composition_update(data);
        let new_state = self.read_text_area_state();
        *self.text_area_state.borrow_mut() = new_state;
        self.type_emitter.fire(type_input);
    }

    /// `compositionupdate`.
    pub fn handle_composition_update(&self, e: &CompositionEvent) {
        let current_composition = self.current_composition.borrow().clone();
        let Some(current_composition) = current_composition else {
            // should not be possible to receive a 'compositionupdate' without a 'compositionstart'
            return;
        };
        if self.browser.is_android {
            self.apply_android_composition_input();
        } else {
            self.apply_composition_data(&current_composition, &e.data);
        }
        self.composition_update_emitter.fire(e.clone());
    }

    /// `compositionend`.
    pub fn handle_composition_end(&self, e: &CompositionEvent) {
        let current_composition = self.current_composition.borrow_mut().take();
        let Some(current_composition) = current_composition else {
            // https://github.com/microsoft/monaco-editor/issues/1663
            // On iOS 13.2, Chinese system IME randomly trigger an additional compositionend event with empty data
            return;
        };
        if self.browser.is_android {
            self.apply_android_composition_input();
        } else {
            self.apply_composition_data(&current_composition, &e.data);
        }
        self.composition_end_emitter.fire(());
    }

    /// `input`.
    pub fn handle_input(&self, e: &InputEvent) {
        // Pretend here we touched the text area, as the `input` event will most likely
        // result in a `selectionchange` event which we want to ignore
        self.text_area.borrow_mut().set_ignore_selection_change_time("received input event");

        if self.current_composition.borrow().is_some() {
            return;
        }

        let new_state = self.read_text_area_state();
        let type_input =
            TextAreaState::deduce_input(&self.text_area_state.borrow(), &new_state, self.os == OperatingSystem::Macintosh);

        if type_input.replace_prev_char_cnt == 0 && type_input.text.len() == 1 {
            // one character was typed
            let unit = type_input.text.units()[0];
            let is_delete = unit == 0x7f;
            if is_high_surrogate(unit) || is_delete {
                // Ignore invalid input but keep it around for next time
                return;
            }
        }

        *self.text_area_state.borrow_mut() = new_state;
        if !type_input.text.is_empty()
            || type_input.replace_prev_char_cnt != 0
            || type_input.replace_next_char_cnt != 0
            || type_input.position_delta != 0
        {
            // https://w3c.github.io/input-events/#interface-InputEvent-Attributes
            if e.input_type == "insertFromPaste" {
                let metadata = in_memory_clipboard_metadata_manager().get(&type_input.text);
                self.paste_emitter.fire(IPasteData { text: type_input.text, metadata });
            } else {
                self.type_emitter.fire(type_input);
            }
        }
    }

    // --- Clipboard operations

    /// `cut`.
    pub fn handle_cut(&self, e: Rc<ClipboardEvent>) {
        // Fire onWillCut event to allow interception
        let cut_event = Rc::new(self.host.create_clipboard_copy_event(e, true, self.browser.is_firefox));
        self.will_cut_emitter.fire(cut_event.clone());
        if cut_event.is_handled() {
            // Event was handled externally, skip default processing
            return;
        }

        // Pretend here we touched the text area, as the `cut` event will most likely
        // result in a `selectionchange` event which we want to ignore
        self.text_area.borrow_mut().set_ignore_selection_change_time("received cut event");

        cut_event.ensure_clipboard_gets_editor_data();
        self.async_trigger_cut.schedule(None);
    }

    /// `copy`.
    pub fn handle_copy(&self, e: Rc<ClipboardEvent>) {
        // !!!!!
        // This is a workaround for what we think is an Electron bug where
        // execCommand('copy') does not always work (it does not fire a clipboard event)
        // !!!!!
        // We signal that we have executed a copy command
        CopyOptions::set_electron_bug_workaround_copy_event_has_fired(true);

        // Fire onWillCopy event to allow interception
        let copy_event = Rc::new(self.host.create_clipboard_copy_event(e, false, self.browser.is_firefox));
        self.will_copy_emitter.fire(copy_event.clone());
        if copy_event.is_handled() {
            // Event was handled externally, skip default processing
            return;
        }

        copy_event.ensure_clipboard_gets_editor_data();
    }

    /// `paste`.
    pub fn handle_paste(&self, e: Rc<ClipboardEvent>) {
        // Fire onWillPaste event to allow interception
        let paste_event = Rc::new(create_clipboard_paste_event(e.clone()));
        self.will_paste_emitter.fire(paste_event.clone());
        if paste_event.is_handled() {
            // Event was handled externally, skip default processing
            return;
        }

        // Pretend here we touched the text area, as the `paste` event will most likely
        // result in a `selectionchange` event which we want to ignore
        self.text_area.borrow_mut().set_ignore_selection_change_time("received paste event");

        e.prevent_default();

        if paste_event.text.is_empty() {
            return;
        }

        self.paste_emitter.fire(IPasteData { text: paste_event.text.clone(), metadata: paste_event.metadata.clone() });
    }

    /// `focus`.
    pub fn handle_focus(&self) {
        let had_focus = self.has_focus.get();

        self.set_has_focus(true);

        if self.accessibility_service.is_screen_reader_optimized()
            && self.browser.is_safari
            && !had_focus
            && self.has_focus.get()
        {
            // When "tabbing into" the textarea, immediately after dispatching the 'focus' event,
            // Safari will always move the selection at offset 0 in the textarea
            if self.async_focus_gain_write_screen_reader_content.value().is_none() {
                self.async_focus_gain_write_screen_reader_content.set(Some(self.create_async_focus_gain_scheduler()));
            }
            if let Some(scheduler) = self.async_focus_gain_write_screen_reader_content.value() {
                scheduler.schedule(None);
            }
        }
    }

    /// `blur`.
    pub fn handle_blur(&self) {
        self.end_composition_without_composition_end("blurWithoutCompositionEnd");
        self.set_has_focus(false);
    }

    /// `-monaco-textarea-synthetic-tap`.
    pub fn handle_synthetic_tap(&self) {
        if self.browser.is_android {
            // on Android, tapping does not cancel the current composition, so the
            // textarea is stuck showing the old composition
            self.end_composition_without_composition_end("tapWithoutCompositionEnd");
        }
    }

    /// O que `blur` e o toque no Android fazem com uma composição que ficou sem `compositionend` (ver
    /// https://github.com/microsoft/vscode/issues/112621, onde o `compositionend` não vem quando o editor sai
    /// do DOM durante a composição).
    fn end_composition_without_composition_end(&self, reason: &str) {
        let had_composition = self.current_composition.borrow_mut().take().is_some();
        if had_composition {
            // The flag was cleared to be able to write to the textarea: clear the textarea to avoid an
            // unwanted cursor type
            self.write_native_text_area_content(reason);

            // Fire artificial composition end
            self.composition_end_emitter.fire(());
        }
    }

    /// `_initializeFromTest()`: só para os testes.
    pub fn initialize_from_test(&self) {
        self.has_focus.set(true);
        let new_state = TextAreaState::read_from_text_area(&*self.text_area.borrow(), None);
        *self.text_area_state.borrow_mut() = new_state;
    }

    /// O deslocamento da posição do editor (âncora, delta e quebras de linha) de `offset` na textarea, levado
    /// ao modelo pelo host.
    fn deduce_model_position(&self, state: &TextAreaState, offset: usize) -> Position {
        let (anchor, delta_offset, line_feed_cnt) = state.deduce_editor_position(offset);
        let anchor = anchor.expect("o estado tem seleção do editor, então a âncora existe");
        self.host.deduce_model_position(anchor, delta_offset, line_feed_cnt)
    }

    /// O listener de `selectionchange` que `_installSelectionChangeListener` registra no documento.
    ///
    /// See https://github.com/microsoft/vscode/issues/27216 and https://github.com/microsoft/vscode/issues/98256
    /// When using a Braille display, it is possible for users to reposition the system caret. This is reflected
    /// in Chrome as a `selectionchange` event.
    ///
    /// The `selectionchange` event appears to be emitted under numerous other circumstances, so it is quite a
    /// challenge to distinguish a `selectionchange` coming in from a user using a Braille display from all the
    /// other cases. The problems with the `selectionchange` event are:
    ///  * the event is emitted when the textarea is focused programmatically
    ///  * the event is emitted when the selection is changed in the textarea programmatically
    ///  * the event is emitted when the value of the textarea is changed programmatically
    ///  * the event is emitted when tabbing into the textarea
    ///  * the event is emitted asynchronously (sometimes with a delay as high as a few tens of ms)
    ///  * the event sometimes comes in bursts for a single logical textarea operation
    ///
    /// `selectionchange` events often come multiple times for a single logical change, so multiple events that
    /// burst in a short period of time are throttled.
    pub fn handle_selection_change(&self) {
        if !self.selection_change_listener_installed.get() {
            return;
        }

        if !self.has_focus.get() {
            return;
        }
        if self.current_composition.borrow().is_some() {
            return;
        }
        if !self.browser.is_chrome {
            // Support only for Chrome until testing happens on other browsers
            return;
        }

        let now = date_now() as i64;

        let delta1 = now - self.previous_selection_change_event_time.get();
        self.previous_selection_change_event_time.set(now);
        if delta1 < 5 {
            // received another `selectionchange` event within 5ms of the previous `selectionchange` event
            // => ignore it
            return;
        }

        let delta2 = now - self.text_area.borrow().get_ignore_selection_change_time() as i64;
        self.text_area.borrow_mut().reset_selection_change_time();
        if delta2 < 100 {
            // received a `selectionchange` event within 100ms since we touched the textarea
            // => ignore it, since we caused it
            return;
        }

        let state = self.text_area_state.borrow().clone();
        if state.selection.is_none() {
            // Cannot correlate a position in the textarea with a position in the editor...
            return;
        }

        let new_value = self.text_area.borrow().get_value();
        if state.value != new_value {
            // Cannot correlate a position in the textarea with a position in the editor...
            return;
        }

        let new_selection_start = self.text_area.borrow().get_selection_start();
        let new_selection_end = self.text_area.borrow().get_selection_end();
        if state.selection_start == new_selection_start && state.selection_end == new_selection_end {
            // Nothing to do...
            return;
        }

        let new_selection_start_position = self.deduce_model_position(&state, new_selection_start);
        let new_selection_end_position = self.deduce_model_position(&state, new_selection_end);

        self.selection_change_request_emitter.fire(Selection::new(
            new_selection_start_position.line_number,
            new_selection_start_position.column,
            new_selection_end_position.line_number,
            new_selection_end_position.column,
        ));
    }

    /// `focusTextArea()`.
    pub fn focus_text_area(&self) {
        // Setting this._hasFocus and writing the screen reader content
        // will result in a focus() and setSelectionRange() in the textarea
        self.set_has_focus(true);

        // If the editor is off DOM, focus cannot be really set, so let's double check that we have managed to set the focus
        self.refresh_focus_state();
    }

    /// `isFocused()`.
    pub fn is_focused(&self) -> bool {
        self.has_focus.get()
    }

    /// `refreshFocusState()`.
    pub fn refresh_focus_state(&self) {
        let has_focus = self.text_area.borrow().has_focus();
        self.set_has_focus(has_focus);
    }

    /// `_setHasFocus(newHasFocus)`.
    fn set_has_focus(&self, new_has_focus: bool) {
        if self.has_focus.get() == new_has_focus {
            // no change
            return;
        }
        self.has_focus.set(new_has_focus);

        // `_installSelectionChangeListener` recomeça o `previousSelectionChangeEventTime` em zero
        self.selection_change_listener_installed.set(new_has_focus);
        if new_has_focus {
            self.previous_selection_change_event_time.set(0);
        }

        if new_has_focus {
            self.write_native_text_area_content("focusgain");
        }

        if new_has_focus {
            self.focus_emitter.fire(());
        } else {
            self.blur_emitter.fire(());
        }
    }

    /// `_setAndWriteTextAreaState(reason, textAreaState)`.
    fn set_and_write_text_area_state(&self, reason: &str, text_area_state: TextAreaState) {
        let has_focus = self.has_focus.get();
        let text_area_state = if has_focus { text_area_state } else { text_area_state.collapse_selection() };
        text_area_state.write_to_text_area(reason, &mut *self.text_area.borrow_mut(), has_focus);
        *self.text_area_state.borrow_mut() = text_area_state;
    }

    /// `writeNativeTextAreaContent(reason)`.
    pub fn write_native_text_area_content(&self, reason: &str) {
        if (!self.accessibility_service.is_screen_reader_optimized() && reason == "render")
            || self.current_composition.borrow().is_some()
        {
            // Do not write to the text on render unless a screen reader is being used #192278
            // Do not write to the text area when doing composition
            return;
        }
        self.set_and_write_text_area_state(reason, self.host.get_screen_reader_content());
    }
}

impl IDisposable for TextAreaInput {
    fn dispose(&self) {
        self.base.dispose();
        self.selection_change_listener_installed.set(false);
    }
}

/// A direção da seleção de uma textarea (`selectionDirection`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAreaSelectionDirection {
    Forward,
    Backward,
    None,
}

/// O `HTMLTextAreaElement` que o [`TextAreaWrapper`] envolve: o que o código lê e escreve do elemento.
pub trait INativeTextArea {
    /// `value`.
    fn value(&self) -> JsString;
    /// `value = ...`.
    fn set_value(&mut self, value: &[u16]);
    /// `selectionStart`.
    fn selection_start(&self) -> usize;
    /// `selectionEnd`.
    fn selection_end(&self) -> usize;
    /// `selectionDirection`.
    fn selection_direction(&self) -> TextAreaSelectionDirection;
    /// `setSelectionRange(selectionStart, selectionEnd)`.
    fn set_selection_range(&mut self, selection_start: usize, selection_end: usize);
    /// `focus()`.
    fn focus(&mut self);
    /// O elemento ativo (`shadowRoot.activeElement` se há shadow root, senão `getActiveElement()`) é a textarea.
    fn is_active_element(&self) -> bool;
    /// `dom.getShadowRoot(textArea)` existe.
    fn is_in_shadow_root(&self) -> bool;
    /// `isConnected`.
    fn is_connected(&self) -> bool;
    /// `activeWindow.parent !== activeWindow`: a janela ativa está dentro de um iframe.
    fn is_in_nested_window(&self) -> bool;
    /// O `try` de `setSelectionRange` com o foco fora da textarea: `saveParentsScrollTop`, `focus()`,
    /// `setSelectionRange(...)` e `restoreParentsScrollTop`, desfazendo o "revelar" desesperado do navegador.
    /// Uma textarea fora do DOM pode lançar; a implementação engole o erro.
    fn focus_and_set_selection_range(&mut self, selection_start: usize, selection_end: usize);
}

/// `TextAreaWrapper`: o [`ICompleteTextAreaWrapper`] sobre uma textarea nativa.
pub struct TextAreaWrapper<N: INativeTextArea> {
    actual: N,
    is_firefox: bool,
    ignore_selection_change_time: u64,
}

impl<N: INativeTextArea> TextAreaWrapper<N> {
    pub fn new(actual: N, browser: &IBrowser) -> Self {
        Self { actual, is_firefox: browser.is_firefox, ignore_selection_change_time: 0 }
    }

    /// `browser.isFirefox && activeWindow.parent !== activeWindow`: a textarea tem de ser refocada.
    fn needs_firefox_iframe_refocus(&self) -> bool {
        self.is_firefox && self.actual.is_in_nested_window()
    }
}

impl<N: INativeTextArea> ITextAreaWrapper for TextAreaWrapper<N> {
    fn get_value(&self) -> JsString {
        self.actual.value()
    }

    fn set_value(&mut self, _reason: &str, value: &[u16]) {
        if self.actual.value().units() == value {
            // No change
            return;
        }
        self.set_ignore_selection_change_time("setValue");
        self.actual.set_value(value);
    }

    fn get_selection_start(&self) -> usize {
        if self.actual.selection_direction() == TextAreaSelectionDirection::Backward {
            self.actual.selection_end()
        } else {
            self.actual.selection_start()
        }
    }

    fn get_selection_end(&self) -> usize {
        if self.actual.selection_direction() == TextAreaSelectionDirection::Backward {
            self.actual.selection_start()
        } else {
            self.actual.selection_end()
        }
    }

    fn set_selection_range(&mut self, _reason: &str, selection_start: usize, selection_end: usize) {
        let current_is_focused = self.actual.is_active_element();
        let current_selection_start = self.actual.selection_start();
        let current_selection_end = self.actual.selection_end();

        if current_is_focused && current_selection_start == selection_start && current_selection_end == selection_end {
            // No change
            // Firefox iframe bug https://github.com/microsoft/monaco-editor/issues/643#issuecomment-367871377
            if self.needs_firefox_iframe_refocus() {
                self.actual.focus();
            }
            return;
        }

        if current_is_focused {
            // No need to focus, only need to change the selection range
            self.set_ignore_selection_change_time("setSelectionRange");
            self.actual.set_selection_range(selection_start, selection_end);
            if self.needs_firefox_iframe_refocus() {
                self.actual.focus();
            }
            return;
        }

        // If the focus is outside the textarea, browsers will try really hard to reveal the textarea.
        // Here, we try to undo the browser's desperate reveal.
        self.set_ignore_selection_change_time("setSelectionRange");
        self.actual.focus_and_set_selection_range(selection_start, selection_end);
    }
}

impl<N: INativeTextArea> ICompleteTextAreaWrapper for TextAreaWrapper<N> {
    fn set_ignore_selection_change_time(&mut self, _reason: &str) {
        self.ignore_selection_change_time = date_now();
    }

    fn get_ignore_selection_change_time(&self) -> u64 {
        self.ignore_selection_change_time
    }

    fn reset_selection_change_time(&mut self) {
        self.ignore_selection_change_time = 0;
    }

    fn has_focus(&self) -> bool {
        // com shadow root vale o `shadowRoot.activeElement`; sem ele, só se a textarea está conectada ao DOM
        (self.actual.is_in_shadow_root() || self.actual.is_connected()) && self.actual.is_active_element()
    }
}
