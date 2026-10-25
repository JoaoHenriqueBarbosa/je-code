//! Porte de `src/vs/editor/contrib/folding/browser/folding.ts`: o controlador de dobras do editor
//! (`FoldingController`), o relator de limite (`RangesLimitReporter`), as vinte e seis ações `editor.fold*`,
//! `editor.unfold*`, `editor.toggleFold*`, `editor.foldLevelN`, `editor.gotoParentFold` e afins e as chamadas de
//! registro do fim do arquivo ([`register_folding`]).
//!
//! Representação:
//! - `FoldingController extends Disposable implements IEditorContribution` é a struct [`FoldingController`], criada por
//!   [`FoldingController::new`] (devolve `Rc<Self>`: os ouvintes, os agendadores e as promessas guardam um [`Weak`]).
//!   O `editor` é um [`Weak`] do widget (o editor é dono da contribuição). `FoldingController.get(editor)` é
//!   [`FoldingController::get`].
//! - Os cinco argumentos de injeção do construtor (`IContextKeyService`, `ILanguageConfigurationService`,
//!   `INotificationService`, `ILanguageFeatureDebounceService`, `ILanguageFeaturesService`) são o
//!   [`FoldingControllerServices`] mais o que o próprio editor já carrega (`language_configuration_service`). O
//!   `INotificationService` não é lido pelo original além do construtor e não existe aqui.
//! - `RangeProvider` é o `Rc<dyn RangeProvider>` de `folding/mod.rs`; o `CancelablePromise` do cálculo é o
//!   [`CancelablePromise`]; o `Delayer` é o [`Delayer`] com `Delay::Millis`.
//! - `abstract class FoldingAction<T>` e as dezenove subclasses mais as sete `FoldLevelAction` são a struct
//!   [`FoldingAction`] com o [`FoldingActionKind`] como dado (o `invoke` de cada subclasse é o `match` de
//!   [`FoldingActionKind::invoke`]). Os argumentos (`FoldingArguments`) são lidos do [`Dynamic`] da ação.
//! - `static _foldingRangeSelector` é o `thread_local` de [`FoldingController::set_folding_range_provider_selector`].
//! - `FoldingStateMemento` é [`FoldingStateMemento`], convertido de e para [`Dynamic`] no `saveViewState` e no
//!   `restoreViewState` que o widget guarda em `contributionsState`.
//!
//! Divergências:
//! - O widget do porte não tem os eventos `onMouseDown` e `onMouseUp` (só a `View` os produz). O tratamento do clique é
//!   exposto como [`FoldingController::on_editor_mouse_down`] e [`FoldingController::on_editor_mouse_up`], que quem
//!   recebe o evento de mouse chama. `e.target.element!.offsetLeft` (não há DOM) é o argumento `element_offset_left`.
//! - `reportTelemetry(accessor, editor)` não existe no porte e é omitido; `accessor.get(ILanguageConfigurationService)`
//!   é o serviço que o editor recebeu (como em `comment_actions`).
//! - `CommandsRegistry.registerCommand('_executeFoldingRangeProvider', ...)` depende de `IModelService` e
//!   `IConfigurationService` por `accessor.get`; o corpo é [`execute_folding_range_provider`], que recebe o modelo e os
//!   valores de `editor.folding`, `editor.foldingStrategy` e `editor.foldingMaximumRegions` já resolvidos, e quem
//!   registra o comando faz a resolução do recurso e da configuração.
//! - `registerColor` e `registerIcon` de `foldingDecorations.ts` não são repetidos: as cores estão na tabela de
//!   cores registradas de `je-theme` e os ícones são ids de [`ThemeIcon`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{Delayer, RunOnceScheduler, create_cancelable_promise};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, FunctionRef};
use je_base::errors::on_unexpected_error;
use je_base::event::{Emitter, Event};
use je_base::js_regexp::JsRegExp;
use je_base::key_codes::{KeyCode, KeyMod, key_chord};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::{CancelablePromise, MaybePromise, Promise};
use je_base::stopwatch::StopWatch;
use je_base::strings::escape_reg_exp_characters;
use je_base::symbols::Delay;
use je_base::types::TypeConstraint;
use je_instantiation::ServicesAccessor;
use je_platform::action::localize2;
use je_platform::commands::commands::{CommandArgument, CommandMetadata};
use je_platform::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings, PlatformKeybindings};

use super::folding_decorations::FoldingDecorationProvider;
use super::folding_model::{
    CollapseMemento, FoldingModel, LineMemento, RegionFilter, get_next_fold_line, get_parent_fold_line, get_previous_fold_line,
    set_collapse_state_at_level, set_collapse_state_for_matching_lines, set_collapse_state_for_rest, set_collapse_state_for_type,
    set_collapse_state_levels_down, set_collapse_state_levels_up, set_collapse_state_up, toggle_collapse_state,
};
use super::folding_ranges::{FoldRange, FoldSource, FoldingRegion, FoldingRegions};
use super::hidden_range_model::HiddenRangeModel;
use super::indent_range_provider::IndentRangeProvider;
use super::syntax_range_provider::{FoldingRange, FoldingRangeKind, FoldingRangeProvider, SyntaxRangeProvider};
use super::{FoldingLimitReporter, RangeProvider, to_selected_lines};
use crate::code_editor_widget::{
    CodeEditorWidget, EditorContributionInstantiation, ICodeEditor, IEditorContribution, SelectionOrRange,
};
use crate::config::editor_options::registry::{
    FOLDING_HIGHLIGHT, FOLDING_IMPORTS_BY_DEFAULT, FOLDING_MAXIMUM_REGIONS, FOLDING_STRATEGY, UNFOLD_ON_CLICK_AFTER_END_OF_LINE,
};
use crate::config::editor_options::simple_keys::{FOLDING, SHOW_FOLDING_CONTROLS};
use crate::config::editor_options::{ConfigurationChangedEvent, EditorOption};
use crate::core_commands::editor_text_focus;
use crate::cursor_common::CursorSimpleModel;
use crate::editor_browser::{EditorMouseEvent, MouseTarget, MouseTargetType};
use crate::editor_common::ScrollType;
use crate::editor_extensions::{
    ActionLabel, ActionOptions, CommandKeybindingsOptions, CommandOptions, EditorAction, EditorActionBase,
    register_editor_action, register_editor_contribution, register_instantiated_editor_action,
};
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::language_feature_debounce::{DebounceConfig, IFeatureDebounceInformation, ILanguageFeatureDebounceService};
use crate::language_feature_registry::AnyLanguageFeatureRegistry;
use crate::language_features_service::LanguageFeaturesService;
use crate::multicursor::listen_weak;
use crate::position::Position;
use crate::range::Range;
use crate::stable_editor_scroll::StableEditorScrollState;
use crate::text_model::TextModel;
use crate::text_model_events::ModelContentChangedEvent;

thread_local! {
    /// `CONTEXT_FOLDING_ENABLED`.
    static CONTEXT_FOLDING_ENABLED: RawContextKey = RawContextKey::new("foldingEnabled", false, ContextKeyMeta::None);

    /// `FoldingController._foldingRangeSelector`.
    static FOLDING_RANGE_SELECTOR: RefCell<Option<FoldingRangeProviderSelector>> = const { RefCell::new(None) };

    /// O registro de contribuição e ações acontece uma vez por thread.
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// `FoldingStateMemento`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FoldingStateMemento {
    pub collapsed_regions: Option<CollapseMemento>,
    pub line_count: Option<i32>,
    pub provider: Option<String>,
    pub folded_imports: Option<bool>,
}

fn number_of(value: &Dynamic) -> Option<f64> {
    match value {
        Dynamic::Number(n) => Some(*n),
        _ => None,
    }
}

fn arg(args: &Dynamic, key: &str) -> Dynamic {
    match args {
        Dynamic::Object(_) => args.get_str(key),
        _ => Dynamic::Undefined,
    }
}

impl FoldingStateMemento {
    fn line_memento_to_dynamic(memento: &LineMemento) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = vec![
            ("startLineNumber", Dynamic::Number(f64::from(memento.start_line_number))),
            ("endLineNumber", Dynamic::Number(f64::from(memento.end_line_number))),
        ];
        if let Some(checksum) = memento.checksum {
            entries.push(("checksum", Dynamic::Number(f64::from(checksum))));
        }
        if let Some(is_collapsed) = memento.is_collapsed {
            entries.push(("isCollapsed", Dynamic::Bool(is_collapsed)));
        }
        if let Some(source) = memento.source {
            entries.push(("source", Dynamic::Number(f64::from(source as u8))));
        }
        Dynamic::object(entries)
    }

    fn line_memento_from_dynamic(value: &Dynamic) -> Option<LineMemento> {
        let source = number_of(&arg(value, "source")).and_then(|n| match n as i64 {
            0 => Some(FoldSource::Provider),
            1 => Some(FoldSource::UserDefined),
            2 => Some(FoldSource::Recovered),
            _ => None,
        });
        Some(LineMemento {
            start_line_number: number_of(&arg(value, "startLineNumber"))? as u32,
            end_line_number: number_of(&arg(value, "endLineNumber"))? as u32,
            checksum: number_of(&arg(value, "checksum")).map(|n| n as i32),
            is_collapsed: match arg(value, "isCollapsed") {
                Dynamic::Bool(b) => Some(b),
                _ => None,
            },
            source,
        })
    }

    /// O valor que `saveViewState` entrega ao widget.
    pub fn to_dynamic(&self) -> Dynamic {
        let mut entries: Vec<(&str, Dynamic)> = Vec::new();
        if let Some(regions) = &self.collapsed_regions {
            entries.push(("collapsedRegions", Dynamic::array(regions.iter().map(Self::line_memento_to_dynamic))));
        }
        if let Some(line_count) = self.line_count {
            entries.push(("lineCount", Dynamic::Number(f64::from(line_count))));
        }
        if let Some(provider) = &self.provider {
            entries.push(("provider", Dynamic::from(provider.as_str())));
        }
        if let Some(folded_imports) = self.folded_imports {
            entries.push(("foldedImports", Dynamic::Bool(folded_imports)));
        }
        Dynamic::object(entries)
    }

    /// O inverso de [`FoldingStateMemento::to_dynamic`]; `None` se o valor não é um objeto.
    pub fn from_dynamic(value: &Dynamic) -> Option<FoldingStateMemento> {
        if !matches!(value, Dynamic::Object(_)) {
            return None;
        }
        let collapsed_regions = match arg(value, "collapsedRegions") {
            Dynamic::Array(items) => Some(items.to_vec().iter().filter_map(Self::line_memento_from_dynamic).collect()),
            _ => None,
        };
        Some(FoldingStateMemento {
            collapsed_regions,
            line_count: number_of(&arg(value, "lineCount")).map(|n| n as i32),
            provider: match arg(value, "provider") {
                Dynamic::String(s) => Some(s.to_string_lossy()),
                _ => None,
            },
            folded_imports: match arg(value, "foldedImports") {
                Dynamic::Bool(b) => Some(b),
                _ => None,
            },
        })
    }
}

/// `FoldingRangeProviderSelector`.
pub type FoldingRangeProviderSelector =
    Rc<dyn Fn(&[Rc<dyn FoldingRangeProvider>], &Rc<TextModel>) -> Option<Vec<Rc<dyn FoldingRangeProvider>>>>;

/// Os serviços que o construtor recebe por injeção (ver o topo do módulo).
pub struct FoldingControllerServices {
    /// `@IContextKeyService`.
    pub context_key_service: Rc<dyn IContextKeyService>,
    /// `@ILanguageFeaturesService`.
    pub language_features_service: Rc<LanguageFeaturesService>,
    /// `@ILanguageFeatureDebounceService`.
    pub language_feature_debounce_service: Rc<dyn ILanguageFeatureDebounceService>,
}

/// `RangesLimitReporter`.
pub struct RangesLimitReporter {
    editor: Weak<CodeEditorWidget>,
    on_did_change: Rc<Emitter<()>>,
    computed: Cell<usize>,
    limited: Cell<Option<usize>>,
}

impl RangesLimitReporter {
    pub fn new(editor: &Rc<CodeEditorWidget>) -> Rc<Self> {
        Rc::new(Self {
            editor: Rc::downgrade(editor),
            on_did_change: Emitter::new(),
            computed: Cell::new(0),
            limited: Cell::new(None),
        })
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `computed`.
    pub fn computed(&self) -> usize {
        self.computed.get()
    }

    /// `limited` (`false` é `None`).
    pub fn limited(&self) -> Option<usize> {
        self.limited.get()
    }
}

impl FoldingLimitReporter for RangesLimitReporter {
    fn limit(&self) -> usize {
        match self.editor.upgrade() {
            Some(editor) => *editor.get_options().get(FOLDING_MAXIMUM_REGIONS) as usize,
            None => 5000,
        }
    }

    fn update(&self, computed: usize, limited: Option<usize>) {
        if computed != self.computed.get() || limited != self.limited.get() {
            self.computed.set(computed);
            self.limited.set(limited);
            self.on_did_change.fire(());
        }
    }
}

impl IDisposable for RangesLimitReporter {
    fn dispose(&self) {
        self.on_did_change.dispose();
    }
}

fn take<T>(cell: &RefCell<Option<T>>) -> Option<T> {
    cell.borrow_mut().take()
}

/// `FoldingController`.
pub struct FoldingController {
    me: Weak<FoldingController>,
    base: Rc<Disposable>,
    editor: Weak<CodeEditorWidget>,
    services: FoldingControllerServices,

    is_enabled: Cell<bool>,
    use_folding_providers: Cell<bool>,
    unfold_on_click_after_end_of_line: Cell<bool>,
    restoring_view_state: Cell<bool>,
    folding_imports_by_default: Cell<bool>,
    current_model_has_folded_imports: Cell<bool>,

    folding_decoration_provider: Rc<FoldingDecorationProvider>,

    folding_model: RefCell<Option<Rc<FoldingModel>>>,
    hidden_range_model: RefCell<Option<Rc<HiddenRangeModel>>>,

    range_provider: RefCell<Option<Rc<dyn RangeProvider>>>,
    folding_region_promise: RefCell<Option<CancelablePromise<Option<Rc<FoldingRegions>>>>>,

    folding_model_promise: RefCell<Option<Promise<Option<Rc<FoldingModel>>>>>,
    update_scheduler: RefCell<Option<Rc<Delayer<Option<Rc<FoldingModel>>>>>>,
    update_debounce_info: Rc<dyn IFeatureDebounceInformation>,

    folding_enabled: Rc<dyn IContextKey>,
    cursor_changed_scheduler: RefCell<Option<Rc<RunOnceScheduler>>>,

    local_to_dispose: Rc<DisposableStore>,
    /// `{ lineNumber, iconClicked }`.
    mouse_down_info: Cell<Option<(i32, bool)>>,

    folding_limit_reporter: Rc<RangesLimitReporter>,
}

impl FoldingController {
    pub const ID: &'static str = "editor.contrib.folding";

    /// `FoldingController.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<FoldingController>> {
        editor.get_contribution_as::<FoldingController>(Self::ID)
    }

    /// `FoldingController.getFoldingRangeProviders(languageFeaturesService, model)`.
    pub fn get_folding_range_providers(
        language_features_service: &LanguageFeaturesService,
        model: &Rc<TextModel>,
    ) -> Vec<Rc<dyn FoldingRangeProvider>> {
        let folding_range_providers = language_features_service.folding_range_provider.ordered(model, false);
        let selector = FOLDING_RANGE_SELECTOR.with(|selector| selector.borrow().clone());
        selector.and_then(|selector| selector(&folding_range_providers, model)).unwrap_or(folding_range_providers)
    }

    /// `FoldingController.setFoldingRangeProviderSelector(foldingRangeSelector)`.
    pub fn set_folding_range_provider_selector(selector: FoldingRangeProviderSelector) -> DisposableRef {
        FOLDING_RANGE_SELECTOR.with(|slot| *slot.borrow_mut() = Some(selector));
        to_disposable(|| FOLDING_RANGE_SELECTOR.with(|slot| *slot.borrow_mut() = None))
    }

    pub fn new(editor: &Rc<CodeEditorWidget>, services: FoldingControllerServices) -> Rc<Self> {
        let base = Disposable::new();
        let folding_limit_reporter = RangesLimitReporter::new(editor);
        base.register(folding_limit_reporter.clone());

        let options = editor.get_options();
        let is_enabled = *options.get(FOLDING);
        let debounce_registry: Rc<dyn AnyLanguageFeatureRegistry> = services.language_features_service.folding_range_provider.clone();
        let update_debounce_info = services.language_feature_debounce_service.for_feature(
            debounce_registry,
            "Folding",
            Some(DebounceConfig { min: Some(200.0), ..DebounceConfig::default() }),
        );

        let folding_decoration_provider = Rc::new(FoldingDecorationProvider::new(editor));
        folding_decoration_provider.set_show_folding_controls(*options.get(SHOW_FOLDING_CONTROLS));
        folding_decoration_provider.set_show_folding_highlights(*options.get(FOLDING_HIGHLIGHT));
        let folding_enabled = CONTEXT_FOLDING_ENABLED.with(|key| key.bind_to(&*services.context_key_service));
        folding_enabled.set(Dynamic::Bool(is_enabled));

        let local_to_dispose = DisposableStore::new();
        base.register(local_to_dispose.clone());

        let this = Rc::new_cyclic(|me: &Weak<FoldingController>| {
            base.register(listen_weak(me, editor.on_did_change_model(), |this: &FoldingController, _| this.on_model_changed()));
            base.register(listen_weak(me, editor.on_did_change_configuration(), |this: &FoldingController, e| {
                this.on_configuration_changed(e);
            }));
            FoldingController {
                me: me.clone(),
                base: base.clone(),
                editor: Rc::downgrade(editor),
                services,
                is_enabled: Cell::new(is_enabled),
                use_folding_providers: Cell::new(options.get(FOLDING_STRATEGY).as_str() != "indentation"),
                unfold_on_click_after_end_of_line: Cell::new(*options.get(UNFOLD_ON_CLICK_AFTER_END_OF_LINE)),
                restoring_view_state: Cell::new(false),
                folding_imports_by_default: Cell::new(*options.get(FOLDING_IMPORTS_BY_DEFAULT)),
                current_model_has_folded_imports: Cell::new(false),
                folding_decoration_provider,
                folding_model: RefCell::new(None),
                hidden_range_model: RefCell::new(None),
                range_provider: RefCell::new(None),
                folding_region_promise: RefCell::new(None),
                folding_model_promise: RefCell::new(None),
                update_scheduler: RefCell::new(None),
                update_debounce_info,
                folding_enabled,
                cursor_changed_scheduler: RefCell::new(None),
                local_to_dispose,
                mouse_down_info: Cell::new(None),
                folding_limit_reporter,
            }
        });
        this.on_model_changed();
        this
    }

    /// `limitReporter` (e `_foldingLimitReporter`).
    pub fn limit_reporter(&self) -> &Rc<RangesLimitReporter> {
        &self.folding_limit_reporter
    }

    /// `saveViewState()`: `None` é o `undefined` (o modelo de dobras já foi descartado).
    pub fn save_folding_state(&self) -> Option<FoldingStateMemento> {
        let editor = self.editor.upgrade()?;
        let model = editor.get_model();
        let Some(model) = model.filter(|model| self.is_enabled.get() && !model.is_too_large_for_tokenization()) else {
            return Some(FoldingStateMemento::default());
        };
        let folding_model = self.folding_model.borrow().clone()?;
        let provider = self.range_provider.borrow().as_ref().map(|provider| provider.id().to_owned());
        Some(FoldingStateMemento {
            collapsed_regions: folding_model.get_memento(),
            line_count: Some(model.get_line_count()),
            provider,
            folded_imports: Some(self.current_model_has_folded_imports.get()),
        })
    }

    /// `restoreViewState(state)`.
    pub fn restore_folding_state(&self, state: &FoldingStateMemento) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let model = editor.get_model();
        let has_hidden_range_model = self.hidden_range_model.borrow().is_some();
        if !model.is_some_and(|model| self.is_enabled.get() && !model.is_too_large_for_tokenization()) || !has_hidden_range_model {
            return;
        }

        self.current_model_has_folded_imports.set(state.folded_imports.unwrap_or(false));
        let folding_model = self.folding_model.borrow().clone();
        if let (Some(collapsed_regions), Some(folding_model)) = (&state.collapsed_regions, folding_model) {
            if !collapsed_regions.is_empty() {
                self.restoring_view_state.set(true);
                folding_model.apply_memento(collapsed_regions);
                self.restoring_view_state.set(false);
            }
        }
    }

    fn on_configuration_changed(&self, e: &ConfigurationChangedEvent) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        if e.has_changed(EditorOption::Folding) {
            let enabled = *editor.get_options().get(FOLDING);
            self.is_enabled.set(enabled);
            self.folding_enabled.set(Dynamic::Bool(enabled));
            self.on_model_changed();
        }
        if e.has_changed(EditorOption::FoldingMaximumRegions) {
            self.on_model_changed();
        }
        if e.has_changed(EditorOption::ShowFoldingControls) || e.has_changed(EditorOption::FoldingHighlight) {
            let options = editor.get_options();
            self.folding_decoration_provider.set_show_folding_controls(*options.get(SHOW_FOLDING_CONTROLS));
            self.folding_decoration_provider.set_show_folding_highlights(*options.get(FOLDING_HIGHLIGHT));
            self.trigger_folding_model_changed();
        }
        if e.has_changed(EditorOption::FoldingStrategy) {
            self.use_folding_providers.set(editor.get_options().get(FOLDING_STRATEGY).as_str() != "indentation");
            self.on_folding_strategy_changed();
        }
        if e.has_changed(EditorOption::UnfoldOnClickAfterEndOfLine) {
            self.unfold_on_click_after_end_of_line.set(*editor.get_options().get(UNFOLD_ON_CLICK_AFTER_END_OF_LINE));
        }
        if e.has_changed(EditorOption::FoldingImportsByDefault) {
            self.folding_imports_by_default.set(*editor.get_options().get(FOLDING_IMPORTS_BY_DEFAULT));
        }
    }

    fn on_model_changed(&self) {
        self.local_to_dispose.clear();

        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let model = editor.get_model();
        let Some(model) = model.filter(|model| self.is_enabled.get() && !model.is_too_large_for_tokenization()) else {
            // huge files get no view model, so they cannot support hidden areas
            return;
        };

        self.current_model_has_folded_imports.set(false);
        let folding_model = FoldingModel::new(Rc::clone(&model), self.folding_decoration_provider.clone());
        self.local_to_dispose.add(folding_model.clone());
        *self.folding_model.borrow_mut() = Some(Rc::clone(&folding_model));

        let hidden_range_model = HiddenRangeModel::new(Rc::clone(&folding_model));
        self.local_to_dispose.add(hidden_range_model.clone());
        self.local_to_dispose.add(listen_weak(&self.me, hidden_range_model.on_did_change(), |this: &FoldingController, hidden_ranges| {
            this.on_hidden_ranges_changes(hidden_ranges);
        }));
        *self.hidden_range_model.borrow_mut() = Some(hidden_range_model);

        let update_scheduler = Rc::new(Delayer::new(Delay::Millis(self.update_debounce_info.get(&model) as u64)));
        self.local_to_dispose.add(update_scheduler.clone());
        *self.update_scheduler.borrow_mut() = Some(update_scheduler);

        let me = self.me.clone();
        let cursor_changed_scheduler = RunOnceScheduler::new(
            move || {
                if let Some(this) = me.upgrade() {
                    this.reveal_cursor();
                }
            },
            200,
        );
        self.local_to_dispose.add(cursor_changed_scheduler.clone());
        *self.cursor_changed_scheduler.borrow_mut() = Some(cursor_changed_scheduler);

        let folding_range_provider_registry = self.services.language_features_service.folding_range_provider.clone();
        self.local_to_dispose.add(listen_weak(&self.me, folding_range_provider_registry.on_did_change(), |this: &FoldingController, _| {
            this.on_folding_strategy_changed();
        }));
        // covers model language changes as well
        self.local_to_dispose.add(listen_weak(&self.me, editor.on_did_change_model_language_configuration(), |this: &FoldingController, _| {
            this.on_folding_strategy_changed();
        }));
        self.local_to_dispose.add(listen_weak(&self.me, editor.on_did_change_model_content(), |this: &FoldingController, e| {
            this.on_did_change_model_content(e);
        }));
        self.local_to_dispose.add(listen_weak(&self.me, editor.on_did_change_cursor_position(), |this: &FoldingController, _| {
            this.on_cursor_position_changed();
        }));
        let me = self.me.clone();
        self.local_to_dispose.add(to_disposable(move || {
            if let Some(this) = me.upgrade() {
                this.release_model_state();
            }
        }));
        self.trigger_folding_model_changed();
    }

    /// A função de descarte que `onModelChanged` registra em `localToDispose`.
    fn release_model_state(&self) {
        let region_promise = take(&self.folding_region_promise);
        if let Some(region_promise) = region_promise {
            region_promise.cancel();
        }
        let update_scheduler = take(&self.update_scheduler);
        if let Some(update_scheduler) = update_scheduler {
            update_scheduler.cancel();
        }
        *self.folding_model.borrow_mut() = None;
        *self.folding_model_promise.borrow_mut() = None;
        *self.hidden_range_model.borrow_mut() = None;
        *self.cursor_changed_scheduler.borrow_mut() = None;
        let range_provider = take(&self.range_provider);
        if let Some(range_provider) = range_provider {
            range_provider.dispose();
        }
    }

    fn on_folding_strategy_changed(&self) {
        let range_provider = take(&self.range_provider);
        if let Some(range_provider) = range_provider {
            range_provider.dispose();
        }
        self.trigger_folding_model_changed();
    }

    fn get_range_provider(&self, editor_model: &Rc<TextModel>) -> Rc<dyn RangeProvider> {
        let existing = self.range_provider.borrow().clone();
        if let Some(existing) = existing {
            return existing;
        }
        let limit: Rc<dyn FoldingLimitReporter> = self.folding_limit_reporter.clone();
        let language_configuration_service = self.editor.upgrade().expect("o editor vive enquanto o controlador existe").language_configuration_service();
        let indent_range_provider: Rc<dyn RangeProvider> =
            Rc::new(IndentRangeProvider::new(Rc::clone(editor_model), language_configuration_service, limit.clone()));
        let mut range_provider = Rc::clone(&indent_range_provider); // fallback
        if self.use_folding_providers.get() && self.folding_model.borrow().is_some() {
            let selected_providers = Self::get_folding_range_providers(&self.services.language_features_service, editor_model);
            if !selected_providers.is_empty() {
                let me = self.me.clone();
                let on_change: Rc<dyn Fn()> = Rc::new(move || {
                    if let Some(this) = me.upgrade() {
                        this.trigger_folding_model_changed();
                    }
                });
                range_provider = Rc::new(SyntaxRangeProvider::new(
                    Rc::clone(editor_model),
                    selected_providers,
                    on_change,
                    limit,
                    Some(indent_range_provider),
                ));
            }
        }
        *self.range_provider.borrow_mut() = Some(Rc::clone(&range_provider));
        range_provider
    }

    /// `getFoldingModel()`.
    pub fn get_folding_model(&self) -> Option<Promise<Option<Rc<FoldingModel>>>> {
        self.folding_model_promise.borrow().clone()
    }

    fn on_did_change_model_content(&self, e: &ModelContentChangedEvent) {
        let hidden_range_model = self.hidden_range_model.borrow().clone();
        if let Some(hidden_range_model) = hidden_range_model {
            hidden_range_model.notify_change_model_content(e);
        }
        self.trigger_folding_model_changed();
    }

    /// `triggerFoldingModelChanged()`.
    pub fn trigger_folding_model_changed(&self) {
        let update_scheduler = self.update_scheduler.borrow().clone();
        let Some(update_scheduler) = update_scheduler else {
            return;
        };
        let region_promise = take(&self.folding_region_promise);
        if let Some(region_promise) = region_promise {
            region_promise.cancel();
        }
        let me = self.me.clone();
        let promise = update_scheduler.trigger(
            move || match me.upgrade() {
                Some(this) => this.compute_folding_model(),
                None => MaybePromise::Value(None),
            },
            None,
        );
        let promise = promise.then_result(|result| match result {
            Ok(folding_model) => Ok(folding_model),
            Err(error) => {
                on_unexpected_error(&error);
                Ok(None)
            }
        });
        *self.folding_model_promise.borrow_mut() = Some(promise);
    }

    /// O corpo da tarefa do `updateScheduler.trigger(...)`.
    fn compute_folding_model(&self) -> MaybePromise<Option<Rc<FoldingModel>>> {
        let folding_model = self.folding_model.borrow().clone();
        let Some(folding_model) = folding_model else {
            // null if editor has been disposed, or folding turned off
            return MaybePromise::Value(None);
        };
        let stop_watch = StopWatch::new(None);
        let provider = self.get_range_provider(folding_model.text_model());
        let folding_region_promise = create_cancelable_promise(move |token| provider.compute(&token));
        *self.folding_region_promise.borrow_mut() = Some(folding_region_promise.clone());
        let me = self.me.clone();
        let current = folding_region_promise.clone();
        MaybePromise::Promise(folding_region_promise.promise.then(move |folding_ranges| {
            let Some(this) = me.upgrade() else {
                return Some(folding_model);
            };
            let is_current = this.folding_region_promise.borrow().as_ref().is_some_and(|promise| promise.promise.ptr_eq(&current.promise));
            if let (Some(folding_ranges), true) = (folding_ranges, is_current) {
                // new request or cancelled in the meantime?
                this.apply_folding_ranges(&folding_model, &folding_ranges);

                // update debounce info
                let new_value = this.update_debounce_info.update(folding_model.text_model(), stop_watch.elapsed());
                let update_scheduler = this.update_scheduler.borrow().clone();
                if let Some(update_scheduler) = update_scheduler {
                    update_scheduler.default_delay.set(Delay::Millis(new_value as u64));
                }
            }
            Some(folding_model)
        }))
    }

    fn apply_folding_ranges(&self, folding_model: &Rc<FoldingModel>, folding_ranges: &FoldingRegions) {
        let editor = self.editor.upgrade();
        let mut scroll_state: Option<StableEditorScrollState> = None;

        if self.folding_imports_by_default.get() && !self.current_model_has_folded_imports.get() {
            let has_changes = folding_ranges.set_collapsed_all_of_type(&FoldingRangeKind::imports().value, true);
            if has_changes {
                scroll_state = editor.as_ref().map(|editor| StableEditorScrollState::capture(editor));
                self.current_model_has_folded_imports.set(has_changes);
            }
        }

        // some cursors might have moved into hidden regions, make sure they are in expanded regions
        let selections = editor.as_ref().and_then(|editor| editor.get_selections());
        let selected_lines = to_selected_lines(selections.as_deref());
        folding_model.update(folding_ranges, Some(&selected_lines));

        if let (Some(scroll_state), Some(editor)) = (&scroll_state, &editor) {
            scroll_state.restore(editor);
        }
    }

    fn on_hidden_ranges_changes(&self, hidden_ranges: &Vec<Range>) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let hidden_range_model = self.hidden_range_model.borrow().clone();
        if let Some(hidden_range_model) = hidden_range_model {
            if !hidden_ranges.is_empty() && !self.restoring_view_state.get() {
                if let Some(mut selections) = editor.get_selections() {
                    if hidden_range_model.adjust_selections(&mut selections) {
                        editor.set_selections(&selections, None, None);
                    }
                }
            }
        }
        editor.set_hidden_areas(hidden_ranges, Some(self as *const FoldingController as usize), None);
    }

    fn on_cursor_position_changed(&self) {
        let has_ranges = self.hidden_range_model.borrow().as_ref().is_some_and(|hidden_range_model| hidden_range_model.has_ranges());
        if has_ranges {
            let scheduler = self.cursor_changed_scheduler.borrow().clone();
            if let Some(scheduler) = scheduler {
                scheduler.schedule(None);
            }
        }
    }

    fn reveal_cursor(&self) {
        let Some(folding_model) = self.get_folding_model() else {
            return;
        };
        let me = self.me.clone();
        folding_model.then_result(move |result| {
            match result {
                // null is returned if folding got disabled in the meantime
                Ok(Some(folding_model)) => {
                    if let Some(this) = me.upgrade() {
                        this.reveal_cursor_in(&folding_model);
                    }
                }
                Ok(None) => {}
                Err(error) => on_unexpected_error(&error),
            }
            Ok(())
        });
    }

    fn reveal_cursor_in(&self, folding_model: &Rc<FoldingModel>) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let Some(selections) = editor.get_selections().filter(|selections| !selections.is_empty()) else {
            return;
        };
        let hidden_range_model = self.hidden_range_model.borrow().clone();
        let mut to_toggle: Vec<FoldingRegion> = Vec::new();
        for selection in &selections {
            let line_number = selection.selection_start_line_number;
            if hidden_range_model.as_ref().is_some_and(|hidden_range_model| hidden_range_model.is_hidden(line_number)) {
                let filter = |region: &FoldingRegion, _level: usize| region.is_collapsed() && line_number as u32 > region.start_line_number();
                to_toggle.extend(folding_model.get_all_regions_at_line(line_number as u32, Some(&filter)));
            }
        }
        if !to_toggle.is_empty() {
            folding_model.toggle_collapse_state(to_toggle);
            self.reveal(selections[0].get_position());
        }
    }

    /// `onEditorMouseDown(e)`. `element_offset_left` é o `e.target.element!.offsetLeft` da margem (ver "Divergências").
    pub fn on_editor_mouse_down(&self, e: &EditorMouseEvent, element_offset_left: f64) {
        self.mouse_down_info.set(None);

        let hidden_range_model = self.hidden_range_model.borrow().clone();
        let Some(hidden_range_model) = hidden_range_model else {
            return;
        };
        let Some(range) = e.target.range() else {
            return;
        };
        if !e.event.left_button && !e.event.middle_button {
            return;
        }
        let mut icon_clicked = false;
        match &e.target {
            MouseTarget::Margin(margin) if margin.target_type == MouseTargetType::GutterLineDecorations => {
                let gutter_offset_x = margin.detail.offset_x - element_offset_left;

                // TODO@joao TODO@alex TODO@martin this is such that we don't collide with dirty diff
                if gutter_offset_x < 4.0 {
                    // the whitespace between the border and the real folding icon border is 4px
                    return;
                }

                icon_clicked = true;
            }
            MouseTarget::ContentEmpty(empty) => {
                if !(self.unfold_on_click_after_end_of_line.get() && hidden_range_model.has_ranges() && !empty.detail.is_after_lines) {
                    return;
                }
            }
            MouseTarget::ContentText(_) => {
                if !hidden_range_model.has_ranges() {
                    return;
                }
                let at_line_end = self
                    .editor
                    .upgrade()
                    .and_then(|editor| editor.get_model())
                    .is_some_and(|model| range.start_column == model.get_line_max_column(range.start_line_number));
                if !at_line_end {
                    return;
                }
            }
            _ => return,
        }

        self.mouse_down_info.set(Some((range.start_line_number, icon_clicked)));
    }

    /// `onEditorMouseUp(e)`.
    pub fn on_editor_mouse_up(&self, e: &EditorMouseEvent) {
        let folding_model = self.folding_model.borrow().clone();
        let (Some(folding_model), Some((line_number, icon_clicked))) = (folding_model, self.mouse_down_info.get()) else {
            return;
        };

        let Some(range) = e.target.range() else {
            return;
        };
        if range.start_line_number != line_number {
            return;
        }

        if icon_clicked {
            if e.target.target_type() != MouseTargetType::GutterLineDecorations {
                return;
            }
        } else {
            let model = self.editor.upgrade().and_then(|editor| editor.get_model());
            if !model.is_some_and(|model| range.start_column == model.get_line_max_column(line_number)) {
                return;
            }
        }

        let Some(region) = folding_model.get_region_at_line(line_number as u32) else {
            return;
        };
        if region.start_line_number() != line_number as u32 {
            return;
        }
        let is_collapsed = region.is_collapsed();
        if !(icon_clicked || is_collapsed) {
            return;
        }

        let surrounding = e.event.alt_key;
        let mut to_toggle: Vec<FoldingRegion> = Vec::new();
        if surrounding {
            let filter = |other_region: &FoldingRegion| {
                !other_region.contained_by(region.to_line_range()) && !region.contained_by(other_region.to_line_range())
            };
            let to_maybe_toggle = folding_model.get_regions_inside(None, Some(RegionFilter::Region(&filter)));
            to_toggle.extend(to_maybe_toggle.iter().filter(|r| r.is_collapsed()).cloned());
            // if any surrounding regions are folded, unfold those. Otherwise, fold all surrounding
            if to_toggle.is_empty() {
                to_toggle = to_maybe_toggle;
            }
        } else {
            let recursive = e.event.middle_button || e.event.shift_key;
            if recursive {
                for r in folding_model.get_regions_inside(Some(&region), None) {
                    if r.is_collapsed() == is_collapsed {
                        to_toggle.push(r);
                    }
                }
            }
            // when recursive, first only collapse all children. If all are already folded or there are no children, also fold parent.
            if is_collapsed || !recursive || to_toggle.is_empty() {
                to_toggle.push(region);
            }
        }
        folding_model.toggle_collapse_state(to_toggle);
        self.reveal(Position::new(line_number, 1));
    }

    /// `reveal(position)`.
    pub fn reveal(&self, position: Position) {
        if let Some(editor) = self.editor.upgrade() {
            editor.reveal_position_in_center_if_outside_viewport(position, Some(ScrollType::Smooth));
        }
    }
}

impl IDisposable for FoldingController {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IEditorContribution for FoldingController {
    fn has_view_state(&self) -> bool {
        true
    }

    fn save_view_state(&self) -> Dynamic {
        match self.save_folding_state() {
            Some(state) => state.to_dynamic(),
            None => Dynamic::Undefined,
        }
    }

    fn restore_view_state(&self, state: &Dynamic) {
        if let Some(state) = FoldingStateMemento::from_dynamic(state) {
            self.restore_folding_state(&state);
        }
    }
}

// #region Ações

/// `FoldingArguments.levels`: `args && args.levels || 1`.
fn levels_arg(args: &Dynamic) -> usize {
    match number_of(&arg(args, "levels")) {
        Some(levels) if levels != 0.0 && !levels.is_nan() => levels.ceil().max(0.0) as usize,
        _ => 1,
    }
}

/// `FoldingArguments.direction === 'up'`.
fn direction_is_up(args: &Dynamic) -> bool {
    matches!(arg(args, "direction"), Dynamic::String(direction) if direction.to_string_lossy() == "up")
}

/// `FoldingAction.getLineNumbers(args, editor)`: o `selectionLines` (de base 0) ou as linhas das seleções.
fn line_numbers_of(args: &Dynamic, editor: &CodeEditorWidget) -> Vec<u32> {
    match arg(args, "selectionLines") {
        Dynamic::Array(lines) => lines.to_vec().iter().filter_map(number_of).map(|line| line as u32 + 1).collect(), // to 0-bases line numbers
        _ => selected_lines_of(editor),
    }
}

/// `FoldingAction.getSelectedLines(editor)`.
fn selected_lines_of(editor: &CodeEditorWidget) -> Vec<u32> {
    editor.get_selections().map(|selections| selections.iter().map(|s| s.start_line_number as u32).collect()).unwrap_or_default()
}

/// `foldingArgumentsConstraint(args)`.
fn folding_arguments_constraint(args: &[Dynamic]) -> Dynamic {
    let valid = match args.first() {
        None | Some(Dynamic::Undefined) => true,
        Some(value @ Dynamic::Object(_)) => {
            let levels = arg(value, "levels");
            let direction = arg(value, "direction");
            let selection_lines = arg(value, "selectionLines");
            (levels.is_undefined() || matches!(levels, Dynamic::Number(_)))
                && (direction.is_undefined() || matches!(direction, Dynamic::String(_)))
                && (selection_lines.is_undefined()
                    || matches!(&selection_lines, Dynamic::Array(lines) if lines.to_vec().iter().all(|line| matches!(line, Dynamic::Number(_)))))
        }
        Some(_) => false,
    };
    Dynamic::Bool(valid)
}

/// O `metadata` de `editor.unfold` e `editor.fold`.
fn folding_metadata(description: &str, argument_name: &str, argument_description: String, levels_default: Option<f64>, direction_default: Option<&str>) -> CommandMetadata {
    let mut levels = vec![("type", Dynamic::from("number"))];
    if let Some(levels_default) = levels_default {
        levels.push(("default", Dynamic::Number(levels_default)));
    }
    let mut direction = vec![("type", Dynamic::from("string")), ("enum", Dynamic::array([Dynamic::from("up"), Dynamic::from("down")]))];
    if let Some(direction_default) = direction_default {
        direction.push(("default", Dynamic::from(direction_default)));
    }
    let schema = Dynamic::object([
        ("type", Dynamic::from("object")),
        (
            "properties",
            Dynamic::object([
                ("levels", Dynamic::object(levels)),
                ("direction", Dynamic::object(direction)),
                (
                    "selectionLines",
                    Dynamic::object([("type", Dynamic::from("array")), ("items", Dynamic::object([("type", Dynamic::from("number"))]))]),
                ),
            ]),
        ),
    ]);
    let mut argument = CommandArgument::new(argument_name);
    argument.description = Some(argument_description);
    argument.constraint = Some(TypeConstraint::Function(FunctionRef::native("foldingArgumentsConstraint", 1, folding_arguments_constraint)));
    argument.schema = Some(schema);
    let mut metadata = CommandMetadata::new(description);
    metadata.args = Some(vec![argument]);
    metadata
}

/// Qual ação é: o dado que diferencia as subclasses de `FoldingAction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldingActionKind {
    Unfold,
    UnfoldRecursively,
    Fold,
    ToggleFold,
    FoldRecursively,
    ToggleFoldRecursively,
    FoldAllBlockComments,
    FoldAllRegions,
    UnfoldAllRegions,
    FoldAllExcept,
    UnfoldAllExcept,
    FoldAll,
    UnfoldAll,
    /// `FoldLevelAction` de `1` a `7`.
    FoldLevel(usize),
    GotoParentFold,
    GotoPreviousFold,
    GotoNextFold,
    CreateFoldingRangeFromSelection,
    RemoveManualFoldingRanges,
    ToggleImportFold,
}

impl FoldingActionKind {
    /// As ações de `registerEditorAction(...)`, na ordem do fim do arquivo original.
    const REGISTERED: [FoldingActionKind; 19] = [
        FoldingActionKind::Unfold,
        FoldingActionKind::UnfoldRecursively,
        FoldingActionKind::Fold,
        FoldingActionKind::FoldRecursively,
        FoldingActionKind::ToggleFoldRecursively,
        FoldingActionKind::FoldAll,
        FoldingActionKind::UnfoldAll,
        FoldingActionKind::FoldAllBlockComments,
        FoldingActionKind::FoldAllRegions,
        FoldingActionKind::UnfoldAllRegions,
        FoldingActionKind::FoldAllExcept,
        FoldingActionKind::UnfoldAllExcept,
        FoldingActionKind::ToggleFold,
        FoldingActionKind::GotoParentFold,
        FoldingActionKind::GotoPreviousFold,
        FoldingActionKind::GotoNextFold,
        FoldingActionKind::CreateFoldingRangeFromSelection,
        FoldingActionKind::RemoveManualFoldingRanges,
        FoldingActionKind::ToggleImportFold,
    ];

    /// `id`, a chave de tradução e o rótulo em inglês.
    fn id_and_label(self) -> (String, &'static str, String) {
        let (id, key, label) = match self {
            Self::Unfold => ("editor.unfold", "unfoldAction.label", "Unfold"),
            Self::UnfoldRecursively => ("editor.unfoldRecursively", "unFoldRecursivelyAction.label", "Unfold Recursively"),
            Self::Fold => ("editor.fold", "foldAction.label", "Fold"),
            Self::ToggleFold => ("editor.toggleFold", "toggleFoldAction.label", "Toggle Fold"),
            Self::FoldRecursively => ("editor.foldRecursively", "foldRecursivelyAction.label", "Fold Recursively"),
            Self::ToggleFoldRecursively => ("editor.toggleFoldRecursively", "toggleFoldRecursivelyAction.label", "Toggle Fold Recursively"),
            Self::FoldAllBlockComments => ("editor.foldAllBlockComments", "foldAllBlockComments.label", "Fold All Block Comments"),
            Self::FoldAllRegions => ("editor.foldAllMarkerRegions", "foldAllMarkerRegions.label", "Fold All Regions"),
            Self::UnfoldAllRegions => ("editor.unfoldAllMarkerRegions", "unfoldAllMarkerRegions.label", "Unfold All Regions"),
            Self::FoldAllExcept => ("editor.foldAllExcept", "foldAllExcept.label", "Fold All Except Selected"),
            Self::UnfoldAllExcept => ("editor.unfoldAllExcept", "unfoldAllExcept.label", "Unfold All Except Selected"),
            Self::FoldAll => ("editor.foldAll", "foldAllAction.label", "Fold All"),
            Self::UnfoldAll => ("editor.unfoldAll", "unfoldAllAction.label", "Unfold All"),
            Self::FoldLevel(level) => return (format!("editor.foldLevel{level}"), "foldLevelAction.label", format!("Fold Level {level}")),
            Self::GotoParentFold => ("editor.gotoParentFold", "gotoParentFold.label", "Go to Parent Fold"),
            Self::GotoPreviousFold => ("editor.gotoPreviousFold", "gotoPreviousFold.label", "Go to Previous Folding Range"),
            Self::GotoNextFold => ("editor.gotoNextFold", "gotoNextFold.label", "Go to Next Folding Range"),
            Self::CreateFoldingRangeFromSelection => {
                ("editor.createFoldingRangeFromSelection", "createManualFoldRange.label", "Create Folding Range from Selection")
            }
            Self::RemoveManualFoldingRanges => ("editor.removeManualFoldingRanges", "removeManualFoldingRanges.label", "Remove Manual Folding Ranges"),
            Self::ToggleImportFold => ("editor.toggleImportFold", "toggleImportFold.label", "Toggle Import Fold"),
        };
        (id.to_owned(), key, label.to_owned())
    }

    /// `kbOpts` sem o `kbExpr` e o peso, que são os mesmos em todas as ações.
    fn keybindings(self) -> Keybindings {
        let ctrl_k = KeyMod::CTRL_CMD | KeyCode::KeyK.bits();
        let chord = |second: u32| Keybindings { primary: Some(key_chord(ctrl_k, KeyMod::CTRL_CMD | second)), ..Keybindings::default() };
        match self {
            Self::Unfold => Keybindings {
                primary: Some(KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::BracketRight.bits()),
                mac: Some(PlatformKeybindings { primary: Some(KeyMod::CTRL_CMD | KeyMod::ALT | KeyCode::BracketRight.bits()), secondary: None }),
                ..Keybindings::default()
            },
            Self::Fold => Keybindings {
                primary: Some(KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::BracketLeft.bits()),
                mac: Some(PlatformKeybindings { primary: Some(KeyMod::CTRL_CMD | KeyMod::ALT | KeyCode::BracketLeft.bits()), secondary: None }),
                ..Keybindings::default()
            },
            Self::UnfoldRecursively => chord(KeyCode::BracketRight.bits()),
            Self::ToggleFold => chord(KeyCode::KeyL.bits()),
            Self::FoldRecursively => chord(KeyCode::BracketLeft.bits()),
            Self::ToggleFoldRecursively => chord(KeyMod::SHIFT | KeyCode::KeyL.bits()),
            Self::FoldAllBlockComments => chord(KeyCode::Slash.bits()),
            Self::FoldAllRegions => chord(KeyCode::Digit8.bits()),
            Self::UnfoldAllRegions => chord(KeyCode::Digit9.bits()),
            Self::FoldAllExcept => chord(KeyCode::Minus.bits()),
            Self::UnfoldAllExcept => chord(KeyCode::Equal.bits()),
            Self::FoldAll => chord(KeyCode::Digit0.bits()),
            Self::UnfoldAll => chord(KeyCode::KeyJ.bits()),
            Self::FoldLevel(level) => chord(KeyCode(KeyCode::Digit0.0 + level as i32).bits()),
            Self::CreateFoldingRangeFromSelection => chord(KeyCode::Comma.bits()),
            Self::RemoveManualFoldingRanges => chord(KeyCode::Period.bits()),
            Self::GotoParentFold | Self::GotoPreviousFold | Self::GotoNextFold | Self::ToggleImportFold => Keybindings::default(),
        }
    }

    fn metadata(self) -> Option<CommandMetadata> {
        match self {
            Self::Unfold => Some(folding_metadata(
                "Unfold the content in the editor",
                "Unfold editor argument",
                "Property-value pairs that can be passed through this argument:\n\t\t\t\t\t\t\t* 'levels': Number of levels to unfold. If not set, defaults to 1.\n\t\t\t\t\t\t\t* 'direction': If 'up', unfold given number of levels up otherwise unfolds down.\n\t\t\t\t\t\t\t* 'selectionLines': Array of the start lines (0-based) of the editor selections to apply the unfold action to. If not set, the active selection(s) will be used.\n\t\t\t\t\t\t\t".to_owned(),
                Some(1.0),
                Some("down"),
            )),
            Self::Fold => Some(folding_metadata(
                "Fold the content in the editor",
                "Fold editor argument",
                "Property-value pairs that can be passed through this argument:\n\t\t\t\t\t\t\t\t* 'levels': Number of levels to fold.\n\t\t\t\t\t\t\t\t* 'direction': If 'up', folds given number of levels up otherwise folds down.\n\t\t\t\t\t\t\t\t* 'selectionLines': Array of the start lines (0-based) of the editor selections to apply the fold action to. If not set, the active selection(s) will be used.\n\t\t\t\t\t\t\t\tIf no levels or direction is set, folds the region at the locations or if already collapsed, the first uncollapsed parent instead.\n\t\t\t\t\t\t\t".to_owned(),
                None,
                None,
            )),
            _ => None,
        }
    }

    /// O `invoke(foldingController, foldingModel, editor, args, languageConfigurationService)` de cada subclasse.
    fn invoke(self, controller: &Rc<FoldingController>, folding_model: &Rc<FoldingModel>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) {
        let fm = &**folding_model;
        match self {
            Self::Unfold => {
                let levels = levels_arg(args);
                let line_numbers = line_numbers_of(args, editor);
                if direction_is_up(args) {
                    set_collapse_state_levels_up(fm, false, levels, &line_numbers);
                } else {
                    set_collapse_state_levels_down(fm, false, levels, Some(&line_numbers));
                }
            }
            Self::UnfoldRecursively => set_collapse_state_levels_down(fm, false, usize::MAX, Some(&selected_lines_of(editor))),
            Self::Fold => {
                let line_numbers = line_numbers_of(args, editor);
                let has_levels = matches!(arg(args, "levels"), Dynamic::Number(_));
                let has_direction = matches!(arg(args, "direction"), Dynamic::String(_));
                if !has_levels && !has_direction {
                    // fold the region at the location or if already collapsed, the first uncollapsed parent instead.
                    set_collapse_state_up(fm, true, &line_numbers);
                } else if direction_is_up(args) {
                    set_collapse_state_levels_up(fm, true, levels_arg(args), &line_numbers);
                } else {
                    set_collapse_state_levels_down(fm, true, levels_arg(args), Some(&line_numbers));
                }
            }
            Self::ToggleFold => toggle_collapse_state(fm, 1, &selected_lines_of(editor)),
            Self::FoldRecursively => set_collapse_state_levels_down(fm, true, usize::MAX, Some(&selected_lines_of(editor))),
            Self::ToggleFoldRecursively => toggle_collapse_state(fm, usize::MAX, &selected_lines_of(editor)),
            Self::FoldAllBlockComments => {
                if fm.regions().has_types() {
                    set_collapse_state_for_type(fm, &FoldingRangeKind::comment().value, true);
                } else {
                    let Some(language_id) = editor.get_model().map(|model| model.get_language_id()) else {
                        return;
                    };
                    let configuration = editor.language_configuration_service().get_language_configuration(&language_id);
                    let token = configuration.comments.as_ref().and_then(|comments| comments.block_comment_start_token.clone()).filter(|token| !token.is_empty());
                    if let Some(token) = token {
                        let units: Vec<u16> = token.encode_utf16().collect();
                        let source = format!("^\\s*{}", escape_reg_exp_characters(&units).to_string_lossy());
                        let reg_exp = Rc::new(JsRegExp::from_str(&source, "").expect("o token de comentário escapado forma uma expressão regular válida"));
                        set_collapse_state_for_matching_lines(fm, &reg_exp, true);
                    }
                }
            }
            Self::FoldAllRegions => Self::set_regions_collapsed(fm, editor, true),
            Self::UnfoldAllRegions => Self::set_regions_collapsed(fm, editor, false),
            Self::FoldAllExcept => set_collapse_state_for_rest(fm, true, &selected_lines_of(editor)),
            Self::UnfoldAllExcept => set_collapse_state_for_rest(fm, false, &selected_lines_of(editor)),
            Self::FoldAll => set_collapse_state_levels_down(fm, true, usize::MAX, None),
            Self::UnfoldAll => set_collapse_state_levels_down(fm, false, usize::MAX, None),
            Self::FoldLevel(level) => set_collapse_state_at_level(fm, level, true, &selected_lines_of(editor)),
            Self::GotoParentFold => Self::goto_fold_line(editor, |line| get_parent_fold_line(line, fm)),
            Self::GotoPreviousFold => Self::goto_fold_line(editor, |line| get_previous_fold_line(line, fm)),
            Self::GotoNextFold => Self::goto_fold_line(editor, |line| get_next_fold_line(line, fm)),
            Self::CreateFoldingRangeFromSelection => Self::create_folding_range_from_selection(fm, editor),
            Self::RemoveManualFoldingRanges => {
                if let Some(selections) = editor.get_selections() {
                    let ranges: Vec<Range> = selections
                        .iter()
                        .map(|s| Range::new(s.start_line_number, s.start_column, s.end_line_number, s.end_column))
                        .collect();
                    fm.remove_manual_ranges(&ranges);
                    controller.trigger_folding_model_changed();
                }
            }
            Self::ToggleImportFold => {
                let mut regions_to_toggle: Vec<FoldingRegion> = Vec::new();
                let regions = fm.regions();
                for i in (0..regions.length()).rev() {
                    if regions.get_type(i) == Some(FoldingRangeKind::imports().value.as_str()) {
                        regions_to_toggle.push(regions.to_region(i));
                    }
                }
                fm.toggle_collapse_state(regions_to_toggle);
                controller.trigger_folding_model_changed();
            }
        }
    }

    /// O corpo comum de `FoldAllRegionsAction` e `UnfoldAllRegionsAction`.
    fn set_regions_collapsed(folding_model: &FoldingModel, editor: &Rc<CodeEditorWidget>, do_collapse: bool) {
        if folding_model.regions().has_types() {
            set_collapse_state_for_type(folding_model, &FoldingRangeKind::region().value, do_collapse);
        } else {
            let Some(language_id) = editor.get_model().map(|model| model.get_language_id()) else {
                return;
            };
            let configuration = editor.language_configuration_service().get_language_configuration(&language_id);
            if let Some(markers) = &configuration.folding_rules.markers {
                set_collapse_state_for_matching_lines(folding_model, &markers.start, do_collapse);
            }
        }
    }

    /// O corpo comum das três ações `editor.gotoParentFold`, `gotoPreviousFold` e `gotoNextFold`.
    fn goto_fold_line(editor: &Rc<CodeEditorWidget>, target_line: impl Fn(u32) -> Option<u32>) {
        let selected_lines = selected_lines_of(editor);
        if let Some(&first) = selected_lines.first() {
            if let Some(start_line_number) = target_line(first) {
                let line = start_line_number as i32;
                editor.set_selection(SelectionOrRange::Range(Range::new(line, 1, line, 1)), None);
            }
        }
    }

    fn create_folding_range_from_selection(folding_model: &FoldingModel, editor: &Rc<CodeEditorWidget>) {
        let mut collapse_ranges: Vec<FoldRange> = Vec::new();
        let Some(selections) = editor.get_selections() else {
            return;
        };
        for selection in &selections {
            let mut end_line_number = selection.end_line_number;
            if selection.end_column == 1 {
                end_line_number -= 1;
            }
            if end_line_number > selection.start_line_number {
                collapse_ranges.push(FoldRange {
                    start_line_number: selection.start_line_number as u32,
                    end_line_number: end_line_number as u32,
                    r#type: None,
                    is_collapsed: true,
                    source: FoldSource::UserDefined,
                });
                let start = selection.start_line_number;
                editor.set_selection(SelectionOrRange::Range(Range::new(start, 1, start, 1)), None);
            }
        }
        if !collapse_ranges.is_empty() {
            collapse_ranges.sort_by_key(|range| range.start_line_number);
            let line_count = editor.get_model().map(|model| model.get_line_count() as u32);
            let regions = folding_model.regions();
            let new_ranges = FoldingRegions::sanitize_and_merge((&*regions).into(), (&collapse_ranges).into(), line_count, None);
            folding_model.update_post(FoldingRegions::from_fold_ranges(&new_ranges));
        }
    }
}

/// `FoldingAction` e as suas subclasses.
pub struct FoldingAction {
    action: EditorActionBase,
    kind: FoldingActionKind,
}

impl FoldingAction {
    pub fn new(kind: FoldingActionKind) -> Self {
        let (id, label_key, label) = kind.id_and_label();
        Self {
            action: EditorActionBase::new(ActionOptions {
                command: CommandOptions {
                    precondition: Some(CONTEXT_FOLDING_ENABLED.with(|key| key.expr())),
                    kb_opts: vec![CommandKeybindingsOptions::new(KeybindingWeight::EDITOR_CONTRIB, Some(editor_text_focus()), kind.keybindings())],
                    metadata: kind.metadata(),
                    ..CommandOptions::new(&id)
                },
                context_menu_opts: Vec::new(),
                label: ActionLabel::Localized { label: localize2(label_key, &label), alias: None },
            }),
            kind,
        }
    }

    pub fn kind(&self) -> FoldingActionKind {
        self.kind
    }

    /// `runEditorCommand` com o controlador já resolvido (o `FoldingController.get(editor)` do original).
    pub fn run_with_controller(&self, controller: &Rc<FoldingController>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        let Some(folding_model_promise) = controller.get_folding_model() else {
            return Promise::resolved(());
        };
        let (kind, controller, editor, args) = (self.kind, Rc::clone(controller), Rc::clone(editor), args.clone());
        folding_model_promise.then_result(move |result| {
            if let Some(folding_model) = result? {
                kind.invoke(&controller, &folding_model, &editor, &args);
                if let Some(selection) = editor.get_selection() {
                    controller.reveal(selection.get_start_position());
                }
            }
            Ok(())
        })
    }
}

impl EditorAction for FoldingAction {
    fn action_base(&self) -> &EditorActionBase {
        &self.action
    }

    fn run(&self, _accessor: Option<&ServicesAccessor>, editor: &Rc<CodeEditorWidget>, args: &Dynamic) -> Promise<()> {
        match FoldingController::get(editor) {
            Some(controller) => self.run_with_controller(&controller, editor, args),
            None => Promise::resolved(()),
        }
    }
}

// #endregion

// #region Registro

/// As chamadas `registerEditorContribution(...)`, `registerEditorAction(...)` e `registerInstantiatedEditorAction(...)`
/// do fim do arquivo. `services` entrega os [`FoldingControllerServices`] de cada editor (o `createInstance` do original).
/// É idempotente: o registro acontece uma vez por thread.
pub fn register_folding(services: Rc<dyn Fn(&Rc<CodeEditorWidget>) -> FoldingControllerServices>) {
    if REGISTERED.replace(true) {
        return;
    }
    // eager because it uses `saveViewState`/`restoreViewState`
    register_editor_contribution(
        FoldingController::ID,
        Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> { FoldingController::new(editor, services(editor)) }),
        EditorContributionInstantiation::Eager,
    );
    for kind in FoldingActionKind::REGISTERED {
        register_editor_action(move || FoldingAction::new(kind));
    }
    for level in 1..=7 {
        register_instantiated_editor_action(Rc::new(FoldingAction::new(FoldingActionKind::FoldLevel(level))));
    }
}

/// O limite fixo do comando `_executeFoldingRangeProvider`, que não repassa as atualizações.
struct FixedLimitReporter(usize);

impl FoldingLimitReporter for FixedLimitReporter {
    fn limit(&self) -> usize {
        self.0
    }

    fn update(&self, _computed: usize, _limited: Option<usize>) {}
}

/// O corpo do comando `_executeFoldingRangeProvider(resource)`: as regiões de dobra do modelo, pela estratégia
/// configurada, em `FoldingRange`s (ver "Divergências"). Com `editor.folding` desligado devolve a lista vazia.
pub fn execute_folding_range_provider(
    model: &Rc<TextModel>,
    language_features_service: &LanguageFeaturesService,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    folding_enabled: bool,
    strategy: &str,
    maximum_regions: usize,
) -> Promise<Vec<FoldingRange>> {
    if !folding_enabled {
        return Promise::resolved(Vec::new());
    }
    let folding_limit_reporter: Rc<dyn FoldingLimitReporter> = Rc::new(FixedLimitReporter(maximum_regions));
    let indent_range_provider: Rc<dyn RangeProvider> =
        Rc::new(IndentRangeProvider::new(Rc::clone(model), language_configuration_service, folding_limit_reporter.clone()));
    let mut range_provider = Rc::clone(&indent_range_provider);
    if strategy != "indentation" {
        let providers = FoldingController::get_folding_range_providers(language_features_service, model);
        if !providers.is_empty() {
            let no_op: Rc<dyn Fn()> = Rc::new(|| {});
            range_provider = Rc::new(SyntaxRangeProvider::new(Rc::clone(model), providers, no_op, folding_limit_reporter, Some(indent_range_provider)));
        }
    }
    let disposing = Rc::clone(&range_provider);
    range_provider.compute(&CancellationToken::None).then_result(move |ranges| {
        // finally { rangeProvider.dispose(); }
        disposing.dispose();
        let mut result: Vec<FoldingRange> = Vec::new();
        if let Some(ranges) = ranges? {
            for i in 0..ranges.length() {
                result.push(FoldingRange {
                    start: ranges.get_start_line_number(i) as i32,
                    end: ranges.get_end_line_number(i) as i32,
                    kind: ranges.get_type(i).map(FoldingRangeKind::new),
                });
            }
        }
        Ok(result)
    })
}

// #endregion
