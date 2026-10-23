//! Porte de `src/vs/base/browser/ui/findinput/replaceInput.ts`: o [`ReplaceInput`], o campo de substituição
//! do widget de busca (um [`HistoryInputBox`](crate::input_box::HistoryInputBox) com o botão de preservar
//! caixa à direita), com [`ReplaceInputOptions`].
//!
//! O DOM, o CSS e o que ficou de fora estão em [`crate::find_input_base`]; o que o `ReplaceInput` divide com
//! o `FindInput` (`input_box`, `enable`, `disable`, `set_enabled`, `clear`, `get_value`, `set_value`,
//! `select`, `focus`, `validate`, `show_message`, `clear_message`, `highlight_find_options`,
//! `on_key_down`, `on_key_up`, `on_mouse_down`, `on_did_option_change`) chega por `Deref`. O botão é o
//! `PreserveCaseToggle` ([`crate::find_input_toggles::preserve_case_toggle`]); `focusOnPreserve`,
//! `getPreserveCase`/`setPreserveCase`, `onInput` (sem valor) e `onPreserveCaseKeyDown` são daqui.
//!
//! `width` (o setter) reserva a largura do botão à direita do texto (zero se `show_option_buttons` é
//! falso) e fixa a largura do `domNode`. `applyStyles`, que no original é vazio, não existe.

use std::ops::Deref;
use std::rc::Rc;

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::history::{HistorySet, IHistory};
use je_base::lifecycle::{Disposable, IDisposable};
use je_ui::{NodeId, Ui};

use crate::common::registered_emitter;
use crate::context_view_api::IContextViewProvider;
use crate::dom::StandardKeyboardEvent;
use crate::find_input_base::{FindInputBase, FindInputStyles, InputParts};
use crate::find_input_toggles::{FindInputToggleOpts, preserve_case_toggle};
use crate::hover_api::IHoverLifecycleOptions;
use crate::input_box::InputValidator;
use crate::toggle::Toggle;

/// `IReplaceInputOptions`.
#[derive(Clone)]
pub struct ReplaceInputOptions {
    pub placeholder: Option<String>,
    pub validation: Option<InputValidator>,
    pub label: String,
    pub hover_lifecycle_options: Option<IHoverLifecycleOptions>,
    pub append_preserve_case_label: String,
    pub history: Option<Rc<dyn IHistory<String>>>,
    pub show_history_hint: Option<Rc<dyn Fn() -> bool>>,
    /// `inputBoxStyles`, `toggleStyles` e o fundo desabilitado do CSS.
    pub styles: FindInputStyles,
    /// Pisca o cursor do campo. Desligue em testes e capturas.
    pub caret_blink: bool,
}

impl ReplaceInputOptions {
    pub fn new(label: &str, styles: FindInputStyles) -> Self {
        Self {
            placeholder: None,
            validation: None,
            label: label.to_owned(),
            hover_lifecycle_options: None,
            append_preserve_case_label: String::new(),
            history: None,
            show_history_hint: None,
            styles,
            caret_blink: true,
        }
    }
}

/// O `ReplaceInput`. Ver a documentação do módulo.
pub struct ReplaceInput {
    base: Rc<Disposable>,
    shell: Rc<FindInputBase>,
    preserve_case: Rc<Toggle>,
    cached_options_width: f64,
    on_preserve_case_key_down: Rc<Emitter<StandardKeyboardEvent>>,
}

impl Deref for ReplaceInput {
    type Target = FindInputBase;

    fn deref(&self) -> &FindInputBase {
        &self.shell
    }
}

impl ReplaceInput {
    /// `new ReplaceInput(parent, contextViewProvider, showOptionButtons, options)`.
    pub fn new(ui: &Ui, parent: Option<NodeId>, context_view_provider: Option<Rc<dyn IContextViewProvider>>, show_option_buttons: bool, options: ReplaceInputOptions) -> Rc<ReplaceInput> {
        let base = Disposable::new();
        let preserve_case = preserve_case_toggle(
            ui,
            FindInputToggleOpts {
                append_title: options.append_preserve_case_label.clone(),
                is_checked: false,
                styles: options.styles.toggle_styles,
                hover_lifecycle_options: options.hover_lifecycle_options.clone(),
            },
        );
        let input = InputParts {
            placeholder: options.placeholder.clone().unwrap_or_default(),
            label: if options.label.is_empty() { "input".to_owned() } else { options.label.clone() },
            validation: options.validation.clone(),
            history: Some(options.history.clone().unwrap_or_else(|| Rc::new(HistorySet::new([])))),
            show_history_hint: options.show_history_hint.clone(),
            input_box_styles: options.styles.input_box_styles.clone(),
            hide_hover_on_value_change: false,
            caret_blink: options.caret_blink,
        };
        let shell = FindInputBase::new(ui, parent, context_view_provider, input, vec![preserve_case.clone()], show_option_buttons, options.styles.disabled_background);
        let cached_options_width = if show_option_buttons { preserve_case.width() } else { 0.0 };
        let this = Rc::new(ReplaceInput { shell, preserve_case, cached_options_width, on_preserve_case_key_down: registered_emitter(&base), base });
        {
            let emitter = this.on_preserve_case_key_down.clone();
            this.base.register(this.preserve_case.on_key_down().listen(move |e| emitter.fire(*e)));
        }
        this
    }

    pub fn on_input(&self) -> BaseEvent<()> {
        self.shell.input_box.on_input()
    }

    pub fn on_preserve_case_key_down(&self) -> BaseEvent<StandardKeyboardEvent> {
        self.on_preserve_case_key_down.event()
    }

    pub fn get_preserve_case(&self) -> bool {
        self.preserve_case.checked()
    }

    pub fn set_preserve_case(&self, value: bool) {
        self.preserve_case.set_checked(value);
    }

    pub fn focus_on_preserve(&self) {
        self.preserve_case.focus();
    }

    pub fn preserve_case(&self) -> &Rc<Toggle> {
        &self.preserve_case
    }

    /// `set width`: reserva a largura do botão à direita do texto e fixa a largura do `domNode`.
    pub fn set_width(&self, new_width: f64) {
        self.shell.set_fixed_width(new_width);
        self.shell.relayout(self.cached_options_width);
    }
}

impl IDisposable for ReplaceInput {
    fn dispose(&self) {
        self.base.dispose();
        self.shell.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ReplaceInput"
    }
}
