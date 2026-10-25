//! Porte de `src/vs/workbench/contrib/search/browser/anythingQuickAccess.ts`: o acesso rápido "ir para arquivo" que une o
//! histórico de editores, a busca de arquivos (com cache de consulta, caminho absoluto e relativo), os símbolos do
//! workspace, os símbolos do editor (`@`) e os links (`IAnythingQuickAccessProviderRunOptions` incluso, de
//! `search/common/search.ts`).
//!
//! Notas de tradução:
//! - **Serviços.** Os 25 serviços injetados do original são o [`AnythingQuickAccessServices`]. Os que o porte já tem entram
//!   como o trait de cada um. Três colaboradores ainda não têm porte e entram como contratos mínimos deste módulo, com os
//!   mesmos nomes de método do original: [`ISymbolsQuickAccess`] (`SymbolsQuickAccessProvider.getSymbolPicks`),
//!   [`IGotoSymbolQuickAccess`] (`GotoSymbolQuickAccessProvider.getSymbolPicks`, `addDecorations`, `clearDecorations`) e
//!   [`IPickerEditorState`] (`PickerEditorState`, de `browser/quickaccess.ts`). `getHelpPicks` depende do registro de
//!   provedores de acesso rápido (`IQuickAccessRegistry`) e entra como [`IHelpPicks`]. `getOutOfWorkspaceEditorResources`
//!   (`search/common/search.ts`) é o `out_of_workspace_editor_resources` do pacote de serviços.
//! - **Fora do porte (chat).** `IQuickChatService` e `IChatWidgetService` (o item "Open Quick Chat" do centro de comandos e o
//!   `attach` com Shift que adiciona o arquivo ao contexto do chat) são do chat, fora de escopo; o `attach` cai no caso de
//!   aceitar, que é o comportamento do original sem um widget focado.
//! - **Itens.** `IAnythingQuickPickItem extends IPickerQuickAccessItem, IQuickPickItemWithResource` é o [`IQuickPickItem`] com os
//!   métodos em [`PickerItemHandlers`] e os campos extras (`resource`, `editor`, `range`) em [`AnythingItemData`] (no `data`).
//!   `buttons` e `iconClasses` (preguiçosos no original) são calculados na criação do item.
//! - **Estado do seletor.** `IAnythingPickState` é o [`PickState`] com `RefCell`s; o seletor do estado é guardado por
//!   identidade.
//! - **`instanceof Promise`.** O `Array.isArray(picks)` do teste é a variante [`ProvidedPicks::Picks`].
//! - **`ThrottledDelayer`.** O `fileQueryDelayer` usa o `ThrottledDelayer` do porte.
//! - **Registro de grupos.** `activeGroup` só é lido quando o provedor aceita links (a regra de `_supportsLinks`).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::arrays::top;
use je_base::async_::{async_fn, ThrottledDelayer};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::filters::Match;
use je_base::fuzzy_scorer::{compare_items_by_fuzzy_score, prepare_query, score_item_fuzzy, FuzzyScorerCache, PreparedQuery};
use je_base::js_string::JsString;
use je_base::labels::untildify;
use je_base::lifecycle::{to_disposable, Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::map::ResourceMap;
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::ResolveFileOptions;
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::log::LogService;
use je_platform::notification::notification::{INotificationService, NotificationMessage};
use je_platform::opener::editor_types::{EditorOptions, TextEditorSelection};
use je_platform::opener::opener::{IOpenerService, OpenOptions, OpenResource};
use je_platform::path::path_service::IPathService;
use je_platform::quickinput::picker_quick_access::{
    FastAndSlowPicks, IPickerQuickAccessProviderOptions, IPickerQuickAccessSource, NoResultsPick, PickerItemHandlers, PickerQuickAccessProvider, Picks,
    PicksWithActive, PromisedPicks, ProvidedPicks, TriggerAction, TriggerResult,
};
use je_platform::quickinput::quick_access::{AsQuickAccessRunOptions, DefaultQuickAccessFilterValue, IQuickAccessProvider, IQuickAccessProviderRunOptions};
use je_platform::quickinput::quick_input::{
    IKeyMods, IQuickInputButton, IQuickPick, IQuickPickItem, IQuickPickItemHighlights, IQuickPickItemWithResource, IQuickPickSeparator,
    QuickInputButtonRef, QuickInputHideReason, QuickPickInput, QuickPickItemScorerAccessor,
};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;

use crate::common::editor::{EditorInputOrUntyped, EditorResourceAccessor, TextEditorOptions, UntypedEditorInput};
use crate::contrib::search::common::cache_state::FileQueryCacheState;
use crate::contrib::search::search::extract_range_from_filter;
use crate::services::editor::editor_groups_service::{GroupRef, IEditorGroupsService, PreferredGroup};
use crate::services::editor::editor_service::{ActiveTextEditorControl, IEditorService};
use crate::services::editor::custom_editor_label_service::ICustomEditorLabelService;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::history::history::IHistoryService;
use crate::services::search::query_builder::{IFileQueryBuilderOptions, ICommonQueryBuilderOptions, QueryBuilder, WorkspaceFolderOrUri};
use crate::services::search::search::{ISearchComplete, ISearchService};
use crate::services::working_copy::working_copy_service::IWorkingCopyService;

/// `IAnythingQuickAccessFileSearchDelegate`.
#[derive(Clone)]
pub struct IAnythingQuickAccessFileSearchDelegate {
    pub search: Rc<dyn Fn(&str, CancellationToken) -> Promise<ISearchComplete>>,
    pub get_item_description: Option<Rc<dyn Fn(&URI) -> String>>,
}

/// `IAnythingQuickAccessProviderRunOptions` (com o `AnythingQuickAccessProviderRunOptions` que ele estende).
#[derive(Clone, Default)]
pub struct IAnythingQuickAccessProviderRunOptions {
    pub base: IQuickAccessProviderRunOptions,
    pub include_help: Option<bool>,
    pub filter: Option<Rc<dyn Fn(&QuickPickInput) -> bool>>,
    /// Useful for adding items to the top of the list that might contain actions.
    pub addition_picks: Option<Vec<QuickPickInput>>,
    pub files_only: Option<bool>,
    pub include_open_to_side_button: Option<bool>,
    pub file_search_delegate: Option<IAnythingQuickAccessFileSearchDelegate>,
}

impl AsQuickAccessRunOptions for IAnythingQuickAccessProviderRunOptions {
    fn base(&self) -> &IQuickAccessProviderRunOptions {
        &self.base
    }
}

/// Os campos de `IAnythingQuickPickItem` que o item de dados não tem.
#[derive(Clone, Default)]
pub struct AnythingItemData {
    pub resource: Option<URI>,
    pub editor: Option<EditorInputOrUntyped>,
    /// `range: { decoration, selection }` de `IEditorSymbolAnythingQuickPickItem`.
    pub range: Option<EditorSymbolRange>,
}

/// `{ decoration: IRange; selection: IRange }`.
#[derive(Clone, Copy, Debug)]
pub struct EditorSymbolRange {
    pub decoration: Range,
    pub selection: Range,
}

/// O `resource` de um item (o `IQuickPickItemWithResource.resource`).
pub fn resource_of(item: &IQuickPickItem) -> Option<URI> {
    PickerItemHandlers::data_of::<AnythingItemData>(item).and_then(|data| data.resource.clone())
}

/// `isEditorSymbolQuickPickItem`.
pub fn is_editor_symbol_quick_pick_item(pick: Option<&IQuickPickItem>) -> bool {
    pick.and_then(PickerItemHandlers::data_of::<AnythingItemData>).is_some_and(|data| data.range.is_some() && data.resource.is_some())
}

/// Um item que o `getSymbolPicks` do editor devolve: o item e o `range` dele, ou um separador.
#[derive(Clone)]
pub enum EditorSymbolPick {
    Separator(IQuickPickSeparator),
    Item { item: IQuickPickItem, range: Option<EditorSymbolRange> },
}

/// `SymbolsQuickAccessProvider.getSymbolPicks(filter, { skipLocal, skipSorting, delay }, token)`.
pub trait ISymbolsQuickAccess {
    fn get_symbol_picks(&self, filter: &str, skip_local: bool, skip_sorting: bool, delay: u64, token: CancellationToken) -> Promise<Vec<IQuickPickItem>>;
}

/// `GotoSymbolQuickAccessProvider` (o que o provedor lê dele). `PREFIX` é `@`.
pub trait IGotoSymbolQuickAccess {
    fn get_symbol_picks(&self, model: &Rc<TextModel>, filter: &str, extra_container_label: &str, disposables: &Rc<DisposableStore>, token: CancellationToken) -> Promise<Vec<EditorSymbolPick>>;
    fn add_decorations(&self, editor: &ActiveTextEditorControl, range: &Range);
    fn clear_decorations(&self, editor: &ActiveTextEditorControl);
}

/// `GotoSymbolQuickAccessProvider.PREFIX`.
pub const GOTO_SYMBOL_PREFIX: &str = "@";

/// `PickerEditorState` (`browser/quickaccess.ts`).
pub trait IPickerEditorState {
    fn set(&self);
    fn restore(&self) -> Promise<()>;
    fn reset(&self);
    fn open_transient_editor(&self, editor: UntypedEditorInput) -> Promise<()>;
}

/// `getHelpPicks` (o centro de comandos), que lê o registro de provedores de acesso rápido.
pub trait IHelpPicks {
    fn get_help_picks(&self, query: &PreparedQuery, run_options: &IAnythingQuickAccessProviderRunOptions) -> Vec<IQuickPickItem>;
}

/// Os serviços injetados (ver as notas do módulo).
#[derive(Clone)]
pub struct AnythingQuickAccessServices {
    pub search_service: Rc<dyn ISearchService>,
    pub context_service: Rc<dyn IWorkspaceContextService>,
    pub path_service: Rc<dyn IPathService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub file_service: Rc<dyn IFileService>,
    pub label_service: Rc<dyn ILabelService>,
    pub model_service: Rc<dyn je_editor::services_model::IModelService>,
    pub language_service: Rc<dyn je_editor::language_service::ILanguageService>,
    pub working_copy_service: Rc<dyn IWorkingCopyService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub editor_service: Rc<dyn IEditorService>,
    pub history_service: Rc<dyn IHistoryService>,
    pub files_configuration_service: Rc<dyn crate::services::files_configuration::files_configuration_service::IFilesConfigurationService>,
    pub text_model_service: Rc<dyn crate::services::textmodel_resolver::resolver_service::ITextModelService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub log_service: Rc<dyn LogService>,
    pub custom_editor_label_service: Rc<dyn ICustomEditorLabelService>,
    pub opener_service: Rc<dyn IOpenerService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub editor_groups_service: Rc<dyn IEditorGroupsService>,
    pub workspace_symbols_quick_access: Rc<dyn ISymbolsQuickAccess>,
    pub editor_symbols_quick_access: Rc<dyn IGotoSymbolQuickAccess>,
    pub editor_view_state: Rc<dyn IPickerEditorState>,
    pub help_picks: Rc<dyn IHelpPicks>,
    /// `getOutOfWorkspaceEditorResources` (`search/common/search.ts`).
    pub out_of_workspace_editor_resources: Rc<dyn Fn() -> Vec<URI>>,
}

/// `IAnythingPickState`.
struct PickState {
    picker: RefCell<Option<Weak<dyn IQuickPick>>>,
    scorer_cache: RefCell<FuzzyScorerCache>,
    file_query_cache: RefCell<Option<Rc<FileQueryCacheState>>>,
    last_original_filter: RefCell<Option<String>>,
    last_filter: RefCell<Option<String>>,
    last_range: Cell<Option<Range>>,
    last_global_picks: RefCell<Option<PicksWithActive>>,
    is_quick_navigating: Cell<Option<bool>>,
}

/// `AnythingQuickAccessProvider.NO_RESULTS_PICK`.
fn no_results_pick() -> IQuickPickItem {
    let mut item = IQuickPickItem::default();
    item.base.label = localize("noAnythingResults", "No matching results", &[]).into();
    item
}

const MAX_RESULTS: usize = 512;
const TYPING_SEARCH_DELAY: u64 = 200; // this delay accommodates for the user typing a word and then stops typing to start searching
const SYMBOL_PICKS_MERGE_DELAY: u64 = 200; // allow some time to merge fast and slow picks to reduce flickering

/// `this.configuration`.
struct Configuration {
    open_editor_pinned: bool,
    open_side_by_side_direction: Option<String>,
    include_symbols: bool,
    include_history: bool,
    history_filter_sort_order: Option<String>,
    preserve_input: bool,
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn text_of(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `AnythingQuickAccessProvider`.
pub struct AnythingQuickAccessProvider {
    this: Weak<AnythingQuickAccessProvider>,
    picker_base: PickerQuickAccessProvider,
    services: AnythingQuickAccessServices,
    pick_state: PickState,
    link_source_group: Cell<Option<i64>>,
    file_query_builder: Rc<QueryBuilder>,
    file_query_delayer: ThrottledDelayer<Vec<URI>>,
    label_only_editor_history_pick_accessor: QuickPickItemScorerAccessor,
    /// O `_getPicks` sobrescrito por uma subclasse (`protected override _getPicks`), quando houver.
    picks_override: RefCell<Option<Rc<dyn IPickerQuickAccessSource<IAnythingQuickAccessProviderRunOptions>>>>,
}

impl AnythingQuickAccessProvider {
    pub const PREFIX: &'static str = "";

    pub fn new(services: AnythingQuickAccessServices, file_query_builder: Rc<QueryBuilder>) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            picker_base: PickerQuickAccessProvider::new(
                Self::PREFIX,
                Some(IPickerQuickAccessProviderOptions {
                    can_accept_in_background: true,
                    no_results_pick: Some(NoResultsPick::Item(no_results_pick())),
                    should_skip_trim_pick_filter: false,
                }),
            ),
            services,
            pick_state: PickState {
                picker: RefCell::new(None),
                scorer_cache: RefCell::new(FuzzyScorerCache::new()),
                file_query_cache: RefCell::new(None),
                last_original_filter: RefCell::new(None),
                last_filter: RefCell::new(None),
                last_range: Cell::new(None),
                last_global_picks: RefCell::new(None),
                is_quick_navigating: Cell::new(None),
            },
            link_source_group: Cell::new(None),
            file_query_builder,
            file_query_delayer: ThrottledDelayer::new(TYPING_SEARCH_DELAY),
            label_only_editor_history_pick_accessor: QuickPickItemScorerAccessor::new(true, false),
            picks_override: RefCell::new(None),
        })
    }

    fn self_rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o provedor está vivo enquanto é usado")
    }

    pub fn default_filter_value(&self) -> Option<DefaultQuickAccessFilterValue> {
        if self.configuration().preserve_input {
            return Some(DefaultQuickAccessFilterValue::Last);
        }
        None
    }

    fn configuration(&self) -> Configuration {
        let editor_config = self.services.configuration_service.get_value(Some("workbench.editor"), None);
        let search_config = self.services.configuration_service.get_value(Some("search"), None);
        let quick_access_config = self.services.configuration_service.get_value(Some("workbench.quickOpen"), None);
        let quick_open = if search_config.is_truthy() { search_config.get_str("quickOpen") } else { Dynamic::Undefined };
        let history = if quick_open.is_truthy() { quick_open.get_str("history") } else { Dynamic::Undefined };
        let include_history = if quick_open.is_truthy() && !matches!(quick_open.get_str("includeHistory"), Dynamic::Undefined | Dynamic::Null) {
            quick_open.get_str("includeHistory").is_truthy()
        } else {
            true
        };
        let get = |config: &Dynamic, key: &str| if config.is_truthy() { config.get_str(key) } else { Dynamic::Undefined };
        Configuration {
            open_editor_pinned: !get(&editor_config, "enablePreviewFromQuickOpen").is_truthy() || !get(&editor_config, "enablePreview").is_truthy(),
            open_side_by_side_direction: text_of(&get(&editor_config, "openSideBySideDirection")),
            include_symbols: get(&quick_open, "includeSymbols").is_truthy(),
            include_history,
            history_filter_sort_order: text_of(&get(&history, "filterSortOrder")),
            preserve_input: get(&quick_access_config, "preserveInput").is_truthy(),
        }
    }

    /// `createFileQueryCache()`.
    fn create_file_query_cache(&self) -> Rc<FileQueryCacheState> {
        let (query_this, load_this, dispose_this) = (self.self_rc(), self.self_rc(), self.self_rc());
        FileQueryCacheState::new(
            Rc::new(move |cache_key| {
                query_this.file_query_builder.file(
                    &query_this.services.context_service.get_workspace().folders().into_iter().map(WorkspaceFolderOrUri::Folder).collect::<Vec<_>>(),
                    &query_this.get_file_query_options(None, Some(cache_key.to_string()), None),
                )
            }),
            Rc::new(move |query| load_this.services.search_service.file_search(query, None).then(|_| ())),
            Rc::new(move |cache_key| dispose_this.services.search_service.clear_cache(cache_key)),
            self.pick_state.file_query_cache.borrow().clone(),
        )
        .load()
    }

    /// `pickState.set(picker, useDefaultWorkspace)`.
    fn set_pick_state(&self, picker: &Rc<dyn IQuickPick>, use_default_workspace: bool) {
        // Picker for this run
        *self.pick_state.picker.borrow_mut() = Some(Rc::downgrade(picker));

        // Caches
        let is_quick_navigating = picker.quick_navigate().is_some();
        if !is_quick_navigating {
            if use_default_workspace {
                let cache = self.create_file_query_cache();
                *self.pick_state.file_query_cache.borrow_mut() = Some(cache);
            } else {
                let previous = self.pick_state.file_query_cache.borrow_mut().take();
                if let Some(previous) = previous {
                    previous.dispose();
                }
            }
            *self.pick_state.scorer_cache.borrow_mut() = FuzzyScorerCache::new();
        }

        // Other
        self.pick_state.is_quick_navigating.set(Some(is_quick_navigating));
        *self.pick_state.last_original_filter.borrow_mut() = None;
        *self.pick_state.last_filter.borrow_mut() = None;
        self.pick_state.last_range.set(None);
        *self.pick_state.last_global_picks.borrow_mut() = None;
        self.services.editor_view_state.reset();
    }

    fn current_picker(&self) -> Option<Rc<dyn IQuickPick>> {
        self.pick_state.picker.borrow().as_ref().and_then(Weak::upgrade)
    }

    /// `provide(picker, token, runOptions)`.
    pub fn provide(&self, picker: Rc<dyn IQuickPick>, token: CancellationToken, run_options: Option<Rc<IAnythingQuickAccessProviderRunOptions>>) -> DisposableRef {
        let disposables = DisposableStore::new();

        // Update the pick state for this run
        self.set_pick_state(&picker, run_options.as_ref().is_none_or(|options| options.file_search_delegate.is_none()));
        self.link_source_group.set(if self.supports_links(run_options.as_deref()) { Some(self.services.editor_groups_service.active_group().id()) } else { None });

        // Add editor decorations for active editor symbol picks
        let editor_decorations_disposable = MutableDisposable::<dyn IDisposable>::new();
        disposables.add_rc(editor_decorations_disposable.clone());
        let this = self.self_rc();
        let decorations_picker = picker.clone();
        let decorations = editor_decorations_disposable.clone();
        disposables.add(picker.on_did_change_active().listen(move |_| {
            // Clear old decorations
            decorations.set(None);

            // Add new decoration if editor symbol is active
            let active = decorations_picker.active_items();
            if is_editor_symbol_quick_pick_item(active.first()) {
                if let Some(item) = active.first() {
                    decorations.set(this.decorate_and_reveal_symbol_range(item));
                }
            }
        }));

        // Restore view state upon cancellation if we changed it but only when the picker was closed via explicit user
        // gesture and not e.g. when focus was lost because that could mean the user clicked into the editor directly.
        let this = self.self_rc();
        disposables.add(picker.on_did_hide().once().listen(move |event| {
            if event.reason == QuickInputHideReason::Gesture {
                let _ = this.services.editor_view_state.restore();
            }
        }));

        // Start picker
        let default_source: Rc<dyn IPickerQuickAccessSource<IAnythingQuickAccessProviderRunOptions>> = self.self_rc();
        let source = self.picks_override.borrow().clone().unwrap_or(default_source);
        disposables.add(self.picker_base.provide(source, picker, token, run_options));

        disposables
    }

    fn decorate_and_reveal_symbol_range(&self, pick: &IQuickPickItem) -> Option<Rc<dyn IDisposable>> {
        let data = PickerItemHandlers::data_of::<AnythingItemData>(pick)?;
        let (resource, range) = (data.resource.clone()?, data.range?);
        let active_editor = self.services.editor_service.active_editor();
        let active_resource = EditorResourceAccessor::get_original_uri(active_editor.as_ref(), None);
        if !self.services.uri_identity_service.ext_uri().is_equal(Some(&resource), active_resource.as_ref(), false) {
            return None; // active editor needs to be for resource
        }

        let active_editor_control = self.services.editor_service.active_text_editor_control()?; // we need a text editor control to decorate and reveal

        // we must remember our current view state to be able to restore
        self.services.editor_view_state.set();

        // Decorate (the reveal in center with a smooth scroll belongs to the editor control, which the contract reaches through addDecorations)
        self.services.editor_symbols_quick_access.add_decorations(&active_editor_control, &range.decoration);

        let this = self.self_rc();
        Some(to_disposable(move || this.services.editor_symbols_quick_access.clear_decorations(&active_editor_control)))
    }

    fn supports_links(&self, run_options: Option<&IAnythingQuickAccessProviderRunOptions>) -> bool {
        !run_options.is_some_and(|options| options.files_only == Some(true))
            && run_options.is_none_or(|options| options.file_search_delegate.is_none())
            && run_options.is_none_or(|options| options.base.handle_accept.is_none())
            && self.pick_state.is_quick_navigating.get() != Some(true)
    }

    /// `_getLinkPick`.
    fn get_link_pick(&self, filter: &str) -> Option<IQuickPickItem> {
        // Require an explicit hierarchical URI, not a drive letter, file query, or Quick Access prefix.
        let scheme_end = filter.find("://")?;
        let scheme = &filter[..scheme_end];
        let mut scheme_chars = scheme.chars();
        let valid_scheme = scheme.chars().count() >= 2
            && scheme_chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
        if !valid_scheme {
            return None;
        }

        let invalid_link = || {
            let mut item = IQuickPickItem::default();
            item.base.label = localize("invalidQuickOpenLink", "Invalid Link", &[]).into();
            item.base.description = Some(localize("invalidQuickOpenLinkDescription", "Enter a valid link to open it from Quick Open.", &[]).into());
            Some(item)
        };

        let validated = (|| -> Result<URI, Error> {
            let resource = URI::parse(filter, true);
            let resource_scheme = resource.scheme().to_string_lossy().to_lowercase();
            if filter.chars().any(|c| c.is_whitespace() || c == '\\')
                || [schemas::COMMAND, schemas::DATA, schemas::INTERNAL, "javascript"].contains(&resource_scheme.as_str())
            {
                return Err(Error::new("Unsupported link"));
            }
            if resource_scheme == schemas::HTTP || resource_scheme == schemas::HTTPS {
                // `new URL(filter)`: o autor da URL precisa existir e o host não pode ser inválido.
                let after = &filter[scheme_end + 3..];
                let authority_end = after.find(['/', '?', '#']).unwrap_or(after.len());
                let authority = &after[..authority_end];
                let host_and_port = authority.rsplit('@').next().unwrap_or("");
                if host_and_port.is_empty() || host_and_port.starts_with('[') && !host_and_port.contains(']') {
                    return Err(Error::new("Invalid URL"));
                }
                if resource.authority().is_empty() {
                    return Err(Error::new("Missing link authority"));
                }
            }
            Ok(resource)
        })();
        let Ok(resource) = validated else {
            return invalid_link();
        };

        let services = self.services.clone();
        let this = self.self_rc();
        let source_group = self.link_source_group.get().unwrap_or_else(|| services.editor_groups_service.active_group().id());
        let open_to_side_label = localize("openLinkToSide", "Open to the Side", &[]);
        let filter_text = filter.to_string();
        let open = {
            let (this, filter_text) = (this.clone(), filter_text.clone());
            Rc::new(move |key_mods: IKeyMods, in_background: bool, force_open_side_by_side: bool| -> Promise<()> {
                let (this, filter_text) = (this.clone(), filter_text.clone());
                async_fn(async move {
                    let outcome: Result<(), Error> = async {
                        let configuration = this.configuration();
                        let open_to_side = key_mods.alt || (configuration.open_editor_pinned && key_mods.ctrl_cmd) || force_open_side_by_side;
                        if open_to_side {
                            this.services.editor_view_state.restore().await?;
                            if this.services.editor_groups_service.get_group(source_group).is_some() {
                                this.services.editor_groups_service.activate_group(GroupRef::Id(source_group));
                            }
                        }

                        let handled = this
                            .services
                            .opener_service
                            .open(
                                &OpenResource::Text(filter_text.as_str().into()),
                                Some(&OpenOptions {
                                    from_user_gesture: Some(true),
                                    allow_contributed_openers: Some(je_platform::opener::opener::AllowContributedOpeners::Bool(true)),
                                    open_to_side: Some(open_to_side),
                                    editor_options: Some(EditorOptions {
                                        preserve_focus: Some(in_background),
                                        pinned: Some(key_mods.ctrl_cmd || in_background || configuration.open_editor_pinned),
                                        ..EditorOptions::default()
                                    }),
                                    ..OpenOptions::default()
                                }),
                            )
                            .await?;
                        if !handled {
                            this.services.notification_service.warn(vec![NotificationMessage::Text(localize(
                                "quickOpenLinkNotOpened",
                                "The link could not be opened. It may be unsupported or opening it was canceled.",
                                &[],
                            ))]);
                        }
                        Ok(())
                    }
                    .await;
                    if let Err(error) = outcome {
                        this.services.notification_service.error(vec![NotificationMessage::Error(error)]);
                    }
                    Ok(())
                })
            })
        };

        let configuration = self.configuration();
        let mut item = IQuickPickItem::default();
        item.base.label = format!("$(link-external) {}", localize("openQuickOpenLink", "Open Link", &[])).into();
        item.base.description = Some(filter_text.as_str().into());
        item.detail = Some(localize("openQuickOpenLinkHint", "Hold Alt while accepting to open to the side.", &[]));
        item.base.buttons = Some(vec![Rc::new(IQuickInputButton {
            icon_class: Some(if configuration.open_side_by_side_direction.as_deref() == Some("right") { "codicon-split-horizontal" } else { "codicon-split-vertical" }.to_string()),
            tooltip: Some(open_to_side_label),
            ..IQuickInputButton::default()
        })]);
        let accept_open = open.clone();
        let trigger_open = open;
        PickerItemHandlers {
            accept: Some(Rc::new(move |key_mods, event| {
                let _ = accept_open(*key_mods, event.in_background, false);
            })),
            trigger: Some(Rc::new(move |_button_index, key_mods| {
                let opened = trigger_open(*key_mods, false, true);
                TriggerResult::Later(opened.then(|_| TriggerAction::ClosePicker))
            })),
            attach: None,
            data: Some(Rc::new(AnythingItemData { resource: Some(resource), editor: None, range: None })),
        }
        .attach_to(&mut item);
        Some(item)
    }

    fn new_resource_map<T>(&self) -> ResourceMap<T> {
        let service = self.services.uri_identity_service.clone();
        ResourceMap::with_key_fn(Rc::new(move |uri: &URI| service.ext_uri().get_comparison_key(uri, false)))
    }

    fn score(&self, item: &IQuickPickItem, query: &PreparedQuery, allow_non_contiguous: bool, accessor: &QuickPickItemScorerAccessor) -> (i32, Option<Vec<Match>>, Option<Vec<Match>>) {
        let with_resource = IQuickPickItemWithResource { item: item.clone(), resource: resource_of(item) };
        let score = score_item_fuzzy(Some(&with_resource), query, allow_non_contiguous, accessor, &mut self.pick_state.scorer_cache.borrow_mut(), &Platform::detect());
        (score.score, score.label_match.clone(), score.description_match.clone())
    }

    fn compare(&self, a: &IQuickPickItem, b: &IQuickPickItem, query: &PreparedQuery, allow_non_contiguous: bool, accessor: &QuickPickItemScorerAccessor) -> f64 {
        let a = IQuickPickItemWithResource { item: a.clone(), resource: resource_of(a) };
        let b = IQuickPickItemWithResource { item: b.clone(), resource: resource_of(b) };
        f64::from(compare_items_by_fuzzy_score(&a, &b, query, allow_non_contiguous, accessor, &mut self.pick_state.scorer_cache.borrow_mut(), &Platform::detect()))
    }

    fn doget_picks(
        &self,
        filter: &str,
        options: &IAnythingQuickAccessProviderRunOptions,
        enable_editor_symbol_search: bool,
        disposables: &Rc<DisposableStore>,
        token: CancellationToken,
    ) -> ProvidedPicks {
        let query = prepare_query(&units(filter), &Platform::detect());

        // Return early if we have editor symbol picks. We support this by:
        // - having a previously active global pick (e.g. a file)
        // - the user typing `@` to start the local symbol query
        if enable_editor_symbol_search {
            if let Some(editor_symbol_picks) = self.get_editor_symbol_picks(&query, disposables, token.clone()) {
                return ProvidedPicks::Promised(editor_symbol_picks.then(PromisedPicks::Picks));
            }
        }

        // If we have a known last active editor symbol pick, we try to restore the last global pick to support the case of
        // narrowing out from a editor symbol search back into the global search
        let active_pick = self.current_picker().and_then(|picker| picker.active_items().into_iter().next());
        if is_editor_symbol_quick_pick_item(active_pick.as_ref()) {
            if let Some(last_global_picks) = self.pick_state.last_global_picks.borrow().clone() {
                return ProvidedPicks::Picks(Picks::WithActive(last_global_picks));
            }
        }

        // Otherwise return normally with history and file/symbol results
        let history_editor_picks = self.get_editor_history_picks(&query);

        let mut picks: Vec<QuickPickInput> = Vec::new();
        if let Some(addition_picks) = &options.addition_picks {
            for pick in addition_picks {
                let QuickPickInput::Item(item) = pick else {
                    picks.push(pick.clone());
                    continue;
                };
                let mut item = item.clone();
                if query.original.is_empty() {
                    item.highlights = None;
                    picks.push(QuickPickInput::Item(item));
                    continue;
                }
                let (score, label_match, description_match) = self.score(&item, &query, true, &QuickPickItemScorerAccessor::default());
                if score == 0 {
                    continue;
                }
                item.highlights = Some(IQuickPickItemHighlights { label: label_match, description: description_match, detail: None });
                picks.push(QuickPickInput::Item(item));
            }
        }
        let recently_opened_separator = || QuickPickInput::Separator(IQuickPickSeparator { label: Some(localize("recentlyOpenedSeparator", "recently opened", &[])), ..IQuickPickSeparator::default() });
        if self.pick_state.is_quick_navigating.get() == Some(true) {
            picks = history_editor_picks.iter().cloned().map(QuickPickInput::Item).collect();
        } else {
            if options.include_help == Some(true) {
                picks.extend(self.services.help_picks.get_help_picks(&query, options).into_iter().map(QuickPickInput::Item));
            }
            if !history_editor_picks.is_empty() {
                picks.push(recently_opened_separator());
                picks.extend(history_editor_picks.iter().cloned().map(QuickPickInput::Item));
            }
        }

        let fast_picks: Vec<QuickPickInput> = match &options.filter {
            Some(filter) => picks.into_iter().filter(|pick| filter(pick)).collect(),
            None => picks,
        };

        // Slow picks: files and symbols
        let this = self.self_rc();
        let options = options.clone();
        let additional_picks = async_fn(async move {
            // Exclude any result that is already present in editor history.
            let mut additional_picks_excludes: ResourceMap<bool> = this.new_resource_map();
            for history_editor_pick in &history_editor_picks {
                if let Some(resource) = resource_of(history_editor_pick) {
                    additional_picks_excludes.set(resource, true);
                }
            }

            let include_symbols = this.configuration().include_symbols;
            let mut additional = this.get_additional_picks(&query, &additional_picks_excludes, include_symbols, token.clone(), Some(&options)).await?;
            if let Some(filter) = &options.filter {
                additional.retain(|pick| filter(&QuickPickInput::Item(pick.clone())));
            }
            if token.is_cancellation_requested() {
                return Ok(Picks::Items(Vec::new()));
            }

            if additional.is_empty() {
                return Ok(Picks::Items(Vec::new()));
            }
            let separator_label = if include_symbols {
                localize("fileAndSymbolResultsSeparator", "file and symbol results", &[])
            } else {
                localize("fileResultsSeparator", "file results", &[])
            };
            let mut items = vec![QuickPickInput::Separator(IQuickPickSeparator { label: Some(separator_label), ..IQuickPickSeparator::default() })];
            items.extend(additional.into_iter().map(QuickPickInput::Item));
            Ok(Picks::Items(items))
        });

        ProvidedPicks::FastAndSlow(FastAndSlowPicks {
            // Fast picks: help (if included) & editor history
            picks: Picks::Items(fast_picks),
            additional_picks,
            // allow some time to merge files and symbols to reduce flickering
            merge_delay: Some(SYMBOL_PICKS_MERGE_DELAY),
        })
    }

    fn get_additional_picks(
        &self,
        query: &PreparedQuery,
        excludes: &ResourceMap<bool>,
        include_symbols: bool,
        token: CancellationToken,
        run_options: Option<&IAnythingQuickAccessProviderRunOptions>,
    ) -> Promise<Vec<IQuickPickItem>> {
        let this = self.self_rc();
        let (query, excludes_keys) = (query.clone(), excludes.keys().cloned().collect::<Vec<_>>());
        let run_options = run_options.cloned();
        async_fn(async move {
            let mut excludes: ResourceMap<bool> = this.new_resource_map();
            for key in excludes_keys {
                excludes.set(key, true);
            }
            // Resolve file and symbol picks (if enabled)
            let file_picks = this.get_file_picks(&query, &excludes, token.clone(), run_options.as_ref());
            let symbol_picks = this.get_workspace_symbol_picks(&query, include_symbols, token.clone());
            let (file_picks, symbol_picks) = (file_picks.await?, symbol_picks.await?);

            if token.is_cancellation_requested() {
                return Ok(Vec::new());
            }

            // Perform sorting (top results by score)
            let mut all = file_picks;
            all.extend(symbol_picks);
            let accessor = QuickPickItemScorerAccessor::default();
            let sorted_anything_picks = top(&all, |a, b| this.compare(a, b, &query, true, &accessor), MAX_RESULTS);

            // Perform filtering
            let mut filtered_anything_picks: Vec<IQuickPickItem> = Vec::new();
            for mut anything_pick in sorted_anything_picks {
                // Always preserve any existing highlights (e.g. from workspace symbols)
                if anything_pick.highlights.is_some() {
                    filtered_anything_picks.push(anything_pick);
                }
                // Otherwise, do the scoring and matching here
                else {
                    let (score, label_match, description_match) = this.score(&anything_pick, &query, true, &accessor);
                    if score == 0 {
                        continue;
                    }

                    anything_pick.highlights = Some(IQuickPickItemHighlights { label: label_match, description: description_match, detail: None });
                    filtered_anything_picks.push(anything_pick);
                }
            }

            Ok(filtered_anything_picks)
        })
    }

    //#region Editor History

    fn get_editor_history_picks(&self, query: &PreparedQuery) -> Vec<IQuickPickItem> {
        let configuration = self.configuration();

        // Just return all history entries if not searching
        if query.normalized.is_empty() {
            return self.services.history_service.get_history().iter().map(|editor| self.create_anything_pick_editor(editor, &configuration)).collect();
        }

        if !configuration.include_history {
            return Vec::new(); // disabled when searching
        }

        // Perform filtering
        let default_accessor = QuickPickItemScorerAccessor::default();
        // Only match on label of the editor unless the search includes path separators
        let editor_history_scorer_accessor = if query.contains_path_separator { &default_accessor } else { &self.label_only_editor_history_pick_accessor };
        let mut editor_history_picks: Vec<IQuickPickItem> = Vec::new();
        for editor in self.services.history_service.get_history() {
            if EditorResourceAccessor::get_original_uri(Some(&editor), None).is_none() {
                continue;
            }

            let mut editor_history_pick = self.create_anything_pick_editor(&editor, &configuration);

            let (score, label_match, description_match) = self.score(&editor_history_pick, query, false, editor_history_scorer_accessor);
            if score == 0 {
                continue; // exclude editors not matching query
            }

            editor_history_pick.highlights = Some(IQuickPickItemHighlights { label: label_match, description: description_match, detail: None });
            editor_history_picks.push(editor_history_pick);
        }

        // Return without sorting if settings tell to sort by recency
        if configuration.history_filter_sort_order.as_deref() == Some("recency") {
            return editor_history_picks;
        }

        // Perform sorting
        editor_history_picks.sort_by(|a, b| self.compare(a, b, query, false, editor_history_scorer_accessor).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal));
        editor_history_picks
    }

    //#endregion

    //#region File Search

    fn get_file_picks(&self, query: &PreparedQuery, excludes: &ResourceMap<bool>, token: CancellationToken, run_options: Option<&IAnythingQuickAccessProviderRunOptions>) -> Promise<Vec<IQuickPickItem>> {
        let this = self.self_rc();
        let query = query.clone();
        let excludes_keys: Vec<URI> = excludes.keys().cloned().collect();
        let run_options = run_options.cloned();
        async_fn(async move {
            if query.normalized.is_empty() {
                return Ok(Vec::new());
            }
            let mut excludes: ResourceMap<bool> = this.new_resource_map();
            for key in excludes_keys {
                excludes.set(key, true);
            }

            let file_search_delegate = run_options.as_ref().and_then(|options| options.file_search_delegate.clone());

            // Absolute path result
            let absolute_path_result = if file_search_delegate.is_none() { this.get_absolute_path_file_result(&query, token.clone()).await? } else { None };
            if token.is_cancellation_requested() {
                return Ok(Vec::new());
            }

            // Use absolute path result as only results if present
            if let Some(absolute_path_result) = absolute_path_result {
                if excludes.has(&absolute_path_result) {
                    return Ok(Vec::new()); // excluded
                }

                // Create a single result pick and make sure to apply full highlights to ensure the pick is displayed. Since a
                // ~ might have been used for searching, our fuzzy scorer may otherwise not properly respect the pick as a result
                let mut absolute_path_pick = this.create_anything_pick_resource(&absolute_path_result, &this.configuration(), None, true);
                let label_length = absolute_path_pick.base.label.len();
                let description_length = absolute_path_pick.base.description.as_ref().map(JsString::len).filter(|length| *length > 0);
                absolute_path_pick.highlights = Some(IQuickPickItemHighlights {
                    label: Some(vec![Match { start: 0, end: label_length }]),
                    description: description_length.map(|end| vec![Match { start: 0, end }]),
                    detail: None,
                });

                return Ok(vec![absolute_path_pick]);
            }

            // Otherwise run the file search (with a delayer if cache is not ready yet)
            let file_matches: Vec<URI> = if this.pick_state.file_query_cache.borrow().as_ref().is_some_and(|cache| cache.is_loaded()) {
                this.do_file_search(&query, token.clone(), file_search_delegate.clone()).await?
            } else {
                let (delayer_this, delayer_query, delayer_token, delayer_delegate) = (this.clone(), query.clone(), token.clone(), file_search_delegate.clone());
                this.file_query_delayer
                    .trigger(
                        move |_| {
                            if delayer_token.is_cancellation_requested() {
                                return Promise::resolved(Vec::new());
                            }
                            delayer_this.do_file_search(&delayer_query, delayer_token.clone(), delayer_delegate.clone())
                        },
                        None,
                    )
                    .await?
            };

            if token.is_cancellation_requested() {
                return Ok(Vec::new());
            }

            // Filter excludes & convert to picks
            let configuration = this.configuration();
            let include_open_to_side_button = run_options.as_ref().and_then(|options| options.include_open_to_side_button).unwrap_or(true);
            Ok(file_matches
                .iter()
                .filter(|resource| !excludes.has(resource))
                .map(|resource| this.create_anything_pick_resource(resource, &configuration, file_search_delegate.as_ref(), include_open_to_side_button))
                .collect())
        })
    }

    fn do_file_search(&self, query: &PreparedQuery, token: CancellationToken, file_search_delegate: Option<IAnythingQuickAccessFileSearchDelegate>) -> Promise<Vec<URI>> {
        let this = self.self_rc();
        let query = query.clone();
        async_fn(async move {
            if let Some(delegate) = file_search_delegate {
                let search = delegate.search.clone();
                return this.get_file_search_results(&query, token, Rc::new(move |file_pattern, token| search(file_pattern, token))).await;
            }

            // File search: this is a search over all files of the workspace using the provided pattern
            let default_this = this.clone();
            let file_search_results = this.get_file_search_results(&query, token.clone(), Rc::new(move |pattern, token| default_this.do_get_file_search_results(pattern, token)));
            // Relative path search: we also want to consider results that match files inside the workspace by looking for relative
            // paths that the user typed as query. This allows to return even excluded results into the picker if found.
            let relative_path_file_results = this.get_relative_path_file_results(&query, token.clone());
            let (file_search_results, relative_path_file_results) = (file_search_results.await?, relative_path_file_results.await?);

            if token.is_cancellation_requested() {
                return Ok(Vec::new());
            }

            // Return quickly if no relative results are present
            let Some(relative_path_file_results) = relative_path_file_results else {
                return Ok(file_search_results);
            };

            // Otherwise, make sure to filter relative path results from the search results to prevent duplicates
            let mut relative_path_file_results_map: ResourceMap<bool> = this.new_resource_map();
            for relative_path_file_result in &relative_path_file_results {
                relative_path_file_results_map.set(relative_path_file_result.clone(), true);
            }

            let mut results: Vec<URI> = file_search_results.into_iter().filter(|result| !relative_path_file_results_map.has(result)).collect();
            results.extend(relative_path_file_results);
            Ok(results)
        })
    }

    fn get_file_search_results(&self, query: &PreparedQuery, token: CancellationToken, search: Rc<dyn Fn(&str, CancellationToken) -> Promise<ISearchComplete>>) -> Promise<Vec<URI>> {
        let this = self.self_rc();
        let query = query.clone();
        async_fn(async move {
            // filePattern for search depends on the number of queries in input:
            // - with multiple: only take the first one and let the filter later drop non-matching results
            // - with single: just take the original in full
            //
            // This enables to e.g. search for "someFile someFolder" by only returning search results for "someFile" and not both
            // that would normally not match.
            let file_pattern = match &query.values {
                Some(values) if values.len() > 1 => values[0].original.to_string_lossy(),
                _ => query.original.to_string_lossy(),
            };

            let mut file_search_results = search(&file_pattern, token.clone()).await?;
            if token.is_cancellation_requested() {
                return Ok(Vec::new());
            }

            // If we detect that the search limit has been hit and we have a query that was composed of multiple inputs where we
            // only took the first part we run another search with the full original query included to make sure we are including
            // all possible results that could match.
            if file_search_results.limit_hit == Some(true) && query.values.as_ref().is_some_and(|values| values.len() > 1) {
                let additional_file_search_results = search(&query.original.to_string_lossy(), token.clone()).await?;
                if token.is_cancellation_requested() {
                    return Ok(Vec::new());
                }

                // Remember which result we already covered
                let mut existing_file_search_results_map: ResourceMap<bool> = this.new_resource_map();
                for file_search_result in &file_search_results.results {
                    existing_file_search_results_map.set(file_search_result.resource.clone(), true);
                }

                // Add all additional results to the original set for inclusion
                for additional_file_search_result in additional_file_search_results.results {
                    if !existing_file_search_results_map.has(&additional_file_search_result.resource) {
                        file_search_results.results.push(additional_file_search_result);
                    }
                }
            }

            let _ = this;
            Ok(file_search_results.results.into_iter().map(|result| result.resource).collect())
        })
    }

    fn do_get_file_search_results(&self, file_pattern: &str, token: CancellationToken) -> Promise<ISearchComplete> {
        let start = runtime::date_now();
        let query = self.file_query_builder.file(
            &self.services.context_service.get_workspace().folders().into_iter().map(WorkspaceFolderOrUri::Folder).collect::<Vec<_>>(),
            &self.get_file_query_options(
                Some(file_pattern.to_string()),
                self.pick_state.file_query_cache.borrow().as_ref().map(|cache| cache.cache_key()),
                Some(MAX_RESULTS as i64),
            ),
        );
        let log_service = self.services.log_service.clone();
        self.services.search_service.file_search(&query, Some(token)).finally(move || {
            log_service.trace(&format!("QuickAccess fileSearch {}ms", runtime::date_now() - start));
        })
    }

    fn get_file_query_options(&self, file_pattern: Option<String>, cache_key: Option<String>, max_results: Option<i64>) -> IFileQueryBuilderOptions {
        IFileQueryBuilderOptions {
            common: ICommonQueryBuilderOptions {
                reason: Some("openFileHandler".to_string()), // used for telemetry - do not change
                extra_file_resources: Some((self.services.out_of_workspace_editor_resources)()),
                max_results: Some(max_results.filter(|max| *max != 0).unwrap_or(0)),
                ..ICommonQueryBuilderOptions::default()
            },
            file_pattern: Some(file_pattern.unwrap_or_default()),
            cache_key,
            sort_by_score: Some(true),
            ..IFileQueryBuilderOptions::default()
        }
    }

    fn get_absolute_path_file_result(&self, query: &PreparedQuery, token: CancellationToken) -> Promise<Option<URI>> {
        let this = self.self_rc();
        let query = query.clone();
        async_fn(async move {
            if !query.contains_path_separator {
                return Ok(None);
            }

            let user_home = this.services.path_service.user_home().await?;
            let home_path = if user_home.scheme().to_string_lossy() == schemas::FILE { je_platform::resources::fs_path(&user_home) } else { user_home.path().to_string_lossy() };
            let detildified_query = untildify(&query.original.to_string_lossy(), &home_path);
            if token.is_cancellation_requested() {
                return Ok(None);
            }

            let flavor = this.services.path_service.path().await?;
            let is_absolute_path_query = is_absolute_for_flavor(flavor, &detildified_query);
            if token.is_cancellation_requested() {
                return Ok(None);
            }

            if is_absolute_path_query {
                let resource = je_base::resources::to_local_resource(
                    &this.services.path_service.file_uri(&detildified_query).await?,
                    this.services.environment_service.remote_authority().as_deref(),
                    &this.services.path_service.default_uri_scheme(),
                );

                if token.is_cancellation_requested() {
                    return Ok(None);
                }

                // ignore if file does not exist
                if let Ok(stat) = this.services.file_service.stat(&resource).await {
                    if stat.is_file {
                        return Ok(Some(this.match_filename_casing(&resource).await?));
                    }
                }
            }

            Ok(None)
        })
    }

    fn get_relative_path_file_results(&self, query: &PreparedQuery, token: CancellationToken) -> Promise<Option<Vec<URI>>> {
        let this = self.self_rc();
        let query = query.clone();
        async_fn(async move {
            if !query.contains_path_separator {
                return Ok(None);
            }

            // Convert relative paths to absolute paths over all folders of the workspace and return them as results if the
            // absolute paths exist
            let original = query.original.to_string_lossy();
            let flavor = this.services.path_service.path().await?;
            let is_absolute_path_query = is_absolute_for_flavor(flavor, &original);
            if !is_absolute_path_query {
                let mut resources: Vec<URI> = Vec::new();
                for folder in this.services.context_service.get_workspace().folders() {
                    if token.is_cancellation_requested() {
                        break;
                    }

                    let resource = je_base::resources::to_local_resource(
                        &folder.to_resource(&original),
                        this.services.environment_service.remote_authority().as_deref(),
                        &this.services.path_service.default_uri_scheme(),
                    );

                    // ignore if file does not exist
                    if let Ok(stat) = this.services.file_service.stat(&resource).await {
                        if stat.is_file {
                            resources.push(this.match_filename_casing(&resource).await?);
                        }
                    }
                }

                return Ok(Some(resources));
            }

            Ok(None)
        })
    }

    /// Attempts to match the filename casing to file system by checking the parent folder's children.
    fn match_filename_casing(&self, resource: &URI) -> Promise<URI> {
        let this = self.self_rc();
        let resource = resource.clone();
        async_fn(async move {
            let parent = this.services.uri_identity_service.ext_uri().dirname(&resource);
            let stat = this.services.file_service.resolve(&parent, Some(ResolveFileOptions { resolve_to: vec![resource.clone()], ..ResolveFileOptions::default() })).await?;
            if let Some(children) = &stat.children {
                let ext_uri = this.services.uri_identity_service.ext_uri();
                if let Some(found) = children.iter().find(|child| ext_uri.is_equal(Some(&child.resource), Some(&resource), false)) {
                    return Ok(URI::join_path(&parent, &[found.name.as_str()], je_platform::resources::host_platform()));
                }
            }
            Ok(resource)
        })
    }

    //#endregion

    //#region Workspace Symbols (if enabled)

    fn get_workspace_symbol_picks(&self, query: &PreparedQuery, include_symbols: bool, token: CancellationToken) -> Promise<Vec<IQuickPickItem>> {
        if query.normalized.is_empty() // we need a value for search for
            || !include_symbols // we need to enable symbols in search
            || self.pick_state.last_range.get().is_some()
        // a range is an indicator for just searching for files
        {
            return Promise::resolved(Vec::new());
        }

        // Delegate to the existing symbols quick access but skip local results and also do not score
        self.services.workspace_symbols_quick_access.get_symbol_picks(&query.original.to_string_lossy(), true, true, TYPING_SEARCH_DELAY, token)
    }

    //#endregion

    //#region Editor Symbols (if narrowing down into a global pick via `@`)

    fn get_editor_symbol_picks(&self, query: &PreparedQuery, disposables: &Rc<DisposableStore>, token: CancellationToken) -> Option<Promise<Picks>> {
        let original = query.original.to_string_lossy();
        let filter_segments: Vec<&str> = original.split(GOTO_SYMBOL_PREFIX).collect();
        let filter = if filter_segments.len() > 1 { Some(filter_segments[filter_segments.len() - 1].trim().to_string()) } else { None };
        let filter = filter?; // we need to be searched for editor symbols via `@`

        let active_global_pick = self.pick_state.last_global_picks.borrow().as_ref().and_then(|picks| picks.active.clone())?; // we need an active global pick to find symbols for

        let active_global_resource = resource_of(&active_global_pick)?;
        if !self.services.file_service.has_provider(&active_global_resource) && active_global_resource.scheme().to_string_lossy() != schemas::UNTITLED {
            return None; // we need a resource that we can resolve
        }

        let label_has_prefix = active_global_pick.base.label.to_string_lossy().contains(GOTO_SYMBOL_PREFIX);
        let description_has_prefix = active_global_pick.base.description.as_ref().is_some_and(|description| description.to_string_lossy().contains(GOTO_SYMBOL_PREFIX));
        if (label_has_prefix || description_has_prefix) && filter_segments.len() < 3 {
            return None; // require at least 2 `@` if our active pick contains `@` in label or description
        }

        Some(self.do_get_editor_symbol_picks(active_global_pick, active_global_resource, filter, disposables.clone(), token))
    }

    fn do_get_editor_symbol_picks(&self, active_global_pick: IQuickPickItem, active_global_resource: URI, filter: String, disposables: Rc<DisposableStore>, token: CancellationToken) -> Promise<Picks> {
        let this = self.self_rc();
        async_fn(async move {
            // Bring the editor to front to review symbols to go to
            // we must remember our current view state to be able to restore
            this.services.editor_view_state.set();

            // open it
            let opened = this
                .services
                .editor_view_state
                .open_transient_editor(UntypedEditorInput {
                    resource: Some(active_global_resource.clone()),
                    options: Some(TextEditorOptions::from(EditorOptions {
                        preserve_focus: Some(true),
                        reveal_if_opened: Some(true),
                        ignore_error: Some(true),
                        ..EditorOptions::default()
                    })),
                    ..UntypedEditorInput::default()
                })
                .await;
            if opened.is_err() {
                return Ok(Picks::Items(Vec::new())); // return if resource cannot be opened
            }

            if token.is_cancellation_requested() {
                return Ok(Picks::Items(Vec::new()));
            }

            // Obtain model from resource
            let model = match this.services.model_service.get_model(&active_global_resource) {
                Some(model) => model,
                None => match this.services.text_model_service.create_model_reference(&active_global_resource).await {
                    Ok(model_reference) => {
                        disposables.add_rc(Rc::new(model_reference.clone()));
                        if token.is_cancellation_requested() {
                            return Ok(Picks::Items(Vec::new()));
                        }
                        let text_model = model_reference.object.text_editor_model();
                        disposables.add(Rc::new(model_reference));
                        match text_model {
                            Some(model) => model,
                            None => return Ok(Picks::Items(Vec::new())),
                        }
                    }
                    Err(_) => return Ok(Picks::Items(Vec::new())), // return if model cannot be resolved
                },
            };

            // Ask provider for editor symbols
            let label = je_base::icon_labels::strip_icons(active_global_pick.base.label.units()).to_string_lossy();
            let editor_symbol_picks = this.services.editor_symbols_quick_access.get_symbol_picks(&model, &filter, &label, &disposables, token.clone()).await?;
            if token.is_cancellation_requested() {
                return Ok(Picks::Items(Vec::new()));
            }

            Ok(Picks::Items(
                editor_symbol_picks
                    .into_iter()
                    .map(|editor_symbol_pick| match editor_symbol_pick {
                        // Preserve separators
                        EditorSymbolPick::Separator(separator) => QuickPickInput::Separator(separator),
                        // Convert editor symbols to anything pick
                        EditorSymbolPick::Item { mut item, range } => {
                            let selection = range.map(|range| range.selection);
                            let (trigger_this, accept_this) = (this.clone(), this.clone());
                            let (trigger_resource, accept_resource) = (active_global_resource.clone(), active_global_resource.clone());
                            PickerItemHandlers {
                                trigger: Some(Rc::new(move |_button_index, key_mods| {
                                    let _ = trigger_this.open_anything(
                                        &AnythingTarget::Resource(trigger_resource.clone()),
                                        OpenAnythingOptions { key_mods: Some(*key_mods), range: selection, force_open_side_by_side: true, ..OpenAnythingOptions::default() },
                                    );
                                    TriggerResult::Now(TriggerAction::ClosePicker)
                                })),
                                accept: Some(Rc::new(move |key_mods, event| {
                                    let _ = accept_this.open_anything(
                                        &AnythingTarget::Resource(accept_resource.clone()),
                                        OpenAnythingOptions {
                                            key_mods: Some(*key_mods),
                                            range: selection,
                                            preserve_focus: Some(event.in_background),
                                            force_pinned: event.in_background,
                                            ..OpenAnythingOptions::default()
                                        },
                                    );
                                })),
                                attach: None,
                                data: Some(Rc::new(AnythingItemData { resource: Some(active_global_resource.clone()), editor: None, range })),
                            }
                            .attach_to(&mut item);
                            QuickPickInput::Item(item)
                        }
                    })
                    .collect(),
            ))
        })
    }

    //#endregion

    //#region Helpers

    fn create_anything_pick_resource(
        &self,
        resource: &URI,
        configuration: &Configuration,
        file_search_delegate: Option<&IAnythingQuickAccessFileSearchDelegate>,
        include_open_to_side_button: bool,
    ) -> IQuickPickItem {
        self.create_anything_pick(&AnythingTarget::Resource(resource.clone()), configuration, file_search_delegate, include_open_to_side_button)
    }

    fn create_anything_pick_editor(&self, editor: &EditorInputOrUntyped, configuration: &Configuration) -> IQuickPickItem {
        self.create_anything_pick(&AnythingTarget::Editor(editor.clone()), configuration, None, true)
    }

    fn create_anything_pick(
        &self,
        target: &AnythingTarget,
        configuration: &Configuration,
        file_search_delegate: Option<&IAnythingQuickAccessFileSearchDelegate>,
        include_open_to_side_button: bool,
    ) -> IQuickPickItem {
        let is_editor_history_entry = !matches!(target, AnythingTarget::Resource(_));

        let resource: Option<URI>;
        let label: String;
        let description: Option<String>;
        let is_dirty: bool;
        let extra_classes: Vec<String>;

        match target {
            AnythingTarget::Editor(EditorInputOrUntyped::Typed(editor)) => {
                resource = EditorResourceAccessor::get_original_uri(Some(&EditorInputOrUntyped::Typed(editor.clone())), None);
                label = editor.get_name();
                description = editor.get_description(None);
                is_dirty = editor.is_dirty() && !editor.is_saving();
                extra_classes = editor.get_label_extra_classes();
            }
            _ => {
                let untyped_resource = match target {
                    AnythingTarget::Resource(resource) => Some(resource.clone()),
                    AnythingTarget::Editor(EditorInputOrUntyped::Untyped(untyped)) => untyped.resource.clone(),
                    AnythingTarget::Editor(EditorInputOrUntyped::Typed(_)) => None,
                };
                let ext_uri = je_platform::resources::ext_uri();
                let untyped_resource = untyped_resource.expect("o item sem tipo tem recurso");
                let custom_label = self.services.custom_editor_label_service.get_name(&untyped_resource).filter(|label| !label.is_empty());
                label = custom_label.clone().unwrap_or_else(|| ext_uri.basename_or_authority(&untyped_resource));
                description = Some(match file_search_delegate.and_then(|delegate| delegate.get_item_description.as_ref()) {
                    Some(get_item_description) => get_item_description(&untyped_resource),
                    None => self.services.label_service.get_uri_label(
                        &if custom_label.is_some() { untyped_resource.clone() } else { ext_uri.dirname(&untyped_resource) },
                        &UriLabelOptions { relative: Some(true), ..UriLabelOptions::default() },
                    ),
                });
                is_dirty = self.services.working_copy_service.is_dirty(&untyped_resource, None)
                    && !self.services.files_configuration_service.has_short_auto_save_delay(Some(&crate::services::files_configuration::files_configuration_service::ResourceOrEditor::Resource(untyped_resource.clone())));
                resource = Some(untyped_resource);
                extra_classes = Vec::new();
            }
        }
        let description = description.filter(|description| !description.is_empty());

        let label_and_description = match &description {
            Some(description) => format!("{label} {description}"),
            None => label.clone(),
        };

        let mut icon_classes = je_editor::get_icon_classes::get_icon_classes(&*self.services.model_service, &*self.services.language_service, resource.as_ref(), None, None);
        icon_classes.extend(extra_classes);

        let mut buttons: Vec<QuickInputButtonRef> = Vec::new();
        let open_side_by_side_direction = configuration.open_side_by_side_direction.as_deref();
        if include_open_to_side_button {
            buttons.push(Rc::new(IQuickInputButton {
                icon_class: Some(if open_side_by_side_direction == Some("right") { "codicon-split-horizontal" } else { "codicon-split-vertical" }.to_string()),
                tooltip: Some(if open_side_by_side_direction == Some("right") {
                    localize("openToSide", "Open to the Side", &[])
                } else {
                    localize("openToBottom", "Open to the Bottom", &[])
                }),
                ..IQuickInputButton::default()
            }));
        }

        // Remove from History
        if is_editor_history_entry {
            buttons.push(Rc::new(IQuickInputButton {
                icon_class: Some(if is_dirty { "dirty-anything codicon-circle-filled" } else { "codicon-close" }.to_string()),
                tooltip: Some(localize("closeEditor", "Remove from Recently Opened", &[])),
                always_visible: Some(is_dirty),
                ..IQuickInputButton::default()
            }));
        }

        let mut item = IQuickPickItem::default();
        item.base.label = label.as_str().into();
        item.base.aria_label = Some(if is_dirty { localize("filePickAriaLabelDirty", "{0} unsaved changes", &[&label_and_description]) } else { label_and_description });
        item.base.description = description.as_deref().map(JsString::from);
        item.base.icon_classes = Some(icon_classes);
        item.base.buttons = Some(buttons);

        let trigger_this = self.self_rc();
        let accept_this = self.self_rc();
        let (trigger_target, accept_target, attach_target) = (target.clone(), target.clone(), target.clone());
        PickerItemHandlers {
            trigger: Some(Rc::new(move |button_index, key_mods| {
                if include_open_to_side_button && button_index == 0 {
                    let _ = trigger_this.open_anything(
                        &trigger_target,
                        OpenAnythingOptions { key_mods: Some(*key_mods), range: trigger_this.pick_state.last_range.get(), force_open_side_by_side: true, ..OpenAnythingOptions::default() },
                    );

                    return TriggerResult::Now(TriggerAction::ClosePicker);
                }

                let remove_from_history_button_index = if include_open_to_side_button { 1 } else { 0 };
                if button_index == remove_from_history_button_index {
                    if let AnythingTarget::Editor(editor) = &trigger_target {
                        trigger_this.services.history_service.remove_from_history(editor);

                        return TriggerResult::Now(TriggerAction::RemoveItem);
                    }
                }

                TriggerResult::Now(TriggerAction::NoAction)
            })),
            accept: Some(Rc::new(move |key_mods, event| {
                let _ = accept_this.open_anything(
                    &accept_target,
                    OpenAnythingOptions {
                        key_mods: Some(*key_mods),
                        range: accept_this.pick_state.last_range.get(),
                        preserve_focus: Some(event.in_background),
                        force_pinned: event.in_background,
                        ..OpenAnythingOptions::default()
                    },
                );
            })),
            // Sem widget de chat (fora de escopo), o `attach` cai no comportamento de aceitar.
            attach: Some({
                let attach_this = self.self_rc();
                Rc::new(move |key_mods, event| {
                    let _ = attach_this.open_anything(
                        &attach_target,
                        OpenAnythingOptions {
                            key_mods: Some(*key_mods),
                            range: attach_this.pick_state.last_range.get(),
                            preserve_focus: Some(event.in_background),
                            force_pinned: event.in_background,
                            ..OpenAnythingOptions::default()
                        },
                    );
                })
            }),
            data: Some(Rc::new(AnythingItemData {
                resource,
                editor: match target {
                    AnythingTarget::Editor(editor) => Some(editor.clone()),
                    AnythingTarget::Resource(_) => None,
                },
                range: None,
            })),
        }
        .attach_to(&mut item);
        item
    }

    fn open_anything(&self, target: &AnythingTarget, options: OpenAnythingOptions) -> Promise<()> {
        let this = self.self_rc();
        let target = target.clone();
        async_fn(async move {
            let configuration = this.configuration();

            // Craft some editor options based on quick access usage
            let key_mods = options.key_mods.unwrap_or_default();
            let editor_options = TextEditorOptions {
                base: EditorOptions {
                    preserve_focus: options.preserve_focus,
                    pinned: Some(key_mods.ctrl_cmd || options.force_pinned || configuration.open_editor_pinned),
                    ..EditorOptions::default()
                },
                selection: options.range.map(|range| TextEditorSelection {
                    start_line_number: f64::from(range.start_line_number),
                    start_column: f64::from(range.start_column),
                    end_line_number: Some(f64::from(range.end_line_number)),
                    end_column: Some(f64::from(range.end_column)),
                }),
                ..TextEditorOptions::default()
            };

            let side = key_mods.alt || (configuration.open_editor_pinned && key_mods.ctrl_cmd) || options.force_open_side_by_side;
            let target_group = if side { PreferredGroup::Side } else { PreferredGroup::Active };

            // Restore any view state if the target is the side group
            if side {
                this.services.editor_view_state.restore().await?;
            }

            match &target {
                // Open editor (typed)
                AnythingTarget::Editor(EditorInputOrUntyped::Typed(editor)) => {
                    this.services.editor_service.open_editor_typed(editor, Some(editor_options), Some(target_group)).await?;
                }
                // Open editor (untyped)
                _ => {
                    let resource_editor_input = match &target {
                        AnythingTarget::Resource(resource) => UntypedEditorInput { resource: Some(resource.clone()), options: Some(editor_options), ..UntypedEditorInput::default() },
                        AnythingTarget::Editor(EditorInputOrUntyped::Untyped(untyped)) => {
                            let mut merged = untyped.clone();
                            let mut merged_options = untyped.options.clone().unwrap_or_default();
                            merged_options.base.preserve_focus = editor_options.base.preserve_focus.or(merged_options.base.preserve_focus);
                            merged_options.base.pinned = editor_options.base.pinned.or(merged_options.base.pinned);
                            merged_options.selection = editor_options.selection.clone().or(merged_options.selection);
                            merged.options = Some(merged_options);
                            merged
                        }
                        AnythingTarget::Editor(EditorInputOrUntyped::Typed(_)) => unreachable!("o editor tipado é tratado acima"),
                    };
                    this.services.editor_service.open_editor_in(resource_editor_input, Some(target_group)).await?;
                }
            }
            Ok(())
        })
    }

    //#endregion
}

/// `URI | EditorInput | IResourceEditorInput`.
#[derive(Clone)]
enum AnythingTarget {
    Resource(URI),
    Editor(EditorInputOrUntyped),
}

#[derive(Clone, Default)]
struct OpenAnythingOptions {
    key_mods: Option<IKeyMods>,
    preserve_focus: Option<bool>,
    range: Option<Range>,
    force_open_side_by_side: bool,
    force_pinned: bool,
}

impl IPickerQuickAccessSource<IAnythingQuickAccessProviderRunOptions> for AnythingQuickAccessProvider {
    /// `_getPicks`.
    fn get_picks(
        &self,
        original_filter: &str,
        disposables: &Rc<DisposableStore>,
        token: CancellationToken,
        run_options: Option<&Rc<IAnythingQuickAccessProviderRunOptions>>,
    ) -> ProvidedPicks {
        let default_options = IAnythingQuickAccessProviderRunOptions::default();
        let options: &IAnythingQuickAccessProviderRunOptions = run_options.map(|options| options.as_ref()).unwrap_or(&default_options);

        if options.files_only == Some(true) {
            let this = self.self_rc();
            let query = prepare_query(&units(original_filter), &Platform::detect());
            let excludes: ResourceMap<bool> = this.new_resource_map();
            let promise = this.get_additional_picks(&query, &excludes, false, token, Some(options));
            return ProvidedPicks::Promised(promise.then(|picks| PromisedPicks::Picks(Picks::Items(picks.into_iter().map(QuickPickInput::Item).collect()))));
        }

        if self.supports_links(Some(options)) {
            if let Some(link_pick) = self.get_link_pick(original_filter) {
                *self.pick_state.last_original_filter.borrow_mut() = None;
                *self.pick_state.last_filter.borrow_mut() = None;
                self.pick_state.last_range.set(None);
                *self.pick_state.last_global_picks.borrow_mut() = None;
                let allowed = options.filter.as_ref().is_none_or(|filter| filter(&QuickPickInput::Item(link_pick.clone())));
                return ProvidedPicks::Picks(Picks::Items(if allowed { vec![QuickPickInput::Item(link_pick)] } else { Vec::new() }));
            }
        }

        // Find a suitable range from the pattern looking for ":", "#" or "," unless we have the `@` editor symbol character
        // inside the filter
        let filter_with_range = extract_range_from_filter(original_filter, Some(&[GOTO_SYMBOL_PREFIX]));

        // Update filter with normalized values
        let filter: String = match &filter_with_range {
            Some(filter_with_range) => filter_with_range.filter.clone(),
            None => original_filter.to_string(),
        };

        // Remember as last range
        self.pick_state.last_range.set(filter_with_range.as_ref().map(|filter_with_range| filter_with_range.range));

        // If the original filter value has changed but the normalized one has not, we return early with a `null` result
        // indicating that the results should preserve because the range information (:<line>:<column>) does not need to
        // trigger any re-sorting.
        if self.pick_state.last_original_filter.borrow().as_deref() != Some(original_filter) && self.pick_state.last_filter.borrow().as_deref() == Some(filter.as_str()) {
            return ProvidedPicks::None;
        }

        // Remember as last filter
        let last_was_filtering = self.pick_state.last_original_filter.borrow().as_ref().is_some_and(|last| !last.is_empty());
        *self.pick_state.last_original_filter.borrow_mut() = Some(original_filter.to_string());
        *self.pick_state.last_filter.borrow_mut() = Some(filter.clone());

        // Remember our pick state before returning new picks unless we are inside an editor symbol filter or result. We can use
        // this state to return back to the global pick when the user is narrowing back out of editor symbols.
        if let Some(picker) = self.current_picker() {
            let picks = picker.items();
            if let Some(active_pick) = picker.active_items().into_iter().next() {
                let active_pick_is_editor_symbol = is_editor_symbol_quick_pick_item(Some(&active_pick));
                let active_pick_is_no_results_in_editor_symbols =
                    active_pick.base.label == no_results_pick().base.label && filter.contains(GOTO_SYMBOL_PREFIX);
                if !active_pick_is_editor_symbol && !active_pick_is_no_results_in_editor_symbols {
                    *self.pick_state.last_global_picks.borrow_mut() = Some(PicksWithActive { items: picks, active: Some(active_pick) });
                }
            }
        }

        // `enableEditorSymbolSearch`: this will enable local editor symbol search if the filter value includes `@` character.
        // We only want to enable this support though if the user was filtering in the picker because this feature depends on an
        // active item in the result list to get symbols from. If we would simply trigger editor symbol search without prior
        // filtering, you could not paste a file name including the `@` character to open it (e.g. /some/file@path)
        // refs: https://github.com/microsoft/vscode/issues/93845
        self.doget_picks(&filter, options, last_was_filtering, disposables, token)
    }
}

impl IQuickAccessProvider<IAnythingQuickAccessProviderRunOptions> for AnythingQuickAccessProvider {
    fn default_filter_value(&self) -> Option<DefaultQuickAccessFilterValue> {
        AnythingQuickAccessProvider::default_filter_value(self)
    }

    fn as_disposable(&self) -> Option<&dyn IDisposable> {
        Some(self)
    }

    fn provide(&self, picker: Rc<dyn IQuickPick>, token: CancellationToken, options: Option<Rc<IAnythingQuickAccessProviderRunOptions>>) -> DisposableRef {
        AnythingQuickAccessProvider::provide(self, picker, token, options)
    }
}

impl IDisposable for AnythingQuickAccessProvider {
    fn dispose(&self) {
        self.picker_base.dispose();
        let cache = self.pick_state.file_query_cache.borrow_mut().take();
        if let Some(cache) = cache {
            cache.dispose();
        }
    }
}

/// O `Disposable` base, para quem registra o provedor num `DisposableStore`.
impl AnythingQuickAccessProvider {
    /// Faz o papel de uma subclasse que sobrescreve `_getPicks`.
    pub fn override_get_picks(&self, source: Rc<dyn IPickerQuickAccessSource<IAnythingQuickAccessProviderRunOptions>>) {
        *self.picks_override.borrow_mut() = Some(source);
    }

    pub fn disposable(&self) -> &Rc<Disposable> {
        self.picker_base.disposable()
    }
}

/// `pathService.path` seguido de `isAbsolute(path)`: o sabor (`win32` ou `posix`) decide o módulo de caminho.
fn is_absolute_for_flavor(flavor: je_base::path::Platform, path: &str) -> bool {
    let environment = je_platform::resources::host_environment();
    match flavor {
        je_base::path::Platform::Posix => je_base::path::PathModule::posix(environment.as_ref()).is_absolute(path),
        je_base::path::Platform::Win32 => je_base::path::PathModule::win32(environment.as_ref()).is_absolute(path),
    }
}
