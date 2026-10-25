//! Porte do `TypeNavigationController`, do `TypeNavigationMode` e do `DefaultKeyboardNavigationDelegate` de
//! `src/vs/base/browser/ui/list/listWidget.ts`: digitar para achar. A lista (e a árvore, que tem uma lista por baixo)
//! com um [`IKeyboardNavigationLabelProvider`] move o foco para o próximo elemento cujo rótulo casa com o que foi digitado.
//!
//! Do TypeScript para o Rust:
//! - O `Event.chain(keydown, ...)` do original é o ouvinte de `keydown` do nó da lista com os mesmos filtros, na mesma
//!   ordem: o alvo não é um campo de texto (`isEditableElement` é o [`Role::TextInput`]), o modo é automático ou a
//!   navegação foi disparada, `typing || keyboardNavigationEventFilter`, e o `mightProducePrintableCharacter` do delegate.
//!   O `Event.debounce(onChar, () => null, 800)` e o `Event.reduce` que acumula a palavra são um timer de 800 ms
//!   reiniciado a cada caractere e a palavra guardada no controlador.
//! - O `event.browserEvent.key` (o caractere digitado) não existe no [`KeyData`]: o caractere vem do código da tecla, como no
//!   layout dos EUA (letras e dígitos, o teclado numérico, e os sinais de `;` a `'`), em minúsculas porque o casamento
//!   ignora a caixa.
//! - Fica de fora o re-anúncio do rótulo do elemento ao fim da digitação (`alert(ariaLabel)` do `onClear`): o `je-ui` não
//!   tem região viva de anúncio.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::filters::{matches_fuzzy2, matches_prefix};
use je_base::key_codes::{KeyCode, KeyCodeUtils};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::runtime::{TimerId, clear_timeout, set_timeout};
use je_ui::{Event, EventKind, KeyData, NodeId, Role, Ui};

use crate::list::list_types::{IKeyboardNavigationDelegate, IKeyboardNavigationEventFilter, IKeyboardNavigationLabelProvider};
use crate::list::list_widget::List;

/// O tempo sem digitar que encerra a palavra (o `800` do `Event.debounce`).
const TYPING_TIMEOUT_MS: u64 = 800;

/// `TypeNavigationMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TypeNavigationMode {
    /// Digitar começa a navegação.
    #[default]
    Automatic,
    /// A navegação só começa depois de [`List::trigger_type_navigation`].
    Trigger,
}

/// `DefaultKeyboardNavigationDelegate`.
pub struct DefaultKeyboardNavigationDelegate;

impl IKeyboardNavigationDelegate for DefaultKeyboardNavigationDelegate {
    fn might_produce_printable_character(&self, event: &KeyData) -> bool {
        if event.modifiers.ctrl || event.modifiers.meta || event.modifiers.alt {
            return false;
        }

        let code = event.key_code;
        (code >= KeyCode::KeyA && code <= KeyCode::KeyZ)
            || (code >= KeyCode::Digit0 && code <= KeyCode::Digit9)
            || (code >= KeyCode::Numpad0 && code <= KeyCode::Numpad9)
            || (code >= KeyCode::Semicolon && code <= KeyCode::Quote)
    }
}

/// O `event.key` da tecla: o caractere que ela digita no layout dos EUA, em minúsculas sem o Shift.
fn typed_character(event: &KeyData) -> Option<String> {
    let code = event.key_code;
    if code >= KeyCode::Numpad0 && code <= KeyCode::Numpad9 {
        let digit = u32::try_from(code.0 - KeyCode::Numpad0.0).ok()?;
        return char::from_digit(digit, 10).map(String::from);
    }

    KeyCodeUtils::to_string(code).map(|label| if event.modifiers.shift { label.to_owned() } else { label.to_lowercase() })
}

/// `label && label.toString()`: os rótulos de um elemento juntos por vírgula (o `toString` de um array); `None` é o
/// `undefined`, quando o elemento não tem rótulo.
fn joined_label(labels: &[Option<String>]) -> Option<String> {
    let parts: Option<Vec<&str>> = labels.iter().map(|label| label.as_deref()).collect();
    parts.map(|parts| parts.join(","))
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypeNavigationControllerState {
    Idle,
    Typing,
}

/// `TypeNavigationController<T>`.
pub struct TypeNavigationController<T: Clone + 'static> {
    weak_self: Weak<TypeNavigationController<T>>,
    list: Weak<List<T>>,
    ui: Ui,
    /// O nó da lista, onde o `keydown` é ouvido (`this.view.domNode`).
    dom_node: NodeId,
    keyboard_navigation_label_provider: Rc<dyn IKeyboardNavigationLabelProvider<T>>,
    keyboard_navigation_event_filter: IKeyboardNavigationEventFilter,
    delegate: Rc<dyn IKeyboardNavigationDelegate>,
    enabled: Cell<bool>,
    state: Cell<TypeNavigationControllerState>,
    /// `list.options.typeNavigationMode`.
    mode: Cell<Option<TypeNavigationMode>>,
    /// `list.options.typeNavigationEnabled`: ligado liga o casamento difuso; ausente o desliga, mas a navegação por prefixo
    /// continua (o `?? true` do `updateOptions` só decide se o ouvinte existe).
    type_navigation_enabled: Cell<Option<bool>>,
    triggered: Cell<bool>,
    typing: Cell<bool>,
    /// A palavra digitada até agora (o acumulador do `Event.reduce`).
    word: RefCell<Option<String>>,
    clear_timer: Cell<Option<TimerId>>,
    enabled_disposables: Rc<DisposableStore>,
}

impl<T: Clone + 'static> TypeNavigationController<T> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        list: Weak<List<T>>,
        ui: &Ui,
        dom_node: NodeId,
        keyboard_navigation_label_provider: Rc<dyn IKeyboardNavigationLabelProvider<T>>,
        keyboard_navigation_event_filter: IKeyboardNavigationEventFilter,
        delegate: Rc<dyn IKeyboardNavigationDelegate>,
        type_navigation_enabled: Option<bool>,
        mode: Option<TypeNavigationMode>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|weak| Self {
            weak_self: weak.clone(),
            list,
            ui: ui.clone(),
            dom_node,
            keyboard_navigation_label_provider,
            keyboard_navigation_event_filter,
            delegate,
            enabled: Cell::new(false),
            state: Cell::new(TypeNavigationControllerState::Idle),
            mode: Cell::new(None),
            type_navigation_enabled: Cell::new(None),
            triggered: Cell::new(false),
            typing: Cell::new(false),
            word: RefCell::new(None),
            clear_timer: Cell::new(None),
            enabled_disposables: DisposableStore::new(),
        });
        this.update_options(type_navigation_enabled, mode);
        this
    }

    /// `updateOptions(options)`: as opções ausentes ficam como estavam.
    pub fn update_options(&self, type_navigation_enabled: Option<bool>, mode: Option<TypeNavigationMode>) {
        if type_navigation_enabled.is_some() {
            self.type_navigation_enabled.set(type_navigation_enabled);
        }
        if mode.is_some() {
            self.mode.set(mode);
        }

        if self.type_navigation_enabled.get().unwrap_or(true) {
            self.enable();
        } else {
            self.disable();
        }
    }

    /// `trigger()`.
    pub fn trigger(&self) {
        self.triggered.set(!self.triggered.get());
    }

    fn enable(&self) {
        if self.enabled.get() {
            return;
        }

        let weak = self.weak_self.clone();
        self.enabled_disposables.add(self.ui.listen(self.dom_node, EventKind::KeyDown, false, move |event| {
            let (Some(this), Some(key)) = (weak.upgrade(), event.key) else { return };
            this.on_key_down(event, &key);
        }));

        self.enabled.set(true);
        self.triggered.set(false);
    }

    fn disable(&self) {
        if !self.enabled.get() {
            return;
        }

        self.enabled_disposables.clear();
        self.enabled.set(false);
        self.triggered.set(false);
    }

    /// O `onChar` do original: os filtros do `keydown` e, se passam, o caractere.
    fn on_key_down(&self, event: &mut Event, key: &KeyData) {
        // `.filter(e => !isEditableElement(e.target))`
        if self.ui.tree().role(event.target) == Role::TextInput {
            return;
        }

        // `.filter(() => this.mode === Automatic || this.triggered)`
        if self.mode.get().unwrap_or_default() != TypeNavigationMode::Automatic && !self.triggered.get() {
            return;
        }

        // `.filter(e => typing || this.keyboardNavigationEventFilter(e))`
        if !self.typing.get() && !(self.keyboard_navigation_event_filter)(key) {
            return;
        }

        // `.filter(e => this.delegate.mightProducePrintableCharacter(e))`
        if !self.delegate.might_produce_printable_character(key) {
            return;
        }

        // `.forEach(e => EventHelper.stop(e, true))`
        event.stop(true);

        let Some(character) = typed_character(key) else { return };
        self.on_char(&character);
    }

    fn on_char(&self, character: &str) {
        // `onClear = Event.debounce(onChar, () => null, 800)`
        if let Some(timer) = self.clear_timer.take() {
            clear_timeout(timer);
        }
        let weak = self.weak_self.clone();
        self.clear_timer.set(Some(set_timeout(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.clear_timer.set(None);
                    this.on_clear();
                }
            },
            TYPING_TIMEOUT_MS,
        )));

        // `onInput = Event.reduce(Event.any(onChar, onClear), (r, i) => i === null ? null : (r || '') + i)`
        let word = {
            let mut word = self.word.borrow_mut();
            let next = format!("{}{}", word.as_deref().unwrap_or_default(), character);
            *word = Some(next.clone());
            next
        };
        self.on_input(Some(&word));

        // `onChar(() => typing = true)`
        self.typing.set(true);
    }

    /// O `onClear` disparado pelo `debounce` (a palavra vira `null`) e o `onClear(() => typing = false)`.
    fn on_clear(&self) {
        *self.word.borrow_mut() = None;
        self.on_input(None);
        self.typing.set(false);
    }

    /// `onInput(word)`.
    fn on_input(&self, word: Option<&str>) {
        let Some(word) = word.filter(|word| !word.is_empty()) else {
            self.state.set(TypeNavigationControllerState::Idle);
            self.triggered.set(false);
            return;
        };

        let Some(list) = self.list.upgrade() else { return };
        let focus = list.get_focus();
        let start = focus.first().copied().unwrap_or(0);
        let delta = usize::from(self.state.get() == TypeNavigationControllerState::Idle);
        self.state.set(TypeNavigationControllerState::Typing);

        let length = list.length();
        let word = utf16(word);
        let fuzzy_enabled = self.type_navigation_enabled.get() == Some(true);

        for i in 0..length {
            let index = (start + i + delta) % length;
            let label = joined_label(&self.keyboard_navigation_label_provider.get_keyboard_navigation_label(&list.element(index)));

            let found = if fuzzy_enabled {
                label.as_deref().is_some_and(|label| {
                    let label = utf16(label);

                    // If prefix is found, focus and return early
                    if matches_prefix(&word, &label).is_some() {
                        return true;
                    }

                    // ensures that when fuzzy matching, doesn't clash with prefix matching (1 input vs 1+ should be prefix and
                    // fuzzy respectively). Also makes sure that exact matches are prioritized.
                    matches_fuzzy2(&word, &label).is_some_and(|fuzzy| fuzzy.len() == 1 && fuzzy[0].end - fuzzy[0].start > 1)
                })
            } else {
                label.as_deref().is_none_or(|label| matches_prefix(&word, &utf16(label)).is_some())
            };

            if found {
                list.set_focus(vec![index], None);
                list.reveal(index, None, 0.0);
                return;
            }
        }
    }
}

impl<T: Clone + 'static> IDisposable for TypeNavigationController<T> {
    fn dispose(&self) {
        self.disable();
        self.enabled_disposables.dispose();
        if let Some(timer) = self.clear_timer.take() {
            clear_timeout(timer);
        }
    }
}
