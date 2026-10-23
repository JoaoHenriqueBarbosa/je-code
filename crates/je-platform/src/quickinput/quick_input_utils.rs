//! Porte de `src/vs/platform/quickinput/browser/quickInputUtils.ts`: a conversão dos botões do quick input
//! em ações ([`quick_input_button_to_action`], [`quick_input_buttons_to_action_arrays`]) e a classe de ícone
//! por caminho ([`registered_icon_classes`]).
//!
//! Fora do porte, por tocar o DOM (especificação, não código executado):
//! - `renderQuickInputDescription`: monta `<a>` com `dom.$`, `DomEmitter`, `StandardKeyboardEvent`, `Gesture`
//!   e `container.append`. É um widget de pintura e entra com o widget do quick input, não aqui. A lógica
//!   pura que ele usa (`parseLinkedText`) já está em `je_base::linked_text`.
//! - `import './media/quickInput.css'`: efeito colateral de CSS, especificação para o widget.
//!
//! Divergências:
//! - `getIconClass` chama `createCSSRule` duas vezes (`domStylesheets`), que escreve numa folha de estilo do
//!   documento. Aqui a parte pura fica (o mapa caminho para classe, o gerador `quick-input-button-icon-N` e a
//!   escolha de `light || dark`), e as duas regras viram dados em [`IconClassRegistration`], lidos por
//!   [`registered_icon_classes`]: `.{classe}, .hc-light .{classe}` usa `light || dark`, e
//!   `.vs-dark .{classe}, .hc-black .{classe}` usa `dark`, ambas com `background-image`. O `asCSSUrl`
//!   (`CSS.escape` mais `FileAccess.uriToBrowserUri`) é do pintor que consumir o registro. O estado
//!   global do módulo (o mapa e o gerador) é um por thread.
//! - `IAction` é o trait de `je_base::actions`, e uma ação é `ActionRef` (`Rc<dyn IAction>`). O objeto literal
//!   do original (`{ id, label, tooltip, class, enabled, run }`) é um `to_action`; o `run` sem argumentos
//!   `() => unknown` é [`RunFn`], que devolve [`MaybePromise`] (o `unknown` que pode ser promessa).
//!   `action.run()` roda o corpo já, de forma síncrona (como a função `async` do original), e devolve a
//!   promessa que o `IAction.run` do porte devolve.
//! - `QuickInputToggleButtonAction` não tem `onDidChange` (o original também não): `label`, `tooltip`,
//!   `class` e `enabled` são campos públicos que o original escreve direto, aqui os setters do trait.
//! - Valores falsos do JavaScript: `button.iconClass || ...`, `if (button.label)` e `if (button.alwaysVisible)`
//!   tratam texto vazio e `false` como ausentes, e o porte também.
//! - `onTrigger: (button) => unknown` recebe o botão por identidade ([`QuickInputButtonRef`]).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::any::Any;
use std::rc::Rc;

use je_base::JsString;
use je_base::actions::{ActionRef, IAction, ToActionProps, to_action};
use je_base::async_::async_fn_maybe;
use je_base::dynamic::Dynamic;
use je_base::id_generator::IdGenerator;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;

use crate::quickinput::quick_input::{IQuickInputButton, QuickInputButtonRef, QuickInputIconPath};

/// O `() => unknown` que roda a ação.
pub type RunFn = Rc<dyn Fn() -> MaybePromise<Dynamic>>;

/// O `onTrigger: (button: IQuickInputButton) => unknown` de [`quick_input_buttons_to_action_arrays`].
pub type OnTrigger = Rc<dyn Fn(&QuickInputButtonRef) -> MaybePromise<Dynamic>>;

/// As duas regras de CSS que `getIconClass` cria para uma classe de ícone.
#[derive(Clone, Debug)]
pub struct IconClassRegistration {
    pub class_name: String,
    /// O ícone do tema escuro (e o do claro, quando não há `light`).
    pub dark: URI,
    /// O ícone do tema claro (`iconPath.light`), se houver.
    pub light: Option<URI>,
}

thread_local! {
    static ICON_PATH_TO_CLASS: RefCell<HashMap<JsString, String>> = RefCell::new(HashMap::new());
    static ICON_CLASS_GENERATOR: IdGenerator = IdGenerator::new("quick-input-button-icon-");
    static ICON_CLASS_REGISTRATIONS: RefCell<Vec<IconClassRegistration>> = const { RefCell::new(Vec::new()) };
}

/// As classes de ícone criadas até agora, na ordem, para o pintor gerar as regras de `background-image`.
pub fn registered_icon_classes() -> Vec<IconClassRegistration> {
    ICON_CLASS_REGISTRATIONS.with(|registrations| registrations.borrow().clone())
}

fn get_icon_class(icon_path: Option<&QuickInputIconPath>) -> Option<String> {
    let icon_path = icon_path?;

    let key = icon_path.dark.to_string(false);
    if let Some(icon_class) = ICON_PATH_TO_CLASS.with(|map| map.borrow().get(&key).cloned()) {
        return Some(icon_class);
    }

    let icon_class = ICON_CLASS_GENERATOR.with(IdGenerator::next_id);
    ICON_CLASS_REGISTRATIONS.with(|registrations| {
        registrations.borrow_mut().push(IconClassRegistration {
            class_name: icon_class.clone(),
            dark: icon_path.dark.clone(),
            light: icon_path.light.clone(),
        });
    });
    ICON_PATH_TO_CLASS.with(|map| map.borrow_mut().insert(key, icon_class.clone()));

    Some(icon_class)
}

struct QuickInputToggleButtonAction {
    id: String,
    label: RefCell<String>,
    tooltip: RefCell<String>,
    class: RefCell<Option<String>>,
    enabled: Cell<bool>,
    checked: Cell<Option<bool>>,
    run_handler: RunFn,
}

impl QuickInputToggleButtonAction {
    fn new(id: &str, label: &str, tooltip: &str, class_name: Option<String>, enabled: bool, checked: bool, run: RunFn) -> Self {
        Self {
            id: id.to_owned(),
            label: RefCell::new(label.to_owned()),
            tooltip: RefCell::new(tooltip.to_owned()),
            class: RefCell::new(class_name),
            enabled: Cell::new(enabled),
            checked: Cell::new(Some(checked)),
            run_handler: run,
        }
    }
}

impl IAction for QuickInputToggleButtonAction {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn label(&self) -> String {
        self.label.borrow().clone()
    }

    fn set_label(&self, value: String) {
        *self.label.borrow_mut() = value;
    }

    fn tooltip(&self) -> String {
        self.tooltip.borrow().clone()
    }

    fn set_tooltip(&self, value: String) {
        *self.tooltip.borrow_mut() = value;
    }

    fn class(&self) -> Option<String> {
        self.class.borrow().clone()
    }

    fn set_class(&self, value: Option<String>) {
        *self.class.borrow_mut() = value;
    }

    fn enabled(&self) -> bool {
        self.enabled.get()
    }

    fn set_enabled(&self, value: bool) {
        self.enabled.set(value);
    }

    fn checked(&self) -> Option<bool> {
        self.checked.get()
    }

    fn set_checked(&self, value: Option<bool>) {
        self.checked.set(value);
        // Toggles behave like buttons. When clicked, they run... the only difference is that their checked state also changes.
        let _ = (self.run_handler)();
    }

    fn run(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        self.checked.set(Some(!self.checked.get().unwrap_or(false)));
        let run = self.run_handler.clone();
        async_fn_maybe(async move { Ok(run()) })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn quick_input_button_to_action(button: &IQuickInputButton, id: &str, run: RunFn) -> ActionRef {
    let mut css_classes = button.icon_class.clone().filter(|class_name| !class_name.is_empty()).or_else(|| get_icon_class(button.icon_path.as_ref()));
    if button.always_visible == Some(true) {
        css_classes = Some(match css_classes {
            Some(class_name) => format!("{class_name} always-visible"),
            None => "always-visible".to_owned(),
        });
    }

    let toggle = button.toggle.clone();
    let handler: RunFn = Rc::new({
        let toggle = toggle.clone();
        move || {
            if let Some(toggle) = &toggle {
                toggle.checked.set(!toggle.checked.get());
            }
            run()
        }
    });

    let tooltip = button.tooltip.clone().unwrap_or_default();
    match toggle {
        Some(toggle) => Rc::new(QuickInputToggleButtonAction::new(id, &tooltip, "", css_classes, true, toggle.checked.get(), handler)),
        None => to_action(ToActionProps {
            id: id.to_owned(),
            label: String::new(),
            tooltip: Some(tooltip),
            enabled: Some(true),
            checked: None,
            class: css_classes,
            alt: None,
            run: Rc::new(move |_: &[Dynamic]| handler()),
        }),
    }
}

/// O `{ primary: IAction[]; secondary: IAction[] }` de [`quick_input_buttons_to_action_arrays`].
pub struct QuickInputActionArrays {
    pub primary: Vec<ActionRef>,
    pub secondary: Vec<ActionRef>,
}

pub fn quick_input_buttons_to_action_arrays(buttons: &[QuickInputButtonRef], id_prefix: &str, on_trigger: OnTrigger) -> QuickInputActionArrays {
    let mut primary: Vec<ActionRef> = Vec::new();
    let mut secondary: Vec<ActionRef> = Vec::new();

    for (index, button) in buttons.iter().enumerate() {
        let triggered_button = button.clone();
        let on_trigger = on_trigger.clone();
        let action = quick_input_button_to_action(
            button,
            &format!("{id_prefix}-{index}"),
            Rc::new(move || {
                let (button, on_trigger) = (triggered_button.clone(), on_trigger.clone());
                MaybePromise::Promise(async_fn_maybe(async move { Ok(on_trigger(&button)) }))
            }),
        );

        if let Some(label) = button.label.as_ref().filter(|label| !label.is_empty()) {
            action.set_label(label.clone());
        }

        if button.secondary == Some(true) {
            secondary.push(action);
        } else {
            primary.push(action);
        }
    }

    QuickInputActionArrays { primary, secondary }
}
