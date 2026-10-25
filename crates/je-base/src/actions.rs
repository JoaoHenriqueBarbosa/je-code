//! Porte de `src/vs/base/common/actions.ts`: `IAction`, `Action`, `ActionRunner`, `Separator`,
//! `SubmenuAction`, `EmptySubmenuAction` e `toAction`.
//!
//! Decisões de tradução:
//! - **`IAction`** é o trait [`IAction`], com os getters e os setters das propriedades mutáveis do
//!   original (`label`, `tooltip`, `class`, `enabled`, `checked`) e `as_any` para o `instanceof`
//!   (`action instanceof Separator`). Uma ação é compartilhada, então vive em `Rc<dyn IAction>`
//!   ([`ActionRef`]) e a mutação passa por mutabilidade interior. As propriedades opcionais
//!   (`class`, `checked`, `alt`) são `Option`.
//! - **`run(...args: unknown[]): unknown`** é `run(&self, args: &[Dynamic]) -> Promise<Dynamic>`: toda
//!   implementação do original (`Action`, `Separator`, `SubmenuAction`, `toAction`) é `async`, então devolve
//!   sempre uma promessa. O valor `unknown` do argumento e do resultado é [`Dynamic`], e `void` é
//!   `Dynamic::Undefined`.
//! - **Herança**: `Action extends Disposable` é composição (um [`Disposable`] por dentro, com o emitter
//!   registrado). `EmptySubmenuAction extends Action` é [`EmptySubmenuAction::create`], que devolve uma [`Action`]
//!   com o id e o rótulo dele. Os métodos protegidos `_setLabel`, `_setTooltip`, `_setClass`, `_setEnabled` e
//!   `_setChecked`, que uma subclasse sobrescreve, não são pontos de extensão aqui, mas os setters públicos
//!   têm o mesmo efeito (comparam, trocam e disparam `onDidChange`). O `protected run` de `ActionRunner`
//!   (`runAction`) é um parâmetro: [`ActionRunner::with_run_action`].
//! - **`IActionRunner.run`** devolve `Promise<()>` (as duas implementações do original devolvem `void`).
//! - **`onDidChange`**: o evento `{ checked: value }` do original distingue `checked: undefined` de "sem
//!   mudança de `checked`" pela presença da chave; [`IActionChangeEvent`] guarda `Option<bool>`, que não
//!   distingue as duas coisas (o mesmo vale para `class`). Os campos mudados valem `Some`.
//! - **`getActionForAltMode`** recebe a plataforma ([`Platform`]) no lugar das constantes `isWindows` e
//!   `isLinux` do módulo.
//! - **`ITelemetryData`** tem os campos `from` e `target` e as demais chaves em `extra`. O tipo
//!   `WorkbenchActionExecutedClassification` é só a anotação de telemetria do compilador, sem forma em
//!   tempo de execução, e não é portado; [`WorkbenchActionExecutedEvent`] é.
//! - **`toAction({ run })`**: `run` devolve [`MaybePromise<Dynamic>`] e a promessa do `async` o adota.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::async_::{async_fn, async_fn_maybe, await_maybe};
use crate::dynamic::Dynamic;
use crate::errors::Error;
use crate::event::{Emitter, Event};
use crate::lifecycle::{Disposable, IDisposable};
use crate::nls;
use crate::platform::Platform;
use crate::promise::{MaybePromise, Promise};

#[derive(Clone, Debug, Default)]
pub struct ITelemetryData {
    pub from: Option<String>,
    pub target: Option<String>,
    /// `[key: string]: unknown`.
    pub extra: Vec<(String, Dynamic)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkbenchActionExecutedEvent {
    pub id: String,
    pub from: String,
    pub detail: Option<String>,
}

/// `IAction`.
pub trait IAction {
    fn id(&self) -> String;
    fn label(&self) -> String;
    fn set_label(&self, value: String);
    fn tooltip(&self) -> String;
    fn set_tooltip(&self, value: String);
    fn class(&self) -> Option<String>;
    fn set_class(&self, value: Option<String>);
    fn enabled(&self) -> bool;
    fn set_enabled(&self, value: bool);
    fn checked(&self) -> Option<bool>;
    fn set_checked(&self, value: Option<bool>);
    /// Alternative action for this item.
    fn alt(&self) -> Option<ActionRef> {
        None
    }
    fn run(&self, args: &[Dynamic]) -> Promise<Dynamic>;
    /// Para o `instanceof` do original.
    fn as_any(&self) -> &dyn Any;
    /// O `instanceof Action` do original: a [`Action`] que esta ação é ou estende (por composição), se houver.
    fn as_base_action(&self) -> Option<&Action> {
        None
    }
}

/// O handle compartilhado de uma ação.
pub type ActionRef = Rc<dyn IAction>;

/// As teclas modificadoras que `getActionForAltMode` consulta.
#[derive(Clone, Copy, Debug, Default)]
pub struct AltModeModifiers {
    pub alt_key: bool,
    pub shift_key: bool,
}

pub fn get_action_for_alt_mode(action: &ActionRef, modifiers: AltModeModifiers, host: &Platform) -> ActionRef {
    let use_alternative = modifiers.alt_key || ((host.is_windows || host.is_linux) && modifiers.shift_key);
    if use_alternative && action.enabled() && let Some(alt) = action.alt().filter(|alt| alt.enabled()) {
        return alt;
    }
    action.clone()
}

/// As propriedades mutáveis de uma ação, que `Action`, `Separator`, `SubmenuAction` e `toAction` repetem.
struct ActionProps {
    id: String,
    label: RefCell<String>,
    tooltip: RefCell<String>,
    class: RefCell<Option<String>>,
    enabled: Cell<bool>,
    checked: Cell<Option<bool>>,
}

impl ActionProps {
    fn new(id: &str, label: &str, tooltip: &str, class: Option<&str>, enabled: bool, checked: Option<bool>) -> Self {
        Self {
            id: id.to_owned(),
            label: RefCell::new(label.to_owned()),
            tooltip: RefCell::new(tooltip.to_owned()),
            class: RefCell::new(class.map(str::to_owned)),
            enabled: Cell::new(enabled),
            checked: Cell::new(checked),
        }
    }
}

/// Implementa os getters e os setters de [`IAction`] sobre um campo `props: ActionProps`. Os setters
/// guardam o valor e depois chamam `$changed` (`|self, event| ...`), onde o [`Action`] dispara
/// `onDidChange` e os demais não fazem nada.
macro_rules! action_props {
    ($changed:expr) => {
        fn id(&self) -> String {
            self.props.id.clone()
        }
        fn label(&self) -> String {
            self.props.label.borrow().clone()
        }
        fn set_label(&self, value: String) {
            if *self.props.label.borrow() != value {
                *self.props.label.borrow_mut() = value.clone();
                ($changed)(self, IActionChangeEvent { label: Some(value), ..Default::default() });
            }
        }
        fn tooltip(&self) -> String {
            self.props.tooltip.borrow().clone()
        }
        fn set_tooltip(&self, value: String) {
            if *self.props.tooltip.borrow() != value {
                *self.props.tooltip.borrow_mut() = value.clone();
                ($changed)(self, IActionChangeEvent { tooltip: Some(value), ..Default::default() });
            }
        }
        fn class(&self) -> Option<String> {
            self.props.class.borrow().clone()
        }
        fn set_class(&self, value: Option<String>) {
            if *self.props.class.borrow() != value {
                *self.props.class.borrow_mut() = value.clone();
                ($changed)(self, IActionChangeEvent { class: value, ..Default::default() });
            }
        }
        fn enabled(&self) -> bool {
            self.props.enabled.get()
        }
        fn set_enabled(&self, value: bool) {
            if self.props.enabled.get() != value {
                self.props.enabled.set(value);
                ($changed)(self, IActionChangeEvent { enabled: Some(value), ..Default::default() });
            }
        }
        fn checked(&self) -> Option<bool> {
            self.props.checked.get()
        }
        fn set_checked(&self, value: Option<bool>) {
            if self.props.checked.get() != value {
                self.props.checked.set(value);
                ($changed)(self, IActionChangeEvent { checked: value, ..Default::default() });
            }
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    };
}

/// `IActionChangeEvent`: o que mudou (os campos mudados valem `Some`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IActionChangeEvent {
    pub label: Option<String>,
    pub tooltip: Option<String>,
    pub class: Option<String>,
    pub enabled: Option<bool>,
    pub checked: Option<bool>,
}

/// Callback de uma [`Action`]: `(event?: unknown) => unknown`.
pub type ActionCallback = Rc<dyn Fn(Dynamic) -> MaybePromise<Dynamic>>;

/// A concrete implementation of [`IAction`].
///
/// Note that in most cases you should use the lighter-weight [`to_action`] function instead.
pub struct Action {
    base: Rc<Disposable>,
    on_did_change: Rc<Emitter<IActionChangeEvent>>,
    props: ActionProps,
    action_callback: Option<ActionCallback>,
}

impl Action {
    /// `new Action(id, label = '', cssClass = '', enabled = true, actionCallback?)`: `class` ausente é `''`,
    /// como o parâmetro padrão do original (que também vale para `undefined`).
    pub fn new(id: &str, label: &str, css_class: &str, enabled: bool, action_callback: Option<ActionCallback>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        Rc::new(Self { base, on_did_change, props: ActionProps::new(id, label, "", Some(css_class), enabled, None), action_callback })
    }

    pub fn on_did_change(&self) -> Event<IActionChangeEvent> {
        self.on_did_change.event()
    }

    fn fire_changed(&self, event: IActionChangeEvent) {
        self.on_did_change.fire(event);
    }
}

impl IAction for Action {
    action_props!(|this: &Action, event| this.fire_changed(event));

    fn as_base_action(&self) -> Option<&Action> {
        Some(self)
    }

    /// `run(event?, data?)`: chama o callback, se houver, e espera o resultado.
    fn run(&self, args: &[Dynamic]) -> Promise<Dynamic> {
        let event = args.first().cloned().unwrap_or(Dynamic::Undefined);
        let callback = self.action_callback.clone();
        async_fn(async move {
            if let Some(callback) = callback {
                await_maybe(callback(event)).await?;
            }
            Ok(Dynamic::Undefined)
        })
    }
}

impl IDisposable for Action {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "Action"
    }
}

#[derive(Clone)]
pub struct IRunEvent {
    pub action: ActionRef,
    pub error: Option<Error>,
}

pub trait IActionRunner: IDisposable {
    fn on_did_run(&self) -> Event<IRunEvent>;
    fn on_will_run(&self) -> Event<IRunEvent>;

    fn run(&self, action: &ActionRef, context: Option<Dynamic>) -> Promise<()>;
}

/// O `runAction` de [`ActionRunner`]: o que de fato roda a ação.
pub type RunAction = Rc<dyn Fn(&ActionRef, Option<Dynamic>) -> Promise<()>>;

pub struct ActionRunner {
    base: Rc<Disposable>,
    on_will_run: Rc<Emitter<IRunEvent>>,
    on_did_run: Rc<Emitter<IRunEvent>>,
    run_action: RunAction,
}

impl ActionRunner {
    pub fn new() -> Rc<Self> {
        Self::with_run_action(Self::default_run_action())
    }

    /// O `runAction` padrão: roda a ação com o contexto. Quem sobrescreve o método protegido no original e chama
    /// `super.runAction(action, context)` chama esta função.
    pub fn default_run_action() -> RunAction {
        Rc::new(|action, context| {
            let promise = action.run(&[context.unwrap_or(Dynamic::Undefined)]);
            async_fn(async move {
                promise.await?;
                Ok(())
            })
        })
    }

    /// Um runner com o `runAction` sobrescrito (o método protegido do original).
    pub fn with_run_action(run_action: RunAction) -> Rc<Self> {
        let base = Disposable::new();
        let on_will_run = Emitter::new();
        base.register(on_will_run.clone());
        let on_did_run = Emitter::new();
        base.register(on_did_run.clone());
        Rc::new(Self { base, on_will_run, on_did_run, run_action })
    }
}

impl IActionRunner for ActionRunner {
    fn on_did_run(&self) -> Event<IRunEvent> {
        self.on_did_run.event()
    }

    fn on_will_run(&self) -> Event<IRunEvent> {
        self.on_will_run.event()
    }

    fn run(&self, action: &ActionRef, context: Option<Dynamic>) -> Promise<()> {
        let (action, run_action) = (action.clone(), self.run_action.clone());
        let (on_will_run, on_did_run) = (self.on_will_run.clone(), self.on_did_run.clone());
        async_fn(async move {
            if !action.enabled() {
                return Ok(());
            }

            on_will_run.fire(IRunEvent { action: action.clone(), error: None });

            let error = match crate::errors::catch(|| run_action(&action, context)) {
                Ok(promise) => promise.await.err(),
                Err(error) => Some(error),
            };

            on_did_run.fire(IRunEvent { action, error });
            Ok(())
        })
    }
}

impl IDisposable for ActionRunner {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ActionRunner"
    }
}

/// Resolve com `Dynamic::Undefined`, como a função `async` vazia (`async run() { }`).
fn resolved_undefined() -> Promise<Dynamic> {
    async_fn(async { Ok(Dynamic::Undefined) })
}

pub struct Separator {
    props: ActionProps,
}

impl Separator {
    pub const ID: &'static str = "vs.actions.separator";

    pub fn new() -> Rc<Self> {
        Rc::new(Self { props: ActionProps::new(Self::ID, "", "", Some("separator"), false, None) })
    }

    /// Joins all non-empty lists of actions with separators.
    pub fn join(action_lists: &[Vec<ActionRef>]) -> Vec<ActionRef> {
        let mut out: Vec<ActionRef> = Vec::new();
        for list in action_lists {
            if list.is_empty() {
                // skip
            } else if !out.is_empty() {
                out.push(Separator::new());
                out.extend(list.iter().cloned());
            } else {
                out = list.clone();
            }
        }

        out
    }

    /// Removes leading, trailing, and consecutive duplicate separators in-place and returns the actions.
    pub fn clean(actions: &mut Vec<ActionRef>) -> &mut Vec<ActionRef> {
        let is_separator = |action: &ActionRef| action.id() == Self::ID;
        while actions.first().is_some_and(is_separator) {
            actions.remove(0);
        }
        while actions.last().is_some_and(is_separator) {
            actions.pop();
        }
        for i in (0..actions.len().saturating_sub(1)).rev() {
            if is_separator(&actions[i]) && is_separator(&actions[i + 1]) {
                actions.remove(i + 1);
            }
        }
        actions
    }
}

impl IAction for Separator {
    action_props!(|_: &Separator, _| ());

    fn run(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        resolved_undefined()
    }
}

pub struct SubmenuAction {
    props: ActionProps,
    actions: Vec<ActionRef>,
}

impl SubmenuAction {
    pub fn new(id: &str, label: &str, actions: Vec<ActionRef>, css_class: Option<&str>) -> Rc<Self> {
        Rc::new(Self { props: ActionProps::new(id, label, "", css_class, true, None), actions })
    }

    pub fn actions(&self) -> &[ActionRef] {
        &self.actions
    }
}

impl IAction for SubmenuAction {
    action_props!(|_: &SubmenuAction, _| ());

    fn run(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        resolved_undefined()
    }
}

/// `EmptySubmenuAction`: a [`Action`] desabilitada `(empty)` de um submenu sem itens.
pub struct EmptySubmenuAction;

impl EmptySubmenuAction {
    pub const ID: &'static str = "vs.actions.empty";

    pub fn create() -> Rc<Action> {
        Action::new(Self::ID, &nls::localize("submenu.empty", "(empty)", &[]), "", false, None)
    }
}

/// O `run: Function` de [`to_action`]: recebe os argumentos da chamada.
pub type ToActionRun = Rc<dyn Fn(&[Dynamic]) -> MaybePromise<Dynamic>>;

pub struct ToActionProps {
    pub id: String,
    pub label: String,
    pub tooltip: Option<String>,
    pub enabled: Option<bool>,
    pub checked: Option<bool>,
    pub class: Option<String>,
    pub alt: Option<ActionRef>,
    pub run: ToActionRun,
}

struct ToAction {
    props: ActionProps,
    alt: Option<ActionRef>,
    run: ToActionRun,
}

impl IAction for ToAction {
    action_props!(|_: &ToAction, _| ());

    fn alt(&self) -> Option<ActionRef> {
        self.alt.clone()
    }

    fn run(&self, args: &[Dynamic]) -> Promise<Dynamic> {
        let (run, args) = (self.run.clone(), args.to_vec());
        async_fn_maybe(async move { Ok(run(&args)) })
    }
}

pub fn to_action(props: ToActionProps) -> ActionRef {
    let tooltip = props.tooltip.unwrap_or_else(|| props.label.clone());
    Rc::new(ToAction {
        props: ActionProps::new(&props.id, &props.label, &tooltip, props.class.as_deref(), props.enabled.unwrap_or(true), props.checked),
        alt: props.alt,
        run: props.run,
    })
}
