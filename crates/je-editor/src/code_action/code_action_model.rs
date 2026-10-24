//! Porte de `src/vs/editor/contrib/codeAction/browser/codeActionModel.ts`: o modelo que decide quando pedir as ações
//! de código (o `CodeActionOracle`, que escuta o cursor e os marcadores) e guarda o estado do pedido em andamento
//! (`CodeActionsState`).
//!
//! Decisões de tradução:
//! - `class X extends Disposable` é a composição com um [`Disposable`] (`base`); os `Disposable` filhos
//!   (`MutableDisposable`, `Emitter`, `TimeoutTimer`) entram nele com `register`. Os ouvintes capturam um [`Weak`] do
//!   dono, como nas outras contribuições, e somem se o dono já foi solto.
//! - `SUPPORTED_CODE_ACTIONS` é a função [`supported_code_actions`] (a chave é por thread, como as outras
//!   `RawContextKey`).
//! - `CodeActionsState` é o `enum` [`CodeActionsState`] (`Empty` e `Triggered`); `CodeActionsState.Type` é
//!   [`CodeActionsStateType`] e `state.type` é [`CodeActionsState::r#type`]. A comparação por identidade
//!   (`newState === this._state`) é a igualdade de `Rc` do `Triggered`; `Empty` é um valor único.
//! - O `Triggered` guarda a promessa cancelável do pedido (`_cancellablePromise`) e a promessa derivada `actions`, que
//!   troca o cancelamento pelo conjunto vazio.
//! - `this._state.position !== newState.position` compara dois objetos `Position` distintos (cada pedido cria o seu com
//!   `getStartPosition()`), então é sempre verdadeiro; aqui só sobra a comparação dos tipos de gatilho.
//! - O objeto literal que `_setCodeActions` recebe é [`PlainCodeActionSet`]; `emptyCodeActionSet` é um conjunto vazio
//!   novo a cada uso (a identidade do congelado não é lida por ninguém).
//! - O que o fecho do `createCancelablePromise` lê de `this` (o registro, o serviço de marcadores, a configuração, o
//!   editor e o `MutableDisposable` das ações, que é o `codeActionsDisposable` do original) é o
//!   [`CodeActionModelServices`], copiado por `Rc` para dentro do corpo `async` ([`async_fn`]). O `_setCodeActions` e
//!   o `_settingEnabledNearbyQuickfixes` moram nele. O `try`/`finally` do desvio de correções vizinhas é o `match`
//!   sobre o resultado, com o descarte do `codeActionSets` depois dele.
//! - Os dois ramos `if (trigger.type === Invoke)` e o final do pedido, que no original são o mesmo código
//!   (`getCodeActions`, cancelamento e `_setCodeActions`), são um só trecho.
//! - `marker.relatedInformation` truthy é o `Option` de `related_information` preenchido; o `IMarker` que o original
//!   atribui a `diagnostics` (um `IMarkerData`) vira [`MarkerData`] sem o dono e o recurso.
//! - `/\s/.test(x)` é [`is_js_whitespace`] (os caracteres do `\s` do JavaScript); um índice fora da linha é
//!   `undefined`, e `/\s/.test(undefined)` é falso.
//! - `isEqual(resource, model.uri)` é o `ExtUri` sem viés, como o `resources.ts`.
//! - `_delay: number = 250` é `Option<u64>` no construtor do oráculo, e o `ignoreLightbulbOff` do modelo é
//!   [`CodeActionModel::set_ignore_lightbulb_off`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{TimeoutTimer, async_fn, create_cancelable_promise};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{Error, is_cancellation_error};
use je_base::event::{Emitter, Event};
use je_base::hierarchical_kind::HierarchicalKind;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable, id_of};
use je_base::promise::{CancelablePromise, Promise};
use je_base::resources::ExtUri;
use je_base::runtime;
use je_base::uri::URI;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};
use je_platform::markers::{IMarkerService, Marker, MarkerData, MarkerReadOptions};
use je_platform::resources::host_environment;

use super::code_action::get_code_actions;
use super::languages::{CodeActionProvider, CodeActionTriggerType, Command};
use super::platform::{ActionSet, IEditorProgressService, ProgressNone};
use super::types::{
    CodeActionFilter, CodeActionItem, CodeActionKind, CodeActionSet, CodeActionTrigger, CodeActionTriggerContext,
    CodeActionTriggerSource,
};
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor, SelectionOrRange};
use crate::config::editor_options::lightbulb::{LIGHTBULB, ShowLightbulbIconMode};
use crate::config::editor_options::registry::READ_ONLY;
use crate::cursor_common::CursorSimpleModel;
use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::position::Position;
use crate::selection::Selection;
use crate::text_model::TextModel;

thread_local! {
    static SUPPORTED_CODE_ACTIONS: RawContextKey = RawContextKey::new("supportedCodeAction", "", ContextKeyMeta::None);
}

/// `SUPPORTED_CODE_ACTIONS`.
pub fn supported_code_actions() -> RawContextKey {
    SUPPORTED_CODE_ACTIONS.with(RawContextKey::clone)
}

pub const APPLY_FIX_ALL_COMMAND_ID: &str = "_typescript.applyFixAllCodeAction";

/// O atraso padrão, em milissegundos, do gatilho automático do oráculo.
const DEFAULT_AUTO_TRIGGER_DELAY: u64 = 250;

/// O atraso, em milissegundos, até o estado automático substituir o manual.
const MANUAL_TO_AUTO_DELAY: u64 = 500;

/// O atraso, em milissegundos, até o indicador de progresso aparecer num pedido manual.
const PROGRESS_DELAY: u64 = 250;

/// `TriggeredCodeAction`.
#[derive(Clone, Debug)]
pub struct TriggeredCodeAction {
    pub selection: Selection,
    pub trigger: CodeActionTrigger,
}

/// `/\s/.test(char)` de uma unidade UTF-16: os caracteres do `\s` do JavaScript. Fora da linha (`undefined`) é falso.
fn is_js_whitespace(unit: Option<&u16>) -> bool {
    matches!(
        unit,
        Some(0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
    )
}

/// `CodeActionOracle`: dispara o pedido de ações quando o cursor ou os marcadores do modelo mudam.
struct CodeActionOracle {
    base: Rc<Disposable>,
    me: Weak<Self>,
    editor: Rc<CodeEditorWidget>,
    signal_change: Rc<dyn Fn(Option<TriggeredCodeAction>)>,
    delay: u64,
    auto_trigger_timer: Rc<TimeoutTimer>,
    ignore_lightbulb_off: Cell<bool>,
}

impl CodeActionOracle {
    fn new(
        editor: &Rc<CodeEditorWidget>,
        marker_service: &Rc<dyn IMarkerService>,
        signal_change: Rc<dyn Fn(Option<TriggeredCodeAction>)>,
        delay: Option<u64>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|me: &Weak<Self>| {
            let base = Disposable::new();
            let auto_trigger_timer = TimeoutTimer::new();
            base.register(auto_trigger_timer.clone());
            Self {
                base,
                me: me.clone(),
                editor: Rc::clone(editor),
                signal_change,
                delay: delay.unwrap_or(DEFAULT_AUTO_TRIGGER_DELAY),
                auto_trigger_timer,
                ignore_lightbulb_off: Cell::new(false),
            }
        });

        let me = Rc::downgrade(&this);
        this.base.register(marker_service.on_marker_changed().listen(move |resources: &Vec<URI>| {
            if let Some(this) = me.upgrade() {
                this.on_marker_changes(resources);
            }
        }));
        let me = Rc::downgrade(&this);
        this.base.register(editor.on_did_change_cursor_position().listen(move |_| {
            if let Some(this) = me.upgrade() {
                this.try_auto_trigger();
            }
        }));
        this
    }

    fn trigger(&self, trigger: CodeActionTrigger) {
        let selection = self.get_range_of_selection_unless_whitespace_enclosed(&trigger);
        (self.signal_change)(selection.map(|selection| TriggeredCodeAction { trigger, selection }));
    }

    fn on_marker_changes(&self, resources: &[URI]) {
        let Some(model) = self.editor.get_model() else {
            return;
        };
        let ext_uri = ExtUri::unbiased(host_environment());
        let model_uri = model.uri();
        if resources.iter().any(|resource| ext_uri.is_equal(Some(resource), Some(&model_uri), false)) {
            self.try_auto_trigger();
        }
    }

    fn try_auto_trigger(&self) {
        let me = self.me.clone();
        self.auto_trigger_timer.cancel_and_set(
            move || {
                if let Some(oracle) = me.upgrade() {
                    oracle.trigger(CodeActionTrigger::new(CodeActionTriggerType::Auto, CodeActionTriggerSource::Default));
                }
            },
            self.delay,
        );
    }

    fn get_range_of_selection_unless_whitespace_enclosed(&self, trigger: &CodeActionTrigger) -> Option<Selection> {
        if !self.editor.has_model() {
            return None;
        }
        let selection = self.editor.get_selection()?;
        if trigger.r#type == CodeActionTriggerType::Invoke {
            return Some(selection);
        }
        let enabled = self.editor.get_option(LIGHTBULB).enabled;
        if enabled == ShowLightbulbIconMode::Off && !self.ignore_lightbulb_off.get() {
            return None;
        } else if enabled == ShowLightbulbIconMode::Off || enabled == ShowLightbulbIconMode::On {
            return Some(selection);
        } else if enabled == ShowLightbulbIconMode::OnCode {
            let is_selection_empty = selection.is_empty();
            if !is_selection_empty {
                return Some(selection);
            }

            let model = self.editor.get_model()?;
            let Position { line_number, column } = selection.get_position();
            let line = model.get_line_content(line_number);
            let units = line.units();
            if units.is_empty() {
                // empty line
                return None;
            } else if column == 1 {
                // look only right
                if is_js_whitespace(units.first()) {
                    return None;
                }
            } else if column == model.get_line_max_column(line_number) {
                // look only left
                if is_js_whitespace(units.last()) {
                    return None;
                }
            } else if is_js_whitespace(units.get((column - 2) as usize)) && is_js_whitespace(units.get((column - 1) as usize)) {
                // look left and right
                return None;
            }
        }

        Some(selection)
    }
}

impl IDisposable for CodeActionOracle {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `CodeActionsState.Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeActionsStateType {
    Empty,
    Triggered,
}

/// `CodeActionsState.Triggered`.
pub struct CodeActionsTriggered {
    pub trigger: CodeActionTrigger,
    pub position: Position,
    pub actions: Promise<Rc<dyn CodeActionSet>>,
    cancellable_promise: CancelablePromise<Rc<dyn CodeActionSet>>,
}

impl CodeActionsTriggered {
    pub fn new(
        trigger: CodeActionTrigger,
        position: Position,
        cancellable_promise: CancelablePromise<Rc<dyn CodeActionSet>>,
    ) -> Self {
        let actions = cancellable_promise.then_result(|result| match result {
            Err(error) if is_cancellation_error(&error) => Ok(empty_code_action_set()),
            result => result,
        });
        Self { trigger, position, actions, cancellable_promise }
    }

    pub fn cancel(&self) {
        self.cancellable_promise.cancel();
    }
}

/// `CodeActionsState.State`: `Empty | Triggered`.
#[derive(Clone)]
pub enum CodeActionsState {
    Empty,
    Triggered(Rc<CodeActionsTriggered>),
}

impl CodeActionsState {
    /// `state.type`.
    pub fn r#type(&self) -> CodeActionsStateType {
        match self {
            Self::Empty => CodeActionsStateType::Empty,
            Self::Triggered(_) => CodeActionsStateType::Triggered,
        }
    }

    /// `a === b`.
    fn is_same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) => true,
            (Self::Triggered(a), Self::Triggered(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// O objeto literal `{ validActions, allActions, documentation, hasAutoFix, hasAIFix, allAIFixes, dispose }`.
struct PlainCodeActionSet {
    valid_actions: Vec<Rc<CodeActionItem>>,
    all_actions: Vec<Rc<CodeActionItem>>,
    documentation: Vec<Command>,
    has_auto_fix: bool,
    has_ai_fix: bool,
    all_ai_fixes: bool,
    dispose: Box<dyn Fn()>,
}

impl PlainCodeActionSet {
    /// Um conjunto com as listas dadas e as três marcas de `set`, que descarta com `dispose`.
    fn from_set(
        set: &Rc<dyn CodeActionSet>,
        valid_actions: Vec<Rc<CodeActionItem>>,
        all_actions: Vec<Rc<CodeActionItem>>,
        dispose: impl Fn() + 'static,
    ) -> Rc<dyn CodeActionSet> {
        Rc::new(Self {
            valid_actions,
            all_actions,
            documentation: set.documentation(),
            has_auto_fix: set.has_auto_fix(),
            has_ai_fix: set.has_ai_fix(),
            all_ai_fixes: set.all_ai_fixes(),
            dispose: Box::new(dispose),
        })
    }
}

impl ActionSet<Rc<CodeActionItem>> for PlainCodeActionSet {
    fn valid_actions(&self) -> Vec<Rc<CodeActionItem>> {
        self.valid_actions.clone()
    }

    fn all_actions(&self) -> Vec<Rc<CodeActionItem>> {
        self.all_actions.clone()
    }

    fn has_auto_fix(&self) -> bool {
        self.has_auto_fix
    }

    fn has_ai_fix(&self) -> bool {
        self.has_ai_fix
    }

    fn all_ai_fixes(&self) -> bool {
        self.all_ai_fixes
    }
}

impl CodeActionSet for PlainCodeActionSet {
    fn documentation(&self) -> Vec<Command> {
        self.documentation.clone()
    }
}

impl IDisposable for PlainCodeActionSet {
    fn dispose(&self) {
        (self.dispose)();
    }
}

/// `emptyCodeActionSet`.
fn empty_code_action_set() -> Rc<dyn CodeActionSet> {
    Rc::new(PlainCodeActionSet {
        all_actions: Vec::new(),
        valid_actions: Vec::new(),
        documentation: Vec::new(),
        has_auto_fix: false,
        has_ai_fix: false,
        all_ai_fixes: false,
        dispose: Box::new(|| {}),
    })
}

/// `IMarker` atribuído a um `IMarkerData`: os mesmos campos, sem o dono e o recurso.
fn marker_to_data(marker: &Marker) -> MarkerData {
    MarkerData {
        code: marker.code.clone(),
        severity: marker.severity,
        message: marker.message.clone(),
        source: marker.source.clone(),
        start_line_number: marker.start_line_number,
        start_column: marker.start_column,
        end_line_number: marker.end_line_number,
        end_column: marker.end_column,
        model_version_id: marker.model_version_id,
        related_information: marker.related_information.clone(),
        tags: marker.tags.clone(),
        origin: marker.origin.clone(),
    }
}

/// Se algum argumento do comando da ação é um texto que contém [`APPLY_FIX_ALL_COMMAND_ID`], troca os diagnósticos
/// da ação pelos marcadores que têm informação relacionada.
fn attach_fix_all_diagnostics(item: &Rc<CodeActionItem>, all_markers: &[Marker]) {
    let is_fix_all = item
        .action
        .borrow()
        .command
        .as_ref()
        .and_then(|command| command.arguments.as_ref())
        .is_some_and(|arguments| {
            arguments.iter().any(|arg| matches!(arg, Dynamic::String(text) if text.to_string_lossy().contains(APPLY_FIX_ALL_COMMAND_ID)))
        });
    if is_fix_all {
        item.action.borrow_mut().diagnostics =
            Some(all_markers.iter().filter(|marker| marker.related_information.is_some()).map(marker_to_data).collect());
    }
}

/// Os serviços que o pedido de ações lê do modelo (o `this` que o fecho do `createCancelablePromise` captura).
#[derive(Clone)]
struct CodeActionModelServices {
    editor: Rc<CodeEditorWidget>,
    registry: Rc<LanguageFeatureRegistry<dyn CodeActionProvider>>,
    marker_service: Rc<dyn IMarkerService>,
    configuration_service: Option<Rc<dyn IConfigurationService>>,
    /// `codeActionsDisposable`.
    code_actions_disposable: Rc<MutableDisposable<dyn IDisposable>>,
}

impl CodeActionModelServices {
    /// `_settingEnabledNearbyQuickfixes()`.
    fn setting_enabled_nearby_quickfixes(&self) -> bool {
        match &self.configuration_service {
            Some(configuration_service) => {
                let overrides = ConfigurationOverrides {
                    resource: self.editor.get_model().map(|model| model.uri()),
                    ..ConfigurationOverrides::default()
                };
                configuration_service
                    .get_value(Some("editor.codeActionWidget.includeNearbyQuickFixes"), Some(&overrides))
                    .is_truthy()
            }
            None => false,
        }
    }

    /// `_setCodeActions(actions)`.
    fn set_code_actions(&self, actions: Rc<dyn CodeActionSet>) -> Rc<dyn CodeActionSet> {
        let disposable: DisposableRef = actions.clone() as DisposableRef;
        self.code_actions_disposable.set(Some(disposable));
        actions
    }

    /// O pedido de ações de um gatilho: o `async token => { ... }` do `createCancelablePromise`.
    fn compute_code_actions(
        &self,
        model: &Rc<TextModel>,
        trigger: TriggeredCodeAction,
        token: CancellationToken,
    ) -> Promise<Rc<dyn CodeActionSet>> {
        let services = self.clone();
        let model = Rc::clone(model);
        async_fn(async move {
            let nearby_quick_fixes = trigger.trigger.r#type == CodeActionTriggerType::Invoke
                && (trigger.trigger.trigger_action == CodeActionTriggerSource::QuickFix
                    || trigger
                        .trigger
                        .filter
                        .as_ref()
                        .and_then(|filter| filter.include.as_ref())
                        .is_some_and(|include| include.contains(&CodeActionKind::quick_fix())));
            if services.setting_enabled_nearby_quickfixes() && nearby_quick_fixes {
                let code_action_sets = DisposableStore::new();
                let owned_by_result = Cell::new(false);
                let result = services.nearby_quick_fixes(&model, &trigger, &token, &code_action_sets, &owned_by_result).await;
                if !owned_by_result.get() {
                    code_action_sets.dispose();
                }
                if let Some(code_action_set) = result? {
                    return Ok(code_action_set);
                }
            }

            // Case for manual triggers - specifically Source Actions and Refactors, e para os automáticos
            let code_action_set = get_code_actions(
                &services.registry,
                &model,
                SelectionOrRange::Selection(trigger.selection),
                &trigger.trigger,
                Rc::new(ProgressNone),
                &token,
            )
            .await?;
            if token.is_cancellation_requested() {
                code_action_set.dispose();
                return Ok(empty_code_action_set());
            }
            Ok(services.set_code_actions(code_action_set))
        })
    }

    /// O primeiro `if` de `createCancelablePromise`: procura correções rápidas no conjunto do gatilho e, na falta delas,
    /// nas linhas dos marcadores. Devolve `Some` quando o original devolve (o conjunto vazio, se cancelado), e `None`
    /// quando o fluxo segue para o pedido comum.
    async fn nearby_quick_fixes(
        &self,
        model: &Rc<TextModel>,
        trigger: &TriggeredCodeAction,
        token: &CancellationToken,
        code_action_sets: &Rc<DisposableStore>,
        owned_by_result: &Cell<bool>,
    ) -> Result<Option<Rc<dyn CodeActionSet>>, Error> {
        let code_action_set = get_code_actions(
            &self.registry,
            model,
            SelectionOrRange::Selection(trigger.selection),
            &trigger.trigger,
            Rc::new(ProgressNone),
            token,
        )
        .await?;
        let disposable: DisposableRef = code_action_set.clone() as DisposableRef;
        code_action_sets.add(disposable);
        let mut all_code_actions = code_action_set.all_actions();
        if token.is_cancellation_requested() {
            return Ok(Some(empty_code_action_set()));
        }

        // Search for non-AI quickfixes in the current code action set - if AI code actions are the only thing found, continue searching for diagnostics in line.
        let valid_actions = code_action_set.valid_actions();
        let found_quickfix = valid_actions.iter().any(|item| {
            let action = item.action.borrow();
            action.kind.as_deref().is_some_and(|kind| {
                !kind.is_empty() && CodeActionKind::quick_fix().contains(&HierarchicalKind::new(kind))
            }) && !action.is_ai.unwrap_or(false)
        });
        let all_markers = self
            .marker_service
            .read(Some(&MarkerReadOptions { resource: Some(model.uri()), ..MarkerReadOptions::default() }));
        if found_quickfix {
            for item in &valid_actions {
                attach_fix_all_diagnostics(item, &all_markers);
            }
            let sets = Rc::clone(code_action_sets);
            let result = self.set_code_actions(PlainCodeActionSet::from_set(
                &code_action_set,
                valid_actions,
                all_code_actions,
                move || sets.dispose(),
            ));
            owned_by_result.set(true);
            return Ok(Some(result));
        }

        // If markers exist, and there are no quickfixes found or length is zero, check for quickfixes on that line.
        if all_markers.is_empty() {
            return Ok(None);
        }

        let curr_position = trigger.selection.get_position();
        let mut distance = i64::MAX;
        let mut current_actions = valid_actions;

        for marker in &all_markers {
            let col = marker.end_column;
            let row = marker.end_line_number;
            let start_row = marker.start_line_number;

            // Found quickfix on the same line and check relative distance to other markers
            if row == i64::from(curr_position.line_number) || start_row == i64::from(curr_position.line_number) {
                let tracked_position = Position::new(row as i32, col as i32);
                let new_code_action_trigger = CodeActionTrigger {
                    r#type: trigger.trigger.r#type,
                    trigger_action: trigger.trigger.trigger_action,
                    filter: Some(CodeActionFilter {
                        include: Some(
                            trigger
                                .trigger
                                .filter
                                .as_ref()
                                .and_then(|filter| filter.include.clone())
                                .unwrap_or_else(CodeActionKind::quick_fix),
                        ),
                        ..CodeActionFilter::default()
                    }),
                    auto_apply: trigger.trigger.auto_apply,
                    context: Some(CodeActionTriggerContext {
                        not_available_message: trigger
                            .trigger
                            .context
                            .as_ref()
                            .map(|context| context.not_available_message.clone())
                            .unwrap_or_default(),
                        position: tracked_position,
                    }),
                };

                let selection_as_position = Selection::new(
                    tracked_position.line_number,
                    tracked_position.column,
                    tracked_position.line_number,
                    tracked_position.column,
                );
                let actions_at_marker = get_code_actions(
                    &self.registry,
                    model,
                    SelectionOrRange::Selection(selection_as_position),
                    &new_code_action_trigger,
                    Rc::new(ProgressNone),
                    token,
                )
                .await?;
                let disposable: DisposableRef = actions_at_marker.clone() as DisposableRef;
                code_action_sets.add(disposable);
                if token.is_cancellation_requested() {
                    return Ok(Some(empty_code_action_set()));
                }

                let valid_actions_at_marker = actions_at_marker.valid_actions();
                if !valid_actions_at_marker.is_empty() {
                    for item in &valid_actions_at_marker {
                        attach_fix_all_diagnostics(item, &all_markers);
                    }

                    if code_action_set.all_actions().is_empty() {
                        all_code_actions.extend(actions_at_marker.all_actions());
                    }

                    // Already filtered through to only get quickfixes, so no need to filter again.
                    if (i64::from(curr_position.column) - col).abs() < distance {
                        let mut combined = valid_actions_at_marker;
                        combined.append(&mut current_actions);
                        current_actions = combined;
                    } else {
                        current_actions.extend(valid_actions_at_marker);
                    }
                }
                distance = (i64::from(curr_position.column) - col).abs();
            }
        }

        let mut filtered_actions: Vec<Rc<CodeActionItem>> = Vec::new();
        for item in &current_actions {
            let title = item.action.borrow().title.clone();
            if !filtered_actions.iter().any(|kept| kept.action.borrow().title == title) {
                filtered_actions.push(Rc::clone(item));
            }
        }

        filtered_actions.sort_by(|a, b| {
            use std::cmp::Ordering;
            let (a, b) = (a.action.borrow(), b.action.borrow());
            match (a.is_preferred.unwrap_or(false), b.is_preferred.unwrap_or(false), a.is_ai.unwrap_or(false), b.is_ai.unwrap_or(false)) {
                (true, false, _, _) => Ordering::Less,
                (false, true, _, _) => Ordering::Greater,
                (_, _, true, false) => Ordering::Greater,
                (_, _, false, true) => Ordering::Less,
                _ => Ordering::Equal,
            }
        });

        // Only retriggers if actually found quickfix on the same line as cursor
        let sets = Rc::clone(code_action_sets);
        let result = self.set_code_actions(PlainCodeActionSet::from_set(
            &code_action_set,
            filtered_actions,
            all_code_actions,
            move || sets.dispose(),
        ));
        owned_by_result.set(true);
        Ok(Some(result))
    }
}

/// `CodeActionModel`.
pub struct CodeActionModel {
    base: Rc<Disposable>,
    me: Weak<Self>,
    services: CodeActionModelServices,
    progress_service: Option<Rc<dyn IEditorProgressService>>,
    code_action_oracle: Rc<MutableDisposable<CodeActionOracle>>,
    state: RefCell<CodeActionsState>,
    supported_code_actions: Rc<dyn IContextKey>,
    on_did_change_state: Rc<Emitter<CodeActionsState>>,
    disposed: Cell<bool>,
    ignore_lightbulb_off: Cell<bool>,
}

impl CodeActionModel {
    pub fn new(
        editor: &Rc<CodeEditorWidget>,
        registry: &Rc<LanguageFeatureRegistry<dyn CodeActionProvider>>,
        marker_service: Rc<dyn IMarkerService>,
        context_key_service: &dyn IContextKeyService,
        progress_service: Option<Rc<dyn IEditorProgressService>>,
        configuration_service: Option<Rc<dyn IConfigurationService>>,
    ) -> Rc<Self> {
        let this = Rc::new_cyclic(|me: &Weak<Self>| {
            let base = Disposable::new();
            let code_action_oracle = MutableDisposable::<CodeActionOracle>::new();
            base.register(code_action_oracle.clone());
            let on_did_change_state = Emitter::new();
            base.register(on_did_change_state.clone());
            let code_actions_disposable = MutableDisposable::<dyn IDisposable>::new();
            base.register(code_actions_disposable.clone());
            Self {
                base,
                me: me.clone(),
                services: CodeActionModelServices {
                    editor: Rc::clone(editor),
                    registry: Rc::clone(registry),
                    marker_service,
                    configuration_service,
                    code_actions_disposable,
                },
                progress_service,
                code_action_oracle,
                state: RefCell::new(CodeActionsState::Empty),
                supported_code_actions: supported_code_actions().bind_to(context_key_service),
                on_did_change_state,
                disposed: Cell::new(false),
                ignore_lightbulb_off: Cell::new(false),
            }
        });

        this.register_listener(editor.on_did_change_model(), |this, _| this.update());
        this.register_listener(editor.on_did_change_model_language(), |this, _| this.update());
        this.register_listener(registry.on_did_change(), |this, _| this.update());
        this.register_listener(editor.on_did_change_configuration(), |this, e| {
            if e.has_changed(LIGHTBULB.id()) {
                this.update();
            }
        });
        this.update();
        this
    }

    /// `this._register(event(listener))`, com o callback recebendo o modelo (o `this` do original) em vez de
    /// capturá-lo: o `Weak` sobe a cada chamada, e a chamada some se o modelo já foi solto.
    fn register_listener<E: 'static>(self: &Rc<Self>, event: Event<E>, handler: impl Fn(&Self, &E) + 'static) {
        let me = Rc::downgrade(self);
        self.base.register(event.listen(move |e| {
            if let Some(this) = me.upgrade() {
                handler(&this, e);
            }
        }));
    }

    /// `onDidChangeState`.
    pub fn on_did_change_state(&self) -> Event<CodeActionsState> {
        self.on_did_change_state.event()
    }

    /// `set ignoreLightbulbOff(value)`.
    pub fn set_ignore_lightbulb_off(&self, value: bool) {
        if self.ignore_lightbulb_off.get() == value {
            return;
        }
        self.ignore_lightbulb_off.set(value);
        if let Some(oracle) = self.code_action_oracle.value() {
            oracle.ignore_lightbulb_off.set(value);
            if value {
                oracle.trigger(CodeActionTrigger::new(CodeActionTriggerType::Auto, CodeActionTriggerSource::Default));
            }
        }
    }

    fn update(&self) {
        if self.disposed.get() {
            return;
        }

        self.code_action_oracle.set(None);

        self.set_state(CodeActionsState::Empty, false);
        self.services.code_actions_disposable.clear();

        let editor = &self.services.editor;
        let registry = &self.services.registry;
        if let Some(model) = editor.get_model()
            && registry.has(&model)
            && !*editor.get_option(READ_ONLY)
        {
            let supported_actions: Vec<String> = registry
                .all(&model)
                .iter()
                .flat_map(|provider| provider.provided_code_action_kinds().unwrap_or_default())
                .collect();
            self.supported_code_actions.set(Dynamic::string(&supported_actions.join(" ")));

            let me = self.me.clone();
            let signal_model = Rc::clone(&model);
            let oracle = CodeActionOracle::new(
                editor,
                &self.services.marker_service,
                Rc::new(move |trigger: Option<TriggeredCodeAction>| {
                    if let Some(this) = me.upgrade() {
                        this.on_trigger(&signal_model, trigger);
                    }
                }),
                None,
            );
            oracle.ignore_lightbulb_off.set(self.ignore_lightbulb_off.get());
            self.code_action_oracle.set(Some(Rc::clone(&oracle)));
            oracle.trigger(CodeActionTrigger::new(CodeActionTriggerType::Auto, CodeActionTriggerSource::Default));
        } else {
            self.supported_code_actions.reset();
        }
    }

    /// O `signalChange` do oráculo: abre o pedido de ações do gatilho (ou esvazia o estado, sem gatilho).
    fn on_trigger(&self, model: &Rc<TextModel>, trigger: Option<TriggeredCodeAction>) {
        let Some(trigger) = trigger else {
            self.set_state(CodeActionsState::Empty, false);
            return;
        };

        let start_position = trigger.selection.get_start_position();

        let actions = {
            let services = self.services.clone();
            let model = Rc::clone(model);
            let trigger = trigger.clone();
            create_cancelable_promise(move |token| services.compute_code_actions(&model, trigger, token))
        };

        if trigger.trigger.r#type == CodeActionTriggerType::Invoke
            && let Some(progress_service) = &self.progress_service
        {
            progress_service.show_while(&actions.promise, Some(PROGRESS_DELAY));
        }
        let new_state = CodeActionsState::Triggered(Rc::new(CodeActionsTriggered::new(trigger.trigger, start_position, actions)));
        let mut is_manual_to_auto_transition = false;
        if let (CodeActionsState::Triggered(current), CodeActionsState::Triggered(incoming)) = (&*self.state.borrow(), &new_state) {
            // Check if the current state is manual and the new state is automatic
            is_manual_to_auto_transition = current.trigger.r#type == CodeActionTriggerType::Invoke
                && incoming.trigger.r#type == CodeActionTriggerType::Auto;
        }

        // Do not trigger state if current state is manual and incoming state is automatic
        if !is_manual_to_auto_transition {
            self.set_state(new_state, false);
        } else {
            // Reset the new state after getting code actions back.
            let me = self.me.clone();
            runtime::set_timeout(
                move || {
                    if let Some(this) = me.upgrade() {
                        this.set_state(new_state, false);
                    }
                },
                MANUAL_TO_AUTO_DELAY,
            );
        }
    }

    /// Transfers ownership of the current code actions to the caller.
    pub fn take_code_actions(&self, actions: &Rc<dyn CodeActionSet>) -> Option<Rc<dyn CodeActionSet>> {
        let current = self.services.code_actions_disposable.value()?;
        if id_of(&current) != id_of(actions) {
            return None;
        }
        self.services.code_actions_disposable.clear_and_leak();
        Some(Rc::clone(actions))
    }

    pub fn trigger(&self, trigger: CodeActionTrigger) {
        if let Some(oracle) = self.code_action_oracle.value() {
            oracle.trigger(trigger);
        }
        self.services.code_actions_disposable.clear();
    }

    fn set_state(&self, new_state: CodeActionsState, skip_notify: bool) {
        let old_state = self.state.borrow().clone();
        if new_state.is_same(&old_state) {
            return;
        }

        // Cancel old request
        if let CodeActionsState::Triggered(triggered) = &old_state {
            triggered.cancel();
        }

        *self.state.borrow_mut() = new_state.clone();

        if !skip_notify && !self.disposed.get() {
            self.on_did_change_state.fire(new_state);
        }
    }
}

impl IDisposable for CodeActionModel {
    fn dispose(&self) {
        if self.disposed.get() {
            return;
        }
        self.disposed.set(true);

        self.base.dispose();
        self.set_state(CodeActionsState::Empty, true);
    }
}
