//! Porte de `src/vs/editor/contrib/gotoSymbol/browser/link/clickLinkGesture.ts`: o gesto de "clicar no link" do editor
//! (ctrl/cmd + mouse sobre um símbolo), que decide quando o mouse está sobre um alvo de navegação, quando a tecla de
//! gatilho foi pressionada e quando o clique deve executar a navegação.
//!
//! Representação:
//! - `ClickLinkMouseEvent`, `ClickLinkKeyboardEvent` e `ClickLinkOptions` são structs de dados com os mesmos campos. O
//!   `'ctrlKey' | 'shiftKey' | 'altKey' | 'metaKey'` do modificador é o enum [`TriggerModifier`], e o `hasModifier(e, m)`
//!   lê os quatro booleanos do evento de mouse ([`MouseEvent`]) ou de teclado ([`KeyboardEvent`]).
//! - `ClickLinkOptions.equals` é o `PartialEq` derivado: os cinco campos que ele compara são todos os campos.
//! - `ClickLinkGesture extends Disposable` é [`ClickLinkGesture`], criado por [`ClickLinkGesture::new`] (devolve
//!   `Rc<Self>`: os ouvintes do editor guardam um [`Weak`]). `onMouseMoveOrRelevantKeyDown`, `onExecute` e `onCancel` são
//!   os métodos de mesmo nome (em snake_case) que devolvem o [`Event`]; o par `[ClickLinkMouseEvent, ClickLinkKeyboardEvent |
//!   null]` é a tupla `(ClickLinkMouseEvent, Option<ClickLinkKeyboardEvent>)`.
//! - `extractLineNumberFromMouseEvent?` é o `Rc<dyn Fn>` de [`IClickLinkGestureOptions`]; o padrão lê a linha da posição do
//!   alvo (`0` sem posição).
//! - As opções `multiCursorModifier` (`'altKey' | 'ctrlKey' | 'metaKey'`) e `mouseMiddleClickAction` são as `String`s do
//!   registro de opções do editor; `platform.isMacintosh` é o [`Platform::detect`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::platform::Platform;
use je_platform::keybinding::keybinding::KeyboardEvent;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::registry::{MOUSE_MIDDLE_CLICK_ACTION, MULTI_CURSOR_MODIFIER};
use crate::config::editor_options::{ConfigurationChangedEvent, EditorOption};
use crate::cursor_events::ICursorSelectionChangedEvent;
use crate::editor_browser::{EditorMouseEvent, MouseEvent, MouseTarget};
use crate::multicursor::listen_weak;

/// `'ctrlKey' | 'shiftKey' | 'altKey' | 'metaKey'` (`TriggerModifier`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerModifier {
    CtrlKey,
    ShiftKey,
    AltKey,
    MetaKey,
}

/// `{ ctrlKey: boolean; shiftKey: boolean; altKey: boolean; metaKey: boolean }`: o que `hasModifier` lê de um evento.
#[derive(Clone, Copy)]
struct ModifierKeys {
    ctrl_key: bool,
    shift_key: bool,
    alt_key: bool,
    meta_key: bool,
}

impl From<&MouseEvent> for ModifierKeys {
    fn from(event: &MouseEvent) -> Self {
        Self { ctrl_key: event.ctrl_key, shift_key: event.shift_key, alt_key: event.alt_key, meta_key: event.meta_key }
    }
}

impl From<&KeyboardEvent> for ModifierKeys {
    fn from(event: &KeyboardEvent) -> Self {
        Self { ctrl_key: event.ctrl_key, shift_key: event.shift_key, alt_key: event.alt_key, meta_key: event.meta_key }
    }
}

/// `hasModifier(e, modifier)`.
fn has_modifier(e: ModifierKeys, modifier: TriggerModifier) -> bool {
    match modifier {
        TriggerModifier::CtrlKey => e.ctrl_key,
        TriggerModifier::ShiftKey => e.shift_key,
        TriggerModifier::AltKey => e.alt_key,
        TriggerModifier::MetaKey => e.meta_key,
    }
}

/// `ClickLinkOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClickLinkOptions {
    pub trigger_key: KeyCode,
    pub trigger_modifier: TriggerModifier,
    pub trigger_side_by_side_key: KeyCode,
    pub trigger_side_by_side_modifier: TriggerModifier,
    pub mouse_middle_click_action: String,
}

impl ClickLinkOptions {
    pub fn new(
        trigger_key: KeyCode,
        trigger_modifier: TriggerModifier,
        trigger_side_by_side_key: KeyCode,
        trigger_side_by_side_modifier: TriggerModifier,
        mouse_middle_click_action: &str,
    ) -> Self {
        Self {
            trigger_key,
            trigger_modifier,
            trigger_side_by_side_key,
            trigger_side_by_side_modifier,
            mouse_middle_click_action: mouse_middle_click_action.to_owned(),
        }
    }
}

/// An event that encapsulates the various trigger modifiers logic needed for go to definition (`ClickLinkMouseEvent`).
#[derive(Clone, Debug)]
pub struct ClickLinkMouseEvent {
    pub target: MouseTarget,
    pub has_trigger_modifier: bool,
    pub has_side_by_side_modifier: bool,
    pub is_none_or_single_mouse_down: bool,
    pub is_left_click: bool,
    pub is_middle_click: bool,
    pub is_right_click: bool,
    pub mouse_middle_click_action: String,
}

impl ClickLinkMouseEvent {
    pub fn new(source: &EditorMouseEvent, opts: &ClickLinkOptions) -> Self {
        let modifiers = ModifierKeys::from(&source.event);
        let mut is_left_click = source.event.left_button;
        let mut is_middle_click = source.event.middle_button;
        let is_right_click = source.event.right_button;
        let mut has_trigger_modifier = has_modifier(modifiers, opts.trigger_modifier);

        if is_middle_click && opts.mouse_middle_click_action == "ctrlLeftClick" {
            // Redirect middle click to left click with modifier
            is_middle_click = false;
            is_left_click = true;
            has_trigger_modifier = true;
        }

        Self {
            target: source.target.clone(),
            has_trigger_modifier,
            has_side_by_side_modifier: has_modifier(modifiers, opts.trigger_side_by_side_modifier),
            is_none_or_single_mouse_down: source.event.detail <= 1,
            is_left_click,
            is_middle_click,
            is_right_click,
            mouse_middle_click_action: opts.mouse_middle_click_action.clone(),
        }
    }
}

/// An event that encapsulates the various trigger modifiers logic needed for go to definition (`ClickLinkKeyboardEvent`).
#[derive(Clone, Debug)]
pub struct ClickLinkKeyboardEvent {
    pub key_code_is_trigger_key: bool,
    pub key_code_is_side_by_side_key: bool,
    pub has_trigger_modifier: bool,
}

impl ClickLinkKeyboardEvent {
    pub fn new(source: &KeyboardEvent, opts: &ClickLinkOptions) -> Self {
        Self {
            key_code_is_trigger_key: source.key_code == opts.trigger_key,
            key_code_is_side_by_side_key: source.key_code == opts.trigger_side_by_side_key,
            has_trigger_modifier: has_modifier(ModifierKeys::from(source), opts.trigger_modifier),
        }
    }
}

/// `createOptions(multiCursorModifier, mouseMiddleClickAction)`.
fn create_options(multi_cursor_modifier: &str, mouse_middle_click_action: &str) -> ClickLinkOptions {
    let is_macintosh = Platform::detect().is_macintosh;
    if multi_cursor_modifier == "altKey" {
        if is_macintosh {
            return ClickLinkOptions::new(KeyCode::Meta, TriggerModifier::MetaKey, KeyCode::Alt, TriggerModifier::AltKey, mouse_middle_click_action);
        }
        return ClickLinkOptions::new(KeyCode::Ctrl, TriggerModifier::CtrlKey, KeyCode::Alt, TriggerModifier::AltKey, mouse_middle_click_action);
    }

    if is_macintosh {
        return ClickLinkOptions::new(KeyCode::Alt, TriggerModifier::AltKey, KeyCode::Meta, TriggerModifier::MetaKey, mouse_middle_click_action);
    }
    ClickLinkOptions::new(KeyCode::Alt, TriggerModifier::AltKey, KeyCode::Ctrl, TriggerModifier::CtrlKey, mouse_middle_click_action)
}

/// `IClickLinkGestureOptions`.
#[derive(Default)]
pub struct IClickLinkGestureOptions {
    /// Return 0 if the mouse event should not be considered.
    pub extract_line_number_from_mouse_event: Option<Rc<dyn Fn(&ClickLinkMouseEvent) -> i32>>,
}

/// `ClickLinkGesture`.
pub struct ClickLinkGesture {
    base: Rc<Disposable>,
    on_mouse_move_or_relevant_key_down: Rc<Emitter<(ClickLinkMouseEvent, Option<ClickLinkKeyboardEvent>)>>,
    on_execute: Rc<Emitter<ClickLinkMouseEvent>>,
    on_cancel: Rc<Emitter<()>>,
    editor: Weak<CodeEditorWidget>,
    extract_line_number_from_mouse_event: Rc<dyn Fn(&ClickLinkMouseEvent) -> i32>,
    opts: RefCell<ClickLinkOptions>,
    last_mouse_move_event: RefCell<Option<ClickLinkMouseEvent>>,
    has_trigger_key_on_mouse_down: Cell<bool>,
    line_number_on_mouse_down: Cell<i32>,
}

impl ClickLinkGesture {
    pub fn new(editor: &Rc<CodeEditorWidget>, opts: IClickLinkGestureOptions) -> Rc<Self> {
        let base = Disposable::new();
        let on_mouse_move_or_relevant_key_down = Emitter::new();
        base.register(on_mouse_move_or_relevant_key_down.clone());
        let on_execute = Emitter::new();
        base.register(on_execute.clone());
        let on_cancel = Emitter::new();
        base.register(on_cancel.clone());

        let extract_line_number_from_mouse_event = opts.extract_line_number_from_mouse_event.unwrap_or_else(|| {
            Rc::new(|e: &ClickLinkMouseEvent| e.target.position().map_or(0, |position| position.line_number))
        });
        let initial_opts = create_options(editor.get_option(MULTI_CURSOR_MODIFIER), editor.get_option(MOUSE_MIDDLE_CLICK_ACTION));

        Rc::new_cyclic(|me: &Weak<ClickLinkGesture>| {
            base.register(listen_weak(me, editor.on_did_change_configuration(), |this: &ClickLinkGesture, e| {
                this.on_configuration_changed(e);
            }));
            base.register(listen_weak(me, editor.on_mouse_move(), |this: &ClickLinkGesture, e| {
                let mouse_event = ClickLinkMouseEvent::new(e, &this.opts.borrow());
                this.on_editor_mouse_move(mouse_event);
            }));
            base.register(listen_weak(me, editor.on_mouse_down(), |this: &ClickLinkGesture, e| {
                let mouse_event = ClickLinkMouseEvent::new(e, &this.opts.borrow());
                this.on_editor_mouse_down(&mouse_event);
            }));
            base.register(listen_weak(me, editor.on_mouse_up(), |this: &ClickLinkGesture, e| {
                let mouse_event = ClickLinkMouseEvent::new(e, &this.opts.borrow());
                this.on_editor_mouse_up(mouse_event);
            }));
            base.register(listen_weak(me, editor.on_key_down(), |this: &ClickLinkGesture, e| {
                let keyboard_event = ClickLinkKeyboardEvent::new(e, &this.opts.borrow());
                this.on_editor_key_down(keyboard_event);
            }));
            base.register(listen_weak(me, editor.on_key_up(), |this: &ClickLinkGesture, e| {
                let keyboard_event = ClickLinkKeyboardEvent::new(e, &this.opts.borrow());
                this.on_editor_key_up(&keyboard_event);
            }));
            base.register(listen_weak(me, editor.on_mouse_drag(), |this: &ClickLinkGesture, _| this.reset_handler()));

            base.register(listen_weak(me, editor.on_did_change_cursor_selection(), |this: &ClickLinkGesture, e| {
                this.on_did_change_cursor_selection(e);
            }));
            base.register(listen_weak(me, editor.on_did_change_model(), |this: &ClickLinkGesture, _| this.reset_handler()));
            base.register(listen_weak(me, editor.on_did_change_model_content(), |this: &ClickLinkGesture, _| this.reset_handler()));
            base.register(listen_weak(me, editor.on_did_scroll_change(), |this: &ClickLinkGesture, e| {
                if e.scroll_top_changed || e.scroll_left_changed {
                    this.reset_handler();
                }
            }));

            ClickLinkGesture {
                base: base.clone(),
                on_mouse_move_or_relevant_key_down,
                on_execute,
                on_cancel,
                editor: Rc::downgrade(editor),
                extract_line_number_from_mouse_event,
                opts: RefCell::new(initial_opts),
                last_mouse_move_event: RefCell::new(None),
                has_trigger_key_on_mouse_down: Cell::new(false),
                line_number_on_mouse_down: Cell::new(0),
            }
        })
    }

    /// `onMouseMoveOrRelevantKeyDown`.
    pub fn on_mouse_move_or_relevant_key_down(&self) -> Event<(ClickLinkMouseEvent, Option<ClickLinkKeyboardEvent>)> {
        self.on_mouse_move_or_relevant_key_down.event()
    }

    /// `onExecute`.
    pub fn on_execute(&self) -> Event<ClickLinkMouseEvent> {
        self.on_execute.event()
    }

    /// `onCancel`.
    pub fn on_cancel(&self) -> Event<()> {
        self.on_cancel.event()
    }

    fn on_configuration_changed(&self, e: &ConfigurationChangedEvent) {
        if e.has_changed(EditorOption::MultiCursorModifier) || e.has_changed(EditorOption::MouseMiddleClickAction) {
            let Some(editor) = self.editor.upgrade() else {
                return;
            };
            let new_opts = create_options(editor.get_option(MULTI_CURSOR_MODIFIER), editor.get_option(MOUSE_MIDDLE_CLICK_ACTION));
            if *self.opts.borrow() == new_opts {
                return;
            }
            *self.opts.borrow_mut() = new_opts;
            *self.last_mouse_move_event.borrow_mut() = None;
            self.has_trigger_key_on_mouse_down.set(false);
            self.line_number_on_mouse_down.set(0);
            self.on_cancel.fire(());
        }
    }

    fn on_did_change_cursor_selection(&self, e: &ICursorSelectionChangedEvent) {
        if e.selection.start_column != e.selection.end_column {
            self.reset_handler(); // immediately stop this feature if the user starts to select (https://github.com/microsoft/vscode/issues/7827)
        }
    }

    fn on_editor_mouse_move(&self, mouse_event: ClickLinkMouseEvent) {
        *self.last_mouse_move_event.borrow_mut() = Some(mouse_event.clone());

        self.on_mouse_move_or_relevant_key_down.fire((mouse_event, None));
    }

    fn on_editor_mouse_down(&self, mouse_event: &ClickLinkMouseEvent) {
        // We need to record if we had the trigger key on mouse down because someone might select something in the editor
        // holding the mouse down and then while mouse is down start to press Ctrl/Cmd to start a copy operation and then
        // release the mouse button without wanting to do the navigation.
        // With this flag we prevent goto definition if the mouse was down before the trigger key was pressed.
        self.has_trigger_key_on_mouse_down.set(mouse_event.has_trigger_modifier);
        self.line_number_on_mouse_down.set((self.extract_line_number_from_mouse_event)(mouse_event));
    }

    fn on_editor_mouse_up(&self, mouse_event: ClickLinkMouseEvent) {
        let current_line_number = (self.extract_line_number_from_mouse_event)(&mouse_event);
        let line_number_on_mouse_down = self.line_number_on_mouse_down.get();
        let line_numbers_correct = line_number_on_mouse_down != 0 && line_number_on_mouse_down == current_line_number;
        if line_numbers_correct
            && (self.has_trigger_key_on_mouse_down.get() || (mouse_event.is_middle_click && mouse_event.mouse_middle_click_action == "openLink"))
        {
            self.on_execute.fire(mouse_event);
        }
    }

    fn on_editor_key_down(&self, e: ClickLinkKeyboardEvent) {
        let last_mouse_move_event = self.last_mouse_move_event.borrow().clone();
        match last_mouse_move_event {
            Some(last_mouse_move_event)
                if e.key_code_is_trigger_key // User just pressed Ctrl/Cmd (normal goto definition)
                    || (e.key_code_is_side_by_side_key && e.has_trigger_modifier) => // User pressed Ctrl/Cmd+Alt (goto definition to the side)
            {
                self.on_mouse_move_or_relevant_key_down.fire((last_mouse_move_event, Some(e)));
            }
            _ if e.has_trigger_modifier => {
                self.on_cancel.fire(()); // remove decorations if user holds another key with ctrl/cmd to prevent accident goto declaration
            }
            _ => {}
        }
    }

    fn on_editor_key_up(&self, e: &ClickLinkKeyboardEvent) {
        if e.key_code_is_trigger_key {
            self.on_cancel.fire(());
        }
    }

    fn reset_handler(&self) {
        *self.last_mouse_move_event.borrow_mut() = None;
        self.has_trigger_key_on_mouse_down.set(false);
        self.on_cancel.fire(());
    }

}

impl IDisposable for ClickLinkGesture {
    fn dispose(&self) {
        self.base.dispose();
    }
}
