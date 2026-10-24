//! Porte de `src/vs/editor/contrib/codeAction/browser/codeAction.ts`: a coleta das ações de código dos provedores
//! (`getCodeActions`), a aplicação de uma ação (`applyCodeAction`) e o corpo do comando `_executeCodeActionProvider`.
//!
//! Decisões de tradução:
//! - As funções `async` são [`async_fn`]: o corpo roda já, de forma síncrona, até o primeiro `await`, como no motor
//!   JS; `throw` é o `Err` devolvido, e o `try`/`catch`/`finally` é um `match` sobre o resultado com o `finally` depois
//!   dele.
//! - `getCodeActions(registry, ...)` recebe o registro como `Rc` e o `progress` como `Rc<dyn IProgress<...>>`, porque o
//!   corpo `async` e o ouvinte de `onDidChange` os usam depois do retorno. O `Progress.None` do chamador é
//!   [`ProgressNone`](super::platform::ProgressNone).
//! - `ManagedCodeActionSet extends Disposable` é a estrutura [`ManagedCodeActionSet`], dona do [`DisposableStore`] dos
//!   provedores. O `disposables.add(managedCodeActionSet)` do original, que faz o conjunto entrar no próprio
//!   repositório, não existe: o conjunto só seria descartado por quem descarta o repositório, e ninguém mais o tem; o
//!   ciclo `Rc` seria vazamento.
//! - A chamada ao provedor (`await provider.provideCodeActions(...)`) fica dentro do `try`: um pânico dela é pego
//!   por [`errors::catch`] e tratado como o `throw` síncrono do provedor.
//! - `emptyCodeActionsResponse` é um valor novo a cada uso.
//! - `applyCodeAction(accessor, ...)` recebe os cinco serviços que o original lê do `ServicesAccessor` como
//!   [`ApplyCodeActionServices`]. O `reason` do `IBulkEditOptions` (`EditSources.codeAction({ kind, providerId })`) é o
//!   [`TextModelEditSource`] de origem `codeAction`: a representação do porte guarda só `source` e `name`, e o `kind`
//!   da ação vai em `name`; o `providerId` (a extensão do provedor) não tem campo e não é guardado.
//! - O `asMessage(err)` do original devolve o texto do erro ou `undefined`; como toda rejeição do porte é um
//!   [`Error`](je_base::errors::Error) com mensagem, o texto de reserva ("An unknown error occurred while applying
//!   the code action") não é alcançável e não existe aqui.
//! - `CommandsRegistry.registerCommand('_executeCodeActionProvider', ...)` registra o comando na carga do módulo e lê
//!   `ILanguageFeaturesService` e `IModelService`, que o porte não tem; o corpo do comando é
//!   [`execute_code_action_provider`], que recebe o registro e o modelo já resolvidos (o `getModel(resource)`, `None`
//!   é o `illegalArgument`). `resource instanceof URI` e `Selection.isISelection`/`Range.isIRange` são o tipo dos
//!   argumentos; `itemResolveCount` ausente é `None`.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error, is_cancellation_error, on_unexpected_external_error};
use je_base::hierarchical_kind::HierarchicalKind;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_platform::commands::ICommandService;
use je_platform::notification::INotificationService;
use je_platform::telemetry::ITelemetryService;

use super::languages::{
    CodeAction, CodeActionContext, CodeActionProvider, CodeActionRef, CodeActionTriggerType, Command,
};
use super::platform::{
    AccessibilitySignal, ActionSet, BulkEditOptions, IAccessibilitySignalService, IBulkEditService, IProgress,
    ProgressNone,
};
use super::types::{
    CodeActionFilter, CodeActionItem, CodeActionKind, CodeActionSet, CodeActionTrigger, CodeActionTriggerSource,
    filters_action, may_include_actions_of_kind,
};
use crate::code_editor_widget::{CodeEditorWidget, SelectionOrRange};
use crate::editor_state::TextModelCancellationTokenSource;
use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::text_model::TextModel;
use crate::text_model_edit_source::TextModelEditSource;

pub const CODE_ACTION_COMMAND_ID: &str = "editor.action.codeAction";
pub const QUICK_FIX_COMMAND_ID: &str = "editor.action.quickFix";
pub const AUTO_FIX_COMMAND_ID: &str = "editor.action.autoFix";
pub const REFACTOR_COMMAND_ID: &str = "editor.action.refactor";
pub const REFACTOR_PREVIEW_COMMAND_ID: &str = "editor.action.refactor.preview";
pub const SOURCE_ACTION_COMMAND_ID: &str = "editor.action.sourceAction";
pub const ORGANIZE_IMPORTS_COMMAND_ID: &str = "editor.action.organizeImports";
pub const FIX_ALL_COMMAND_ID: &str = "editor.action.fixAll";
const CODE_ACTION_SOUND_APPLIED_DURATION: u64 = 1000;

/// O atraso, em milissegundos, até o `progress` ser avisado de que um provedor demora.
const PROGRESS_REPORT_DELAY: u64 = 1250;

/// O atraso do `setTimeout` que descarta o conjunto do `_executeCodeActionProvider`.
const EXECUTE_DISPOSE_DELAY: u64 = 100;

/// `ManagedCodeActionSet`.
pub struct ManagedCodeActionSet {
    all_actions: Vec<Rc<CodeActionItem>>,
    valid_actions: Vec<Rc<CodeActionItem>>,
    documentation: Vec<Command>,
    disposables: Rc<DisposableStore>,
}

impl ManagedCodeActionSet {
    /// `codeActionsPreferredComparator(a, b)`.
    fn code_actions_preferred_comparator(a: &CodeAction, b: &CodeAction) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (a.is_preferred.unwrap_or(false), b.is_preferred.unwrap_or(false)) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => Ordering::Equal,
        }
    }

    /// `codeActionsComparator({ action: a }, { action: b })`.
    fn code_actions_comparator(a: &Rc<CodeActionItem>, b: &Rc<CodeActionItem>) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let (a, b) = (a.action.borrow(), b.action.borrow());
        match (a.is_ai.unwrap_or(false), b.is_ai.unwrap_or(false)) {
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        let has_diagnostics = |action: &CodeAction| action.diagnostics.as_ref().is_some_and(|diagnostics| !diagnostics.is_empty());
        if has_diagnostics(&a) {
            if has_diagnostics(&b) { Self::code_actions_preferred_comparator(&a, &b) } else { Ordering::Less }
        } else if has_diagnostics(&b) {
            Ordering::Greater
        } else {
            Self::code_actions_preferred_comparator(&a, &b) // both have no diagnostics
        }
    }

    pub fn new(actions: &[Rc<CodeActionItem>], documentation: Vec<Command>, disposables: Rc<DisposableStore>) -> Self {
        let mut all_actions = actions.to_vec();
        all_actions.sort_by(Self::code_actions_comparator);
        let valid_actions = all_actions.iter().filter(|item| item.action.borrow().disabled.is_none()).cloned().collect();
        Self { all_actions, valid_actions, documentation, disposables }
    }
}

impl ActionSet<Rc<CodeActionItem>> for ManagedCodeActionSet {
    fn valid_actions(&self) -> Vec<Rc<CodeActionItem>> {
        self.valid_actions.clone()
    }

    fn all_actions(&self) -> Vec<Rc<CodeActionItem>> {
        self.all_actions.clone()
    }

    fn has_auto_fix(&self) -> bool {
        self.valid_actions.iter().any(|item| {
            let fix = item.action.borrow();
            fix.kind.as_deref().is_some_and(|kind| {
                !kind.is_empty() && CodeActionKind::quick_fix().contains(&HierarchicalKind::new(kind))
            }) && fix.is_preferred.unwrap_or(false)
        })
    }

    fn has_ai_fix(&self) -> bool {
        self.valid_actions.iter().any(|item| item.action.borrow().is_ai.unwrap_or(false))
    }

    fn all_ai_fixes(&self) -> bool {
        self.valid_actions.iter().all(|item| item.action.borrow().is_ai.unwrap_or(false))
    }
}

impl CodeActionSet for ManagedCodeActionSet {
    fn documentation(&self) -> Vec<Command> {
        self.documentation.clone()
    }
}

impl IDisposable for ManagedCodeActionSet {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// O que cada provedor devolve a `getCodeActions`: `{ actions, documentation }`.
#[derive(Clone)]
struct ProviderResponse {
    actions: Vec<Rc<CodeActionItem>>,
    documentation: Option<Command>,
}

/// `emptyCodeActionsResponse`.
fn empty_code_actions_response() -> ProviderResponse {
    ProviderResponse { actions: Vec::new(), documentation: None }
}

/// `equals(newProviders, providers)` de `arrays.ts` com a comparação `===`: o mesmo provedor, na mesma ordem.
fn same_providers(a: &[Rc<dyn CodeActionProvider>], b: &[Rc<dyn CodeActionProvider>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| Rc::ptr_eq(a, b))
}

/// `getCodeActions(registry, model, rangeOrSelection, trigger, progress, token)`.
pub fn get_code_actions(
    registry: &Rc<LanguageFeatureRegistry<dyn CodeActionProvider>>,
    model: &Rc<TextModel>,
    range_or_selection: SelectionOrRange,
    trigger: &CodeActionTrigger,
    progress: Rc<dyn IProgress<Rc<dyn CodeActionProvider>>>,
    token: &CancellationToken,
) -> Promise<Rc<dyn CodeActionSet>> {
    let registry = Rc::clone(registry);
    let model = Rc::clone(model);
    let trigger = trigger.clone();
    let token = token.clone();
    async_fn(async move {
        let filter = trigger.filter.clone().unwrap_or_default();
        let mut notebook_excludes = filter.excludes.clone().unwrap_or_default();
        notebook_excludes.push(CodeActionKind::notebook());
        let notebook_filter = CodeActionFilter { excludes: Some(notebook_excludes), ..filter.clone() };

        let code_action_context = CodeActionContext {
            only: filter.include.as_ref().map(|include| include.value.clone()),
            trigger: trigger.r#type,
        };

        let cts = Rc::new(TextModelCancellationTokenSource::new(&model, Some(&token)));
        // if the trigger is auto (autosave, lightbulb, etc), we should exclude notebook codeActions
        let exclude_notebook_code_actions = trigger.r#type == CodeActionTriggerType::Auto;
        let providers = get_code_action_providers(
            &registry,
            &model,
            if exclude_notebook_code_actions { &notebook_filter } else { &filter },
        );

        let disposables = DisposableStore::new();
        let promises: Vec<Promise<ProviderResponse>> = providers
            .iter()
            .map(|provider| {
                get_provider_response(
                    Rc::clone(provider),
                    Rc::clone(&model),
                    range_or_selection,
                    code_action_context.clone(),
                    filter.clone(),
                    Rc::clone(&progress),
                    cts.token(),
                    Rc::clone(&disposables),
                )
            })
            .collect();

        let on_did_change = registry.on_did_change();
        let listener = {
            let (registry, model, providers, cts) =
                (Rc::clone(&registry), Rc::clone(&model), providers.clone(), Rc::clone(&cts));
            on_did_change.listen(move |_| {
                let new_providers = registry.all(&model);
                if !same_providers(&new_providers, &providers) {
                    cts.cancel();
                }
            })
        };

        let result = async {
            let actions = Promise::all(promises).await?;
            let all_actions: Vec<Rc<CodeActionItem>> = actions.iter().flat_map(|response| response.actions.clone()).collect();
            let mut all_documentation: Vec<Command> = actions.iter().filter_map(|response| response.documentation.clone()).collect();
            all_documentation.extend(get_additional_documentation_for_showing_actions(&registry, &model, &trigger, &all_actions));
            let managed_code_action_set: Rc<dyn CodeActionSet> =
                Rc::new(ManagedCodeActionSet::new(&all_actions, all_documentation, Rc::clone(&disposables)));
            Ok::<_, Error>(managed_code_action_set)
        }
        .await;
        let result = result.inspect_err(|_| disposables.dispose());
        listener.dispose();
        cts.dispose();
        result
    })
}

/// O corpo do `map(async provider => ...)` de `getCodeActions`: pede as ações a um provedor.
#[allow(clippy::too_many_arguments)] // as variáveis que o fecho do original captura
fn get_provider_response(
    provider: Rc<dyn CodeActionProvider>,
    model: Rc<TextModel>,
    range_or_selection: SelectionOrRange,
    code_action_context: CodeActionContext,
    filter: CodeActionFilter,
    progress: Rc<dyn IProgress<Rc<dyn CodeActionProvider>>>,
    token: CancellationToken,
    disposables: Rc<DisposableStore>,
) -> Promise<ProviderResponse> {
    async_fn(async move {
        let handle = {
            let (progress, provider) = (Rc::clone(&progress), Rc::clone(&provider));
            runtime::set_timeout(move || progress.report(Rc::clone(&provider)), PROGRESS_REPORT_DELAY)
        };
        let result = async {
            let provided = match errors::catch(|| provider.provide_code_actions(&model, &range_or_selection, &code_action_context, &token)) {
                Ok(provided) => provided.await,
                Err(err) => Err(err),
            };
            let provided_code_actions = provided?;
            if token.is_cancellation_requested() {
                if let Some(provided_code_actions) = &provided_code_actions {
                    provided_code_actions.dispose();
                }
                return Ok(empty_code_actions_response());
            }

            if let Some(provided_code_actions) = &provided_code_actions {
                let disposable: DisposableRef = Rc::clone(provided_code_actions);
                disposables.add(disposable);
            }

            let filtered_actions: Vec<CodeActionRef> = provided_code_actions
                .map(|list| list.actions())
                .unwrap_or_default()
                .into_iter()
                .filter(|action| filters_action(&filter, &action.borrow()))
                .collect();
            let documentation = get_documentation_from_provider(&provider, &filtered_actions, filter.include.as_ref());
            Ok::<_, Error>(ProviderResponse {
                actions: filtered_actions
                    .into_iter()
                    .map(|action| CodeActionItem::new(action, Some(Rc::clone(&provider)), None))
                    .collect(),
                documentation,
            })
        }
        .await;
        runtime::clear_timeout(handle);
        match result {
            Err(err) if is_cancellation_error(&err) => Err(err),
            Err(err) => {
                on_unexpected_external_error(&err);
                Ok(empty_code_actions_response())
            }
            ok => ok,
        }
    })
}

/// `getCodeActionProviders(registry, model, filter)`.
fn get_code_action_providers(
    registry: &LanguageFeatureRegistry<dyn CodeActionProvider>,
    model: &Rc<TextModel>,
    filter: &CodeActionFilter,
) -> Vec<Rc<dyn CodeActionProvider>> {
    registry
        .all(model)
        .into_iter()
        // Don't include providers that we know will not return code actions of interest
        .filter(|provider| match provider.provided_code_action_kinds() {
            // We don't know what type of actions this provider will return.
            None => true,
            Some(kinds) => kinds.iter().any(|kind| may_include_actions_of_kind(filter, &HierarchicalKind::new(kind.as_str()))),
        })
        .collect()
}

/// `getAdditionalDocumentationForShowingActions(registry, model, trigger, actionsToShow)`.
fn get_additional_documentation_for_showing_actions(
    registry: &LanguageFeatureRegistry<dyn CodeActionProvider>,
    model: &Rc<TextModel>,
    trigger: &CodeActionTrigger,
    actions_to_show: &[Rc<CodeActionItem>],
) -> Vec<Command> {
    let mut documentation = Vec::new();
    if !actions_to_show.is_empty() {
        for provider in registry.all(model) {
            if provider.has_additional_menu_items() {
                let context = CodeActionContext {
                    trigger: trigger.r#type,
                    only: trigger.filter.as_ref().and_then(|filter| filter.include.as_ref()).map(|include| include.value.clone()),
                };
                let actions: Vec<CodeActionRef> = actions_to_show.iter().map(|item| Rc::clone(&item.action)).collect();
                documentation.extend(provider.get_additional_menu_items(&context, &actions));
            }
        }
    }
    documentation
}

/// `getDocumentationFromProvider(provider, providedCodeActions, only)`.
fn get_documentation_from_provider(
    provider: &Rc<dyn CodeActionProvider>,
    provided_code_actions: &[CodeActionRef],
    only: Option<&HierarchicalKind>,
) -> Option<Command> {
    let provider_documentation = provider.documentation()?;

    let documentation: Vec<(HierarchicalKind, Command)> = provider_documentation
        .into_iter()
        .map(|entry| (HierarchicalKind::new(entry.kind), entry.command))
        .collect();

    if let Some(only) = only {
        let mut current_best: Option<&(HierarchicalKind, Command)> = None;
        for entry in &documentation {
            if entry.0.contains(only) {
                match current_best {
                    None => current_best = Some(entry),
                    // Take best match
                    Some(best) if best.0.contains(&entry.0) => current_best = Some(entry),
                    Some(_) => {}
                }
            }
        }
        if let Some(best) = current_best {
            return Some(best.1.clone());
        }
    }

    // Otherwise, check to see if any of the provided actions match.
    for action in provided_code_actions {
        let action = action.borrow();
        let Some(kind) = action.kind.as_deref().filter(|kind| !kind.is_empty()) else {
            continue;
        };
        let kind = HierarchicalKind::new(kind);

        for entry in &documentation {
            if entry.0.contains(&kind) {
                return Some(entry.1.clone());
            }
        }
    }
    None
}

/// `ApplyCodeActionReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyCodeActionReason {
    OnSave,
    FromProblemsView,
    FromCodeActions,
    /// Direct invocation when clicking on the AI lightbulb.
    FromAiLightbulb,
    FromProblemsHover,
}

impl ApplyCodeActionReason {
    /// O valor de texto do original.
    pub fn value(self) -> &'static str {
        match self {
            Self::OnSave => "onSave",
            Self::FromProblemsView => "fromProblemsView",
            Self::FromCodeActions => "fromCodeActions",
            Self::FromAiLightbulb => "fromAILightbulb",
            Self::FromProblemsHover => "fromProblemsHover",
        }
    }
}

/// Os serviços que `applyCodeAction` lê do `ServicesAccessor`.
#[derive(Clone)]
pub struct ApplyCodeActionServices {
    pub bulk_edit_service: Rc<dyn IBulkEditService>,
    pub command_service: Rc<dyn ICommandService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,
}

/// O `options` de `applyCodeAction`: `{ preview?, editor? }`.
#[derive(Clone, Default)]
pub struct ApplyCodeActionOptions {
    pub preview: Option<bool>,
    pub editor: Option<Rc<CodeEditorWidget>>,
}

/// `applyCodeAction(accessor, item, codeActionReason, options, token)`.
pub fn apply_code_action(
    services: &ApplyCodeActionServices,
    item: &Rc<CodeActionItem>,
    code_action_reason: ApplyCodeActionReason,
    options: Option<ApplyCodeActionOptions>,
    token: &CancellationToken,
) -> Promise<()> {
    let services = services.clone();
    let item = Rc::clone(item);
    let token = token.clone();
    async_fn(async move {
        let (title, kind, is_preferred) = {
            let action = item.action.borrow();
            (action.title.clone(), action.kind.clone(), action.is_preferred.unwrap_or(false))
        };

        services.telemetry_service.public_log2(
            "codeAction.applyCodeAction",
            Some(Dynamic::object([
                ("codeActionTitle", Dynamic::string(&title)),
                ("codeActionKind", kind.as_deref().map_or(Dynamic::Undefined, Dynamic::string)),
                ("codeActionIsPreferred", Dynamic::Bool(is_preferred)),
                ("reason", Dynamic::string(code_action_reason.value())),
            ])),
        );
        services.accessibility_signal_service.play_signal(AccessibilitySignal::CodeActionTriggered);
        item.resolve(&token).await?;
        if token.is_cancellation_requested() {
            return Ok(());
        }

        let edit = item.action.borrow().edit.clone().filter(|edit| !edit.edits.is_empty());
        if let Some(edit) = edit {
            let result = services
                .bulk_edit_service
                .apply(
                    &edit,
                    BulkEditOptions {
                        editor: options.as_ref().and_then(|options| options.editor.clone()),
                        label: Some(title.clone()),
                        quotable_label: Some(title.clone()),
                        code: Some("undoredo.codeAction".to_owned()),
                        respect_auto_save_config: Some(code_action_reason != ApplyCodeActionReason::OnSave),
                        show_preview: options.as_ref().and_then(|options| options.preview),
                        reason: Some(TextModelEditSource { source: "codeAction".to_owned(), name: kind.clone() }),
                    },
                )
                .await?;

            if !result.is_applied {
                return Ok(());
            }
        }

        let command = item.action.borrow().command.clone();
        if let Some(command) = command {
            if let Err(err) = services.command_service.execute_command(&command.id, command.arguments.unwrap_or_default()).await {
                services.notification_service.error(vec![err.message.into()]);
            }
        }
        // ensure the start sound and end sound do not overlap
        let signal_service = Rc::clone(&services.accessibility_signal_service);
        runtime::set_timeout(
            move || signal_service.play_signal(AccessibilitySignal::CodeActionApplied),
            CODE_ACTION_SOUND_APPLIED_DURATION,
        );
        Ok(())
    })
}

/// O corpo do comando `_executeCodeActionProvider`: `resource`, `rangeOrSelection`, `kind` e `itemResolveCount`
/// chegam já como o modelo (`None` é o `illegalArgument`), o intervalo ou a seleção, o tipo e a quantidade.
pub fn execute_code_action_provider(
    registry: &Rc<LanguageFeatureRegistry<dyn CodeActionProvider>>,
    model: Option<&Rc<TextModel>>,
    range_or_selection: SelectionOrRange,
    kind: Option<&str>,
    item_resolve_count: Option<usize>,
) -> Promise<Vec<CodeActionRef>> {
    let Some(model) = model else {
        return Promise::rejected(Error::new("Illegal argument"));
    };
    let registry = Rc::clone(registry);
    let model = Rc::clone(model);
    let include = kind.map(HierarchicalKind::new);
    async_fn(async move {
        let validated_range_or_selection = match range_or_selection {
            SelectionOrRange::Selection(selection) => SelectionOrRange::Selection(selection),
            SelectionOrRange::Range(range) => SelectionOrRange::Range(model.validate_range(&range)),
        };

        let mut trigger = CodeActionTrigger::new(CodeActionTriggerType::Invoke, CodeActionTriggerSource::Default);
        trigger.filter = Some(CodeActionFilter { include_source_actions: Some(true), include, ..CodeActionFilter::default() });
        let code_action_set = get_code_actions(
            &registry,
            &model,
            validated_range_or_selection,
            &trigger,
            Rc::new(ProgressNone),
            &CancellationToken::None,
        )
        .await?;

        let valid_actions = code_action_set.valid_actions();
        let resolve_count = valid_actions.len().min(item_resolve_count.unwrap_or(0));
        let resolving: Vec<Promise<Rc<CodeActionItem>>> =
            valid_actions.iter().take(resolve_count).map(|item| item.resolve(&CancellationToken::None)).collect();

        let result = Promise::all(resolving).await;
        let to_dispose = Rc::clone(&code_action_set);
        runtime::set_timeout(move || to_dispose.dispose(), EXECUTE_DISPOSE_DELAY);
        result?;
        Ok(valid_actions.iter().map(|item| Rc::clone(&item.action)).collect())
    })
}
