//! Porte de `src/vs/base/browser/ui/findinput/findInput.ts`: o [`FindInput`], o campo de busca do
//! workbench (um [`HistoryInputBox`](crate::input_box::HistoryInputBox) com os botões de caixa, palavra
//! inteira e expressão regular à direita), com [`FindInputOptions`], [`FindInputEvent`] e os estilos
//! ([`FindInputStyles`], [`default_find_input_styles`]).
//!
//! O DOM, o CSS e o que ficou de fora estão em [`crate::find_input_base`], que guarda o que o
//! `FindInput` e o `ReplaceInput` fazem igual (e que o `FindInput` expõe por `Deref`: `input_box`,
//! `dom_node`, `enable`, `disable`, `set_enabled`, `clear`, `get_value`, `set_value`, `select`, `focus`,
//! `validate`, `show_message`, `clear_message`, `highlight_find_options`, `on_key_down`, `on_key_up`,
//! `on_mouse_down`, `on_did_option_change`, `set_focus_input_on_option_click`, `on_search_submit`).
//! Os botões estão em [`crate::find_input_toggles`].
//!
//! O que é só do `FindInput`: os três botões e os getters e setters deles, `setAdditionalToggles`, a
//! reserva de largura à direita do texto (`updateInputBoxPadding`, na [`FindInput::layout`]) e os eventos
//! `onInput`, `onCaseSensitiveKeyDown` e `onRegexKeyDown`. `onInput` dispara por digitação (o `input` do
//! DOM) com `from_composition_end = false`: o fim de composição de IME não existe no `je-ui`.

use std::ops::Deref;
use std::rc::Rc;

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::history::IHistory;
use je_base::lifecycle::{Disposable, IDisposable};
use je_ui::{NodeId, Ui};

use crate::common::registered_emitter;
use crate::context_view_api::IContextViewProvider;
use crate::dom::StandardKeyboardEvent;
pub use crate::find_input_base::{FindInputBase, FindInputStyles, default_find_input_styles};
use crate::find_input_base::InputParts;
use crate::find_input_toggles::{FindInputToggleOpts, case_sensitive_toggle, regex_toggle, whole_words_toggle};
use crate::hover_api::IHoverLifecycleOptions;
use crate::input_box::InputValidator;
use crate::toggle::Toggle;

/// `IFindInputOptions`.
#[derive(Clone)]
pub struct FindInputOptions {
    pub placeholder: Option<String>,
    pub validation: Option<InputValidator>,
    pub label: String,
    pub show_common_find_toggles: bool,
    pub append_case_sensitive_label: String,
    pub append_whole_words_label: String,
    pub append_regex_label: String,
    pub additional_toggles: Vec<Rc<Toggle>>,
    pub show_history_hint: Option<Rc<dyn Fn() -> bool>>,
    /// `inputBoxStyles`, `toggleStyles` e o fundo desabilitado do CSS.
    pub styles: FindInputStyles,
    pub history: Option<Rc<dyn IHistory<String>>>,
    pub hover_lifecycle_options: Option<IHoverLifecycleOptions>,
    pub hide_hover_on_value_change: bool,
    /// Pisca o cursor do campo. Desligue em testes e capturas.
    pub caret_blink: bool,
}

impl FindInputOptions {
    pub fn new(label: &str, styles: FindInputStyles) -> Self {
        Self {
            placeholder: None,
            validation: None,
            label: label.to_owned(),
            show_common_find_toggles: false,
            append_case_sensitive_label: String::new(),
            append_whole_words_label: String::new(),
            append_regex_label: String::new(),
            additional_toggles: Vec::new(),
            show_history_hint: None,
            styles,
            history: None,
            hover_lifecycle_options: None,
            hide_hover_on_value_change: false,
            caret_blink: true,
        }
    }
}

/// `IFindInputEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindInputEvent {
    pub from_composition_end: bool,
}

/// `layout(style)`: o estado do widget de busca que decide se os botões reservam largura.
#[derive(Clone, Copy, Debug, Default)]
pub struct FindInputLayoutStyle {
    pub collapsed_find_widget: bool,
    pub narrow_find_widget: bool,
    pub reduced_find_widget: bool,
}

/// O `FindInput`. Ver a documentação do módulo.
pub struct FindInput {
    base: Rc<Disposable>,
    shell: Rc<FindInputBase>,
    regex: Option<Rc<Toggle>>,
    whole_words: Option<Rc<Toggle>>,
    case_sensitive: Option<Rc<Toggle>>,
    on_input: Rc<Emitter<FindInputEvent>>,
    on_case_sensitive_key_down: Rc<Emitter<StandardKeyboardEvent>>,
    on_regex_key_down: Rc<Emitter<StandardKeyboardEvent>>,
}

impl Deref for FindInput {
    type Target = FindInputBase;

    fn deref(&self) -> &FindInputBase {
        &self.shell
    }
}

impl FindInput {
    /// `new FindInput(parent, contextViewProvider, options)`: o `domNode` entra como último filho de
    /// `parent`, se houver.
    pub fn new(ui: &Ui, parent: Option<NodeId>, context_view_provider: Option<Rc<dyn IContextViewProvider>>, options: FindInputOptions) -> Rc<FindInput> {
        let base = Disposable::new();
        let hover_lifecycle_options = options.hover_lifecycle_options.clone().unwrap_or_else(|| IHoverLifecycleOptions { group_id: Some("find-input".to_owned()), ..Default::default() });
        let toggle_opts = |append_title: &str| FindInputToggleOpts {
            append_title: append_title.to_owned(),
            is_checked: false,
            styles: options.styles.toggle_styles,
            hover_lifecycle_options: Some(hover_lifecycle_options.clone()),
        };
        let (mut regex, mut whole_words, mut case_sensitive) = (None, None, None);
        if options.show_common_find_toggles {
            regex = Some(regex_toggle(ui, toggle_opts(&options.append_regex_label)));
            whole_words = Some(whole_words_toggle(ui, toggle_opts(&options.append_whole_words_label)));
            case_sensitive = Some(case_sensitive_toggle(ui, toggle_opts(&options.append_case_sensitive_label)));
        }
        let common_toggles = [&case_sensitive, &whole_words, &regex].into_iter().flatten().cloned().collect();
        let input = InputParts {
            placeholder: options.placeholder.clone().unwrap_or_default(),
            label: if options.label.is_empty() { "input".to_owned() } else { options.label.clone() },
            validation: options.validation.clone(),
            history: options.history.clone(),
            show_history_hint: options.show_history_hint.clone(),
            input_box_styles: options.styles.input_box_styles.clone(),
            hide_hover_on_value_change: options.hide_hover_on_value_change,
            caret_blink: options.caret_blink,
        };
        let shell = FindInputBase::new(ui, parent, context_view_provider, input, common_toggles, options.show_common_find_toggles, options.styles.disabled_background);
        let this = Rc::new(FindInput {
            shell,
            regex,
            whole_words,
            case_sensitive,
            on_input: registered_emitter(&base),
            on_case_sensitive_key_down: registered_emitter(&base),
            on_regex_key_down: registered_emitter(&base),
            base,
        });
        for (toggle, emitter) in [(&this.case_sensitive, &this.on_case_sensitive_key_down), (&this.regex, &this.on_regex_key_down)] {
            if let Some(toggle) = toggle {
                let emitter = emitter.clone();
                this.base.register(toggle.on_key_down().listen(move |e| emitter.fire(*e)));
            }
        }
        {
            let emitter = this.on_input.clone();
            this.base.register(this.shell.input_box.on_input().listen(move |()| emitter.fire(FindInputEvent { from_composition_end: false })));
        }
        this.set_additional_toggles(Some(options.additional_toggles));
        this
    }

    pub fn on_input(&self) -> BaseEvent<FindInputEvent> {
        self.on_input.event()
    }

    pub fn on_case_sensitive_key_down(&self) -> BaseEvent<StandardKeyboardEvent> {
        self.on_case_sensitive_key_down.event()
    }

    pub fn on_regex_key_down(&self) -> BaseEvent<StandardKeyboardEvent> {
        self.on_regex_key_down.event()
    }

    /// `onDidChange`: o valor mudou.
    pub fn on_did_change(&self) -> BaseEvent<String> {
        self.shell.input_box.on_did_change()
    }

    /// `layout(style)`: refaz o campo e a largura que os botões reservam.
    pub fn layout(&self, style: FindInputLayoutStyle) {
        self.update_input_box_padding(style.collapsed_find_widget);
    }

    /// `setAdditionalToggles`.
    pub fn set_additional_toggles(&self, toggles: Option<Vec<Rc<Toggle>>>) {
        self.shell.set_additional_toggles(toggles.unwrap_or_default());
        self.update_input_box_padding(false);
    }

    /// `updateInputBoxPadding`: `inputBox.paddingRight = togglesWidth + actionsWidth` (sem `actions`).
    fn update_input_box_padding(&self, controls_hidden: bool) {
        self.shell.relayout(if controls_hidden { 0.0 } else { self.shell.toggles_width(true) });
    }

    pub fn get_case_sensitive(&self) -> bool {
        self.case_sensitive.as_ref().is_some_and(|toggle| toggle.checked())
    }

    pub fn set_case_sensitive(&self, value: bool) {
        if let Some(toggle) = &self.case_sensitive {
            toggle.set_checked(value);
        }
    }

    pub fn get_whole_words(&self) -> bool {
        self.whole_words.as_ref().is_some_and(|toggle| toggle.checked())
    }

    pub fn set_whole_words(&self, value: bool) {
        if let Some(toggle) = &self.whole_words {
            toggle.set_checked(value);
        }
    }

    pub fn get_regex(&self) -> bool {
        self.regex.as_ref().is_some_and(|toggle| toggle.checked())
    }

    pub fn set_regex(&self, value: bool) {
        if let Some(toggle) = &self.regex {
            toggle.set_checked(value);
            self.validate();
        }
    }

    pub fn focus_on_case_sensitive(&self) {
        if let Some(toggle) = &self.case_sensitive {
            toggle.focus();
        }
    }

    pub fn focus_on_regex(&self) {
        if let Some(toggle) = &self.regex {
            toggle.focus();
        }
    }

    pub fn case_sensitive(&self) -> Option<&Rc<Toggle>> {
        self.case_sensitive.as_ref()
    }

    pub fn whole_words(&self) -> Option<&Rc<Toggle>> {
        self.whole_words.as_ref()
    }

    pub fn regex(&self) -> Option<&Rc<Toggle>> {
        self.regex.as_ref()
    }
}

impl IDisposable for FindInput {
    fn dispose(&self) {
        self.base.dispose();
        self.shell.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "FindInput"
    }
}
