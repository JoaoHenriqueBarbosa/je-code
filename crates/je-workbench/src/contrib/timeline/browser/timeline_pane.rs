//! Porte da primeira metade de `src/vs/workbench/contrib/timeline/browser/timelinePane.ts` (parte A): os tipos auxiliares
//! (`TreeElement`, `LoadMoreCommand`, `TimelineAggregate`, `TimelineIdentityProvider`, `TimelineActionContext`,
//! `getMultiSelection`, `updateRelativeTime`, as chaves de contexto) e a classe [`TimelinePane`] do início até o fim da
//! lógica de carga (campos, construtor, `followActiveEditor`, `pageOnScroll`, `pageSize`, `setUri`, `onActiveEditorChanged`,
//! `onProvidersChanged`, `onTimelineChanged`, `onEditableChanged`, `onSelectionChanged`, `loadTimeline`,
//! `loadTimelineForSource`, `reloadTimeline`, `handleRequest`, `getItems`, `refresh`, `refreshDebounced`, as mensagens de
//! vazio e o estado de paginação).
//!
//! Divergências:
//! - **Herança.** `TimelinePane extends ViewPane` é um [`TimelinePane`] que guarda um `Rc<ViewPane>` e implementa
//!   [`ViewPaneDelegate`] (registrado no construtor por [`ViewPane::set_delegate`]). Os `Disposable` próprios do painel
//!   (`_register`) ficam no campo `base`; o `_store` que o `onDidChangeValue` recebe é o campo `store`.
//! - **Injeção.** Os dezoito parâmetros do construtor são [`IViewPaneOptions`] mais os campos de [`TimelinePaneServices`]
//!   (que embute os [`ViewPaneServices`] da base). O `IInstantiationService`, o `IThemeService`, o `IHoverService` e o
//!   `IKeybindingService` só servem à base ou à parte B (renderer, hover, menus) e entram por lá.
//! - **Elementos da árvore.** `TimelineItem | LoadMoreCommand` é o enum [`TreeElement`]. O `TimelineItem` do original é um
//!   objeto mutável compartilhado entre o agregado, a árvore e o renderer (`item.relativeTime = ...`, `tree.rerender(item)`
//!   por identidade); aqui é o [`TimelineItemRef`] (`Rc<RefCell<TimelineItem>>`), e a igualdade e o hash de [`TreeElement`]
//!   são por identidade do `Rc`, como o `===` do original. O `TimelineIdentityProvider` devolve o `handle`.
//! - **Árvore e mensagem.** `$message`, `$tree`, `tree` e `treeRenderer` nascem em `renderBody` (parte B). A parte A fala com
//!   eles pelo trait [`ITimelineBody`], que a parte B implementa sobre a `WorkbenchObjectTree<TreeElement, FuzzyScore>` e
//!   registra com [`TimelinePane::set_body`]. Sem o corpo (antes do `renderBody`), as operações sobre ele não fazem nada,
//!   que é o que os `this.tree?` e `if (!this.$message) return` do original fazem.
//! - **`setChildren(null, this.getItems())`.** O gerador `getItems` é consumido por inteiro antes da chamada (vira um `Vec`),
//!   porque a árvore consome todos os elementos de qualquer forma.
//! - **`async`.** `loadTimeline` e `handleRequest` são `async` no original, mas `loadTimeline` não tem `await` (roda
//!   síncrona). `handleRequest` espera o `withProgress` e a continuação roda como reação da promessa. O `IProgressService`
//!   do porte devolve `Promise<()>`, então a resposta da timeline sai da tarefa por uma célula compartilhada.
//! - **`@debounce(500)`.** `refreshDebounced` é o `clearTimeout` mais `setTimeout(500)` do decorador, no `runtime`.
//! - **`getActionsContext()`.** Devolve o `URI` como objeto dinâmico com o `URI` por carga nativa
//!   ([`ObjectRef::with_host`]), a forma que o porte usa para um valor vivo que passa pelo `Dynamic`.
//! - **`MultiDiffEditorInput`.** Ainda não foi portado: o `instanceof` é a comparação do `editor_type_id` com o
//!   `MultiDiffEditorInput.ID` ([`MULTI_DIFF_EDITOR_INPUT_ID`]).
//! - **`getItems` e `mostRecentEnd`.** `timeline.items[Math.min(maxCount, length - 1)]` lança `TypeError` no original quando o
//!   índice não é inteiro dentro dos limites (uma timeline vazia com cursor, ou `timeline.pageSize` fracionário); aqui o
//!   índice inválido não contribui para o `mostRecentEnd`, em vez de abortar o `refresh`.
//! - **`TimelinePane.TITLE`.** `localize2('timeline', "Timeline")` é [`timeline_pane_title`] (o texto localizado) e
//!   [`TIMELINE_PANE_TITLE_ORIGINAL`] (o original).
//!
//! O que a parte B deve fornecer (assinaturas):
//! - `impl ITimelineBody for ...`: `render_height`, `set_children`, `clear_children`, `rerender`, `rerender_element`,
//!   `has_element`, `get_selection`, `dom_focus`, `show_message`, `hide_message`, `set_uri`, `is_focus_free_or_inside`.
//!   Ela chama [`TimelinePane::set_body`] ao fim do `renderBody` e [`TimelinePane::set_message`] com a mensagem inicial
//!   ("The active editor cannot provide timeline information.").
//! - No `impl ViewPaneDelegate for TimelinePane` (que a parte A começa com `get_actions_context`): `render_body` e
//!   `layout_body` (`this.tree.layout(height, width)`).
//! - Os métodos que sobrescrevem a `ViewPane` (`focus`, `setExpanded`, `setVisible`, `renderHeaderTitle`), chamando os da
//!   [`TimelinePane::view_pane`] antes ou depois. `setVisible` usa [`TimelinePane::replace_visibility_disposables`] e
//!   [`TimelinePane::dispose_visibility_disposables`], [`TimelinePane::on_active_editor_changed`],
//!   [`TimelinePane::refresh_debounced`] e `services().extension_service.activate_by_event("onView:timeline", None)`;
//!   `setExpanded` usa [`TimelinePane::set_uri_core`], [`TimelinePane::uri`], [`TimelinePane::workspace`] e
//!   [`TimelinePane::follow_active_editor`].
//! - `loadMore(item)`, `compareSelected`, `onContextMenu`, `onDidOpen` e `onDidScrollToEnd`, que usam
//!   [`TimelinePane::load_timeline`], [`TimelinePane::page_size`], [`TimelinePane::page_on_scroll`],
//!   [`TimelinePane::visible_item_count`], [`TimelinePane::set_max_item_count`],
//!   [`TimelinePane::pending_request_count`], [`TimelinePane::ensure_valid_items`], [`TimelinePane::on_selection_changed`]
//!   e [`TimelinePane::show_workspace_timeline`].
//! - `TimelinePaneCommands` (registrado em [`TimelinePane::base`]), `TimelineElementTemplate`, `TimelineActionRunner`,
//!   `TimelineKeyboardNavigationLabelProvider`, `TimelineListVirtualDelegate`, `TimelineTreeRenderer` e os nove casos do
//!   teste. O `TimelineCanCompareSelectionContext` é [`timeline_can_compare_selection_context`].

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

use je_base::cancellation::CancellationTokenSource;
use je_base::collator::{Collator, CollatorOptions, Sensitivity};
use je_base::date::from_now;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy};
use je_base::event::Event;
use je_base::json::{Value, parse_str};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::map::OrderedMap;
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::runtime::{self, TimerId};
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_platform::actions::menu_ids;
use je_platform::commands::commands::ICommandService;
use je_platform::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};
use je_platform::label::label::ILabelService;
use je_platform::progress::progress::{IProgressService, ProgressLocationId, ProgressOptions, ProgressTask};
use je_platform::storage::{StorageScope, StorageService};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_widgets::list::list_types::IIdentityProvider;

use crate::browser::parts::views::view_pane::{IViewPaneOptions, ViewPane, ViewPaneDelegate, ViewPaneServices};
use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::{EditorInputOrUntyped, EditorResourceAccessor, EditorResourceAccessorOptions, SideBySideEditor};
use crate::contrib::timeline::common::timeline::{
    ITimelineService, Timeline, TimelineChangeEvent, TimelineItem, TimelineItemIdentifier, TimelineOptions, TimelineProvidersChangeEvent, TimelineRequest, TimelineTriggerKind,
};
use crate::services::editor::editor_service::IEditorService;
use crate::services::extensions::extensions::IExtensionService;

/// `ItemHeight`.
pub const ITEM_HEIGHT: f64 = 22.0;

/// `MultiDiffEditorInput.ID`.
pub const MULTI_DIFF_EDITOR_INPUT_ID: &str = "workbench.input.multiDiffEditor";

/// A chave do armazenamento (escopo do perfil) com os ids das fontes excluídas.
pub const EXCLUDE_SOURCES_KEY: &str = "timeline.excludeSources";

/// O prefixo do `handle` dos itens que são comandos (e não itens de timeline).
const COMMAND_HANDLE_PREFIX: &str = "vscode-command:";

/// `LoadMoreCommand.handle`.
pub const LOAD_MORE_HANDLE: &str = "vscode-command:loadMore";

/// O texto original de `TimelinePane.TITLE` (`localize2('timeline', "Timeline")`).
pub const TIMELINE_PANE_TITLE_ORIGINAL: &str = "Timeline";

/// O texto localizado de `TimelinePane.TITLE`.
pub fn timeline_pane_title() -> String {
    localize("timeline", TIMELINE_PANE_TITLE_ORIGINAL, &[])
}

/// O `TimelineItem` compartilhado entre o agregado, a árvore e o renderer (ver as divergências do módulo).
pub type TimelineItemRef = Rc<RefCell<TimelineItem>>;

/// `LoadMoreCommand`: o item "Load more" no fim da lista.
pub struct LoadMoreCommand {
    loading: Cell<bool>,
}

impl LoadMoreCommand {
    pub fn new(loading: bool) -> Self {
        Self { loading: Cell::new(loading) }
    }

    /// `get loading()`.
    pub fn loading(&self) -> bool {
        self.loading.get()
    }

    /// `set loading(value)`.
    pub fn set_loading(&self, value: bool) {
        self.loading.set(value);
    }

    /// `get ariaLabel()`.
    pub fn aria_label(&self) -> String {
        self.label()
    }

    /// `get label()`.
    pub fn label(&self) -> String {
        if self.loading() { localize("timeline.loadingMore", "Loading...", &[]) } else { localize("timeline.loadMore", "Load more", &[]) }
    }

    /// `get themeIcon()`.
    pub fn theme_icon(&self) -> Option<ThemeIcon> {
        None
    }
}

/// `TreeElement = TimelineItem | LoadMoreCommand`. A igualdade e o hash são por identidade, como o `===` do original.
#[derive(Clone)]
pub enum TreeElement {
    Item(TimelineItemRef),
    LoadMore(Rc<LoadMoreCommand>),
}

impl TreeElement {
    fn identity(&self) -> usize {
        match self {
            TreeElement::Item(item) => Rc::as_ptr(item) as *const () as usize,
            TreeElement::LoadMore(command) => Rc::as_ptr(command) as *const () as usize,
        }
    }

    /// `element.handle`.
    pub fn handle(&self) -> String {
        match self {
            TreeElement::Item(item) => item.borrow().handle.clone(),
            TreeElement::LoadMore(_) => LOAD_MORE_HANDLE.to_owned(),
        }
    }

    /// O elemento como item de timeline (o `isTimelineItem` que estreita o tipo).
    pub fn timeline_item(&self) -> Option<&TimelineItemRef> {
        match self {
            TreeElement::Item(item) if is_timeline_item_handle(&item.borrow().handle) => Some(item),
            _ => None,
        }
    }

    /// O elemento como comando "Load more" (o `isLoadMoreCommand` que estreita o tipo).
    pub fn load_more_command(&self) -> Option<&Rc<LoadMoreCommand>> {
        match self {
            TreeElement::LoadMore(command) => Some(command),
            TreeElement::Item(_) => None,
        }
    }

    /// `element.label`.
    pub fn label(&self) -> String {
        match self {
            TreeElement::Item(item) => item.borrow().label.clone(),
            TreeElement::LoadMore(command) => command.label(),
        }
    }

    /// `element.description`.
    pub fn description(&self) -> Option<String> {
        match self {
            TreeElement::Item(item) => item.borrow().description.clone(),
            TreeElement::LoadMore(_) => None,
        }
    }

    /// `element.contextValue`.
    pub fn context_value(&self) -> Option<String> {
        match self {
            TreeElement::Item(item) => item.borrow().context_value.clone(),
            TreeElement::LoadMore(_) => None,
        }
    }

    /// `element.source`.
    pub fn source(&self) -> Option<String> {
        match self {
            TreeElement::Item(item) => Some(item.borrow().source.clone()),
            TreeElement::LoadMore(_) => None,
        }
    }

    /// `element.timestamp`.
    pub fn timestamp(&self) -> f64 {
        match self {
            TreeElement::Item(item) => item.borrow().timestamp,
            TreeElement::LoadMore(_) => 0.0,
        }
    }

    /// `element.relativeTime`.
    pub fn relative_time(&self) -> Option<String> {
        match self {
            TreeElement::Item(item) => item.borrow().relative_time.clone(),
            TreeElement::LoadMore(_) => None,
        }
    }

    /// `element.relativeTimeFullWord`.
    pub fn relative_time_full_word(&self) -> Option<String> {
        match self {
            TreeElement::Item(item) => item.borrow().relative_time_full_word.clone(),
            TreeElement::LoadMore(_) => None,
        }
    }

    /// `element.hideRelativeTime`.
    pub fn hide_relative_time(&self) -> Option<bool> {
        match self {
            TreeElement::Item(item) => item.borrow().hide_relative_time,
            TreeElement::LoadMore(_) => None,
        }
    }
}

impl PartialEq for TreeElement {
    fn eq(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }
}

impl Eq for TreeElement {}

impl Hash for TreeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.identity().hash(state);
    }
}

impl std::fmt::Debug for TreeElement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "TreeElement({})", self.handle())
    }
}

fn is_timeline_item_handle(handle: &str) -> bool {
    !handle.starts_with(COMMAND_HANDLE_PREFIX)
}

/// `isLoadMoreCommand(item)`.
pub fn is_load_more_command(item: Option<&TreeElement>) -> bool {
    item.is_some_and(|item| item.load_more_command().is_some())
}

/// `isTimelineItem(item)`.
pub fn is_timeline_item(item: Option<&TreeElement>) -> bool {
    item.is_some_and(|item| is_timeline_item_handle(&item.handle()))
}

/// `updateRelativeTime(item, lastRelativeTime)`.
pub fn update_relative_time(item: &TimelineItemRef, last_relative_time: Option<String>) -> Option<String> {
    let mut item = item.borrow_mut();
    let is_item = is_timeline_item_handle(&item.handle);
    let timestamp = item.timestamp;
    item.relative_time = is_item.then(|| from_now(Some(timestamp), false, false, false));
    item.relative_time_full_word = is_item.then(|| from_now(Some(timestamp), false, true, false));
    if last_relative_time.is_none() || item.relative_time != last_relative_time {
        item.hide_relative_time = Some(false);
        item.relative_time.clone()
    } else {
        item.hide_relative_time = Some(true);
        last_relative_time
    }
}

/// `TimelineActionContext`.
#[derive(Clone)]
pub struct TimelineActionContext {
    /// The file or folder whose timeline is shown, `None` for the workspace timeline.
    pub uri: Option<URI>,
    pub item: TreeElement,
}

/// `getMultiSelection(item, selection)`: the selected timeline items when `item` is part of a multi-selection, otherwise
/// `None`.
pub fn get_multi_selection(item: &TimelineItemRef, selection: &[Option<TreeElement>]) -> Option<Vec<TimelineItemRef>> {
    let items: Vec<TimelineItemRef> = selection.iter().filter_map(|element| element.as_ref().and_then(|element| element.timeline_item()).cloned()).collect();
    let contains_item = items.iter().any(|selected| {
        let (selected, item) = (selected.borrow(), item.borrow());
        selected.source == item.source && selected.handle == item.handle
    });
    (items.len() > 1 && contains_item).then_some(items)
}

/// `TimelineAggregate`.
pub struct TimelineAggregate {
    pub items: RefCell<Vec<TimelineItemRef>>,
    pub source: String,
    pub last_rendered_index: Cell<i64>,
    cursor: RefCell<Option<String>>,
    stale: Cell<bool>,
}

impl TimelineAggregate {
    pub fn new(timeline: Timeline) -> Self {
        Self {
            source: timeline.source,
            items: RefCell::new(timeline.items.into_iter().map(|item| Rc::new(RefCell::new(item))).collect()),
            cursor: RefCell::new(timeline.cursor),
            last_rendered_index: Cell::new(-1),
            stale: Cell::new(false),
        }
    }

    /// `get cursor()`.
    pub fn cursor(&self) -> Option<String> {
        self.cursor.borrow().clone()
    }

    /// `get more()`.
    pub fn more(&self) -> bool {
        self.cursor.borrow().is_some()
    }

    /// `get newest()`.
    pub fn newest(&self) -> Option<TimelineItemRef> {
        self.items.borrow().first().cloned()
    }

    /// `get oldest()`.
    pub fn oldest(&self) -> Option<TimelineItemRef> {
        self.items.borrow().last().cloned()
    }

    /// `add(timeline)`: devolve se a lista mudou.
    pub fn add(&self, timeline: Timeline) -> bool {
        let Timeline { items: new_items, cursor, .. } = timeline;
        let new_items: Vec<TimelineItemRef> = new_items.into_iter().map(|item| Rc::new(RefCell::new(item))).collect();
        let mut updated = false;
        let mut items = self.items.borrow_mut();

        if !new_items.is_empty() && !items.is_empty() {
            updated = true;

            let mut ids: HashSet<String> = HashSet::new();
            let mut timestamps: Vec<f64> = Vec::new();

            for item in &new_items {
                let item = item.borrow();
                match &item.id {
                    None => timestamps.push(item.timestamp),
                    Some(id) => {
                        ids.insert(id.clone());
                    }
                }
            }

            // Remove any duplicate items
            let mut index = items.len();
            while index > 0 {
                index -= 1;
                let is_duplicate = {
                    let item = items[index].borrow();
                    item.id.as_ref().is_some_and(|id| ids.contains(id)) || timestamps.iter().any(|timestamp| *timestamp == item.timestamp || (timestamp.is_nan() && item.timestamp.is_nan()))
                };
                if is_duplicate {
                    items.remove(index);
                }
            }

            let last_timestamp = new_items.last().map_or(0.0, |item| item.borrow().timestamp);
            let newest_timestamp = items.first().map_or(0.0, |item| item.borrow().timestamp);
            if last_timestamp >= newest_timestamp {
                items.splice(0..0, new_items);
            } else {
                items.extend(new_items);
            }
        } else if !new_items.is_empty() {
            updated = true;

            items.extend(new_items);
        }

        *self.cursor.borrow_mut() = cursor;

        if updated {
            let collator = Collator::new(None, CollatorOptions { numeric: true, sensitivity: Sensitivity::Base, ..CollatorOptions::default() });
            items.sort_by(|a, b| {
                let (a, b) = (a.borrow(), b.borrow());
                let difference = b.timestamp - a.timestamp;
                if difference != 0.0 && !difference.is_nan() {
                    return if difference < 0.0 { Ordering::Less } else { Ordering::Greater };
                }
                let (a_source, b_source): (Vec<u16>, Vec<u16>) = (a.source.encode_utf16().collect(), b.source.encode_utf16().collect());
                collator.compare(&b_source, &a_source).cmp(&0)
            });
        }

        updated
    }

    /// `get stale()`.
    pub fn stale(&self) -> bool {
        self.stale.get()
    }

    /// `invalidate()`.
    pub fn invalidate(&self) {
        self.stale.set(true);
    }
}

/// `TimelineIdentityProvider`.
pub struct TimelineIdentityProvider;

impl IIdentityProvider<TreeElement> for TimelineIdentityProvider {
    fn get_id(&self, item: &TreeElement) -> String {
        item.handle()
    }
}

thread_local! {
    static FOLLOW_ACTIVE_EDITOR_CONTEXT: RawContextKey = RawContextKey::new("timelineFollowActiveEditor", true, ContextKeyMeta::Hide);
    static EXCLUDE_SOURCES_CONTEXT: RawContextKey = RawContextKey::new("timelineExcludeSources", "[]", ContextKeyMeta::Hide);
    static VIEW_FOCUSED_CONTEXT: RawContextKey = RawContextKey::new("timelineFocused", true, ContextKeyMeta::None);
    static FOR_WORKSPACE_CONTEXT: RawContextKey = RawContextKey::new(
        "timelineForWorkspace",
        false,
        ContextKeyMeta::Description(localize("timelineForWorkspace", "Whether the Timeline view shows the workspace timeline instead of the timeline of a file or folder.", &[])),
    );
    static CAN_COMPARE_SELECTION_CONTEXT: RawContextKey = RawContextKey::new("timelineCanCompareSelection", false, ContextKeyMeta::None);
}

/// `TimelineFollowActiveEditorContext`.
pub fn timeline_follow_active_editor_context() -> RawContextKey {
    FOLLOW_ACTIVE_EDITOR_CONTEXT.with(Clone::clone)
}

/// `TimelineExcludeSources`.
pub fn timeline_exclude_sources() -> RawContextKey {
    EXCLUDE_SOURCES_CONTEXT.with(Clone::clone)
}

/// `TimelineViewFocusedContext`.
pub fn timeline_view_focused_context() -> RawContextKey {
    VIEW_FOCUSED_CONTEXT.with(Clone::clone)
}

/// `TimelineForWorkspaceContext`.
pub fn timeline_for_workspace_context() -> RawContextKey {
    FOR_WORKSPACE_CONTEXT.with(Clone::clone)
}

/// `TimelineCanCompareSelectionContext`.
pub fn timeline_can_compare_selection_context() -> RawContextKey {
    CAN_COMPARE_SELECTION_CONTEXT.with(Clone::clone)
}

/// `IPendingRequest`: a requisição em andamento e o que a acompanha (a fonte do token e o ouvinte do cancelamento).
struct PendingRequest {
    request: TimelineRequest,
    disposables: Rc<DisposableStore>,
}

impl PendingRequest {
    /// `request.tokenSource.cancel(); dispose()`.
    fn cancel_and_dispose(&self) {
        self.request.token_source.cancel();
        self.dispose_only();
    }

    /// `dispose()` sem cancelar (a requisição já terminou).
    fn dispose_only(&self) {
        self.disposables.dispose();
    }
}

/// `a === b` entre duas requisições: a mesma promessa de resultado.
fn same_request(a: &TimelineRequest, b: &TimelineRequest) -> bool {
    a.result.ptr_eq(&b.result)
}

/// `a === b` entre dois `URI | undefined`: a mesma instância (ou os dois ausentes).
fn same_uri(a: Option<&URI>, b: Option<&URI>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => URI::ptr_eq(a, b),
        _ => false,
    }
}

/// `uri.scheme === Schemas.file || uri.scheme === 'git'`.
fn is_file_or_git(uri: &URI) -> bool {
    let scheme = uri.scheme().to_string_lossy();
    scheme == schemas::FILE || scheme == "git"
}

/// O que o painel lê do armazenamento: o texto de `timeline.excludeSources` e o conjunto que ele descreve
/// (`new Set(JSON.parse(text))`; um JSON inválido ou que não é uma lista de textos é o conjunto vazio).
fn read_excluded_sources(storage: &dyn StorageService) -> (String, HashSet<String>) {
    let raw = storage.get(EXCLUDE_SOURCES_KEY, StorageScope::Profile, Some("[]")).unwrap_or_else(|| "[]".to_owned());
    let mut errors = Vec::new();
    let sources = match parse_str(&raw, &mut errors).filter(|_| errors.is_empty()) {
        Some(Value::Array(values)) => values
            .into_iter()
            .filter_map(|value| match value {
                Value::String(source) => Some(source.to_string_lossy()),
                _ => None,
            })
            .collect(),
        _ => HashSet::new(),
    };
    (raw, sources)
}

/// O corpo do painel que nasce em `renderBody` (parte B): a mensagem (`$message`) e a árvore (`tree`, `treeRenderer`).
pub trait ITimelineBody {
    /// `tree.renderHeight`.
    fn render_height(&self) -> f64;

    /// `tree.setChildren(null, elements)`.
    fn set_children(&self, elements: Vec<TreeElement>);

    /// `tree.setChildren(null, undefined)`.
    fn clear_children(&self);

    /// `tree.rerender()`.
    fn rerender(&self);

    /// `tree.rerender(element)`.
    fn rerender_element(&self, element: &TreeElement);

    /// `tree.hasElement(element)`.
    fn has_element(&self, element: &TreeElement) -> bool;

    /// `tree.getSelection()`.
    fn get_selection(&self) -> Vec<Option<TreeElement>>;

    /// `tree.domFocus()`.
    fn dom_focus(&self);

    /// O `showMessage` do painel: mostra `$message` (sem a classe `hide`) com o texto.
    fn show_message(&self, message: &str);

    /// O `hideMessage` do painel: limpa `$message` e põe a classe `hide`.
    fn hide_message(&self);

    /// `treeRenderer.setUri(uri)`.
    fn set_uri(&self, uri: Option<&URI>);

    /// A condição do `onEditableChanged` para devolver o foco à árvore: não há elemento ativo, o elemento ativo é o
    /// `body` do documento ou ele está dentro do `$container` (`!activeElement || activeElement === activeElement.ownerDocument.body
    /// || DOM.isAncestor(activeElement, this.$container)`).
    fn is_focus_free_or_inside(&self) -> bool;
}

/// Os serviços do construtor do `TimelinePane` (ver as divergências do módulo).
#[derive(Clone)]
pub struct TimelinePaneServices {
    /// Os serviços da `ViewPane` (`keybindingService`, `contextMenuService`, `configurationService`, `contextKeyService`,
    /// `viewDescriptorService`, `openerService`, `hoverService`...).
    pub view_pane: ViewPaneServices,
    pub storage_service: Rc<dyn StorageService>,
    pub editor_service: Rc<dyn IEditorService>,
    pub command_service: Rc<dyn ICommandService>,
    pub progress_service: Rc<dyn IProgressService>,
    pub timeline_service: Rc<dyn ITimelineService>,
    pub label_service: Rc<dyn ILabelService>,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub extension_service: Rc<dyn IExtensionService>,
}

/// `TimelinePane`.
pub struct TimelinePane {
    base: Rc<Disposable>,
    store: Rc<DisposableStore>,
    weak_self: Weak<TimelinePane>,
    view_pane: Rc<ViewPane>,
    services: TimelinePaneServices,

    body: RefCell<Option<Rc<dyn ITimelineBody>>>,
    visibility_disposables: RefCell<Option<Rc<DisposableStore>>>,

    follow_active_editor_context: Rc<dyn IContextKey>,
    timeline_exclude_sources_context: Rc<dyn IContextKey>,
    workspace_context: Rc<dyn IContextKey>,

    excluded_sources: RefCell<HashSet<String>>,
    pending_requests: RefCell<OrderedMap<String, Rc<PendingRequest>>>,
    timelines_by_source: RefCell<OrderedMap<String, Rc<TimelineAggregate>>>,

    /// The file or folder whose timeline is shown. `None` when showing the workspace timeline, or when there is nothing to show.
    uri: RefCell<Option<URI>>,

    /// Whether the workspace timeline is shown.
    workspace: Cell<bool>,

    edited_item: RefCell<Option<TimelineItemIdentifier>>,

    follow_active_editor: Cell<bool>,
    page_on_scroll: Cell<Option<bool>>,

    /// The items whose inline actions were last rendered for a multi-selection.
    multi_selection: RefCell<Vec<TimelineItemRef>>,

    filename: RefCell<Option<String>>,
    message: RefCell<Option<String>>,

    is_empty: Cell<bool>,
    max_item_count: Cell<f64>,
    visible_item_count: Cell<usize>,
    pending_refresh: Cell<bool>,
    refresh_timer: RefCell<Option<TimerId>>,
}

impl TimelinePane {
    pub fn new(options: IViewPaneOptions, services: TimelinePaneServices) -> Rc<Self> {
        let view_pane = ViewPane::new(IViewPaneOptions { title_menu_id: Some(menu_ids::timeline_title()), ..options }, services.view_pane.clone());

        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());

        let context_key_service: Rc<dyn IContextKeyService> = services.view_pane.context_key_service.clone();
        let follow_active_editor_context = timeline_follow_active_editor_context().bind_to(&*context_key_service);
        let timeline_exclude_sources_context = timeline_exclude_sources().bind_to(&*context_key_service);
        let workspace_context = timeline_for_workspace_context().bind_to(&*context_key_service);

        let (excluded_sources_string, excluded_sources) = read_excluded_sources(services.storage_service.as_ref());
        timeline_exclude_sources_context.set(Dynamic::string(&excluded_sources_string));

        let this = Rc::new_cyclic(|weak| Self {
            base,
            store,
            weak_self: weak.clone(),
            view_pane,
            services,
            body: RefCell::new(None),
            visibility_disposables: RefCell::new(None),
            follow_active_editor_context,
            timeline_exclude_sources_context,
            workspace_context,
            excluded_sources: RefCell::new(excluded_sources),
            pending_requests: RefCell::new(OrderedMap::new()),
            timelines_by_source: RefCell::new(OrderedMap::new()),
            uri: RefCell::new(None),
            workspace: Cell::new(false),
            edited_item: RefCell::new(None),
            follow_active_editor: Cell::new(true),
            page_on_scroll: Cell::new(None),
            multi_selection: RefCell::new(Vec::new()),
            filename: RefCell::new(None),
            message: RefCell::new(None),
            is_empty: Cell::new(true),
            max_item_count: Cell::new(0.0),
            visible_item_count: Cell::new(0),
            pending_refresh: Cell::new(false),
            refresh_timer: RefCell::new(None),
        });

        let delegate: Weak<dyn ViewPaneDelegate> = Rc::downgrade(&this);
        this.view_pane.set_delegate(delegate);

        this.register_listener(this.services.storage_service.on_did_change_value(StorageScope::Profile, Some(EXCLUDE_SOURCES_KEY), &this.store), |this, _| {
            this.on_storage_service_changed();
        });
        this.register_listener(this.services.view_pane.configuration_service.on_did_change_configuration(), |this, event| {
            if event.affects_configuration("timeline.pageOnScroll", None) {
                this.page_on_scroll.set(None);
            }
        });
        let timeline_service = this.services.timeline_service.clone();
        this.register_listener(timeline_service.on_did_change_providers(), |this, event| this.on_providers_changed(event));
        this.register_listener(timeline_service.on_did_change_timeline(), |this, event| this.on_timeline_changed(event));
        this.register_listener(timeline_service.on_did_change_uri(), |this, uri| this.set_uri(uri.clone()));
        this.register_listener(timeline_service.on_did_change_editable(), |this, _| this.on_editable_changed());

        this
    }

    /// `this._register(event(handler, this))`: o ouvinte só roda enquanto o painel existe.
    fn register_listener<T: 'static>(&self, event: Event<T>, handler: impl Fn(&TimelinePane, &T) + 'static) {
        let weak = self.weak_self.clone();
        self.base.register(event.listen(move |value| {
            if let Some(this) = weak.upgrade() {
                handler(&this, value);
            }
        }));
    }

    /// A `ViewPane` de que o painel "herda".
    pub fn view_pane(&self) -> &Rc<ViewPane> {
        &self.view_pane
    }

    /// Os serviços do construtor.
    pub fn services(&self) -> &TimelinePaneServices {
        &self.services
    }

    /// O `Disposable` base do painel (o `_register`).
    pub fn base(&self) -> &Rc<Disposable> {
        &self.base
    }

    /// Entrega o corpo que nasce em `renderBody` (ver [`ITimelineBody`]).
    pub fn set_body(&self, body: Rc<dyn ITimelineBody>) {
        *self.body.borrow_mut() = Some(body);
    }

    fn body(&self) -> Option<Rc<dyn ITimelineBody>> {
        self.body.borrow().clone()
    }

    /// `this.visibilityDisposables?.dispose(); this.visibilityDisposables = new DisposableStore()`.
    pub fn replace_visibility_disposables(&self) -> Rc<DisposableStore> {
        self.dispose_visibility_disposables();
        let store = DisposableStore::new();
        *self.visibility_disposables.borrow_mut() = Some(store.clone());
        store
    }

    /// `this.visibilityDisposables?.dispose()`.
    pub fn dispose_visibility_disposables(&self) {
        let previous = self.visibility_disposables.borrow().clone();
        if let Some(previous) = previous {
            previous.dispose();
        }
    }

    // followActiveEditor, pageOnScroll e pageSize

    /// `get followActiveEditor()`.
    pub fn follow_active_editor(&self) -> bool {
        self.follow_active_editor.get()
    }

    /// `set followActiveEditor(value)`.
    pub fn set_follow_active_editor(&self, value: bool) {
        if self.follow_active_editor.get() == value {
            return;
        }

        self.follow_active_editor.set(value);
        self.follow_active_editor_context.set(Dynamic::Bool(value));

        let filename = self.filename.borrow().clone();
        self.update_filename(filename.as_deref());

        if value {
            self.on_active_editor_changed();
        }
    }

    /// `get pageOnScroll()`.
    pub fn page_on_scroll(&self) -> bool {
        if self.page_on_scroll.get().is_none() {
            let value = self.services.view_pane.configuration_service.get_value(Some("timeline.pageOnScroll"), None);
            self.page_on_scroll.set(Some(value.is_truthy()));
        }

        self.page_on_scroll.get().unwrap_or(false)
    }

    /// `get pageSize()`.
    pub fn page_size(&self) -> f64 {
        if let Dynamic::Number(page_size) = self.services.view_pane.configuration_service.get_value(Some("timeline.pageSize"), None) {
            return page_size;
        }

        // If we are paging when scrolling, then add an extra item to the end to make sure the "Load more" item is out of view
        let render_height = self.body().map_or(0.0, |body| body.render_height());
        (render_height / ITEM_HEIGHT + if self.page_on_scroll() { 1.0 } else { -1.0 }).floor().max(20.0)
    }

    /// `reset()`.
    pub fn reset(&self) {
        self.load_timeline(true, None, TimelineTriggerKind::Invoke);
    }

    // O alvo da timeline

    /// The file or folder whose timeline is shown, `None` for the workspace timeline or when there is nothing to show.
    pub fn uri(&self) -> Option<URI> {
        self.uri.borrow().clone()
    }

    /// Whether the workspace timeline is shown.
    pub fn workspace(&self) -> bool {
        self.workspace.get()
    }

    /// `setUri(uri)`: shows the timeline of the given file or folder, or the workspace timeline when `uri` is `None`.
    pub fn set_uri(&self, uri: Option<URI>) {
        let workspace = uri.is_none();
        self.set_uri_core(uri, workspace, true);
    }

    /// `setUriCore(uri, workspace, disableFollowing)`: `workspace` is whether to show the workspace timeline when `uri` is
    /// `None`, otherwise there is nothing to show.
    pub fn set_uri_core(&self, uri: Option<URI>, workspace: bool, disable_following: bool) {
        if disable_following {
            self.set_follow_active_editor(false);
        }

        let is_workspace = uri.is_none() && workspace;
        *self.uri.borrow_mut() = uri.clone();
        self.workspace.set(is_workspace);
        self.workspace_context.set(Dynamic::Bool(is_workspace));
        self.update_filename(self.get_target_label().as_deref());
        if let Some(body) = self.body() {
            body.set_uri(uri.as_ref());
        }

        // Title menu commands are run with the uri
        self.view_pane.update_actions();

        self.load_timeline(true, None, TimelineTriggerKind::Automatic);
    }

    /// `showWorkspaceTimeline()`.
    pub fn show_workspace_timeline(&self) {
        self.set_uri_core(None, true, true);
    }

    fn has_target(&self) -> bool {
        self.uri.borrow().is_some() || self.workspace.get()
    }

    fn get_target_label(&self) -> Option<String> {
        if self.workspace.get() {
            return Some(localize("timeline.workspace", "Workspace", &[]));
        }
        self.uri.borrow().as_ref().map(|uri| self.services.label_service.get_uri_basename_label(uri))
    }

    // Eventos

    fn on_storage_service_changed(&self) {
        let (excluded_sources_string, excluded_sources) = read_excluded_sources(self.services.storage_service.as_ref());
        self.timeline_exclude_sources_context.set(Dynamic::string(&excluded_sources_string));
        *self.excluded_sources.borrow_mut() = excluded_sources;

        let missing: Vec<String> = {
            let excluded_sources = self.excluded_sources.borrow();
            let timelines = self.timelines_by_source.borrow();
            self.services.timeline_service.get_sources().into_iter().filter(|source| !excluded_sources.contains(&source.id) && !timelines.contains_key(&source.id)).map(|source| source.id).collect()
        };
        if !missing.is_empty() {
            self.load_timeline(true, Some(missing), TimelineTriggerKind::Automatic);
        } else {
            self.refresh();
        }
    }

    /// `onActiveEditorChanged()`.
    pub fn on_active_editor_changed(&self) {
        if !self.follow_active_editor() || !self.view_pane.is_expanded() {
            return;
        }

        // An editor of multiple resources, such as a commit opened from the timeline, keeps the current timeline
        let active_editor = self.services.editor_service.active_editor();
        if let Some(EditorInputOrUntyped::Typed(input)) = &active_editor
            && input.editor_type_id() == MULTI_DIFF_EDITOR_INPUT_ID
        {
            return;
        }

        let options = EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Primary), filter_by_scheme: None };
        let mut uri = EditorResourceAccessor::get_original_uri(active_editor.as_ref(), Some(&options));

        // A lista de esquemas excluídos é a do original (settings, webview e walkthrough)
        if uri.as_ref().is_some_and(|uri| {
            let scheme = uri.scheme().to_string_lossy();
            scheme == schemas::VSCODE_SETTINGS || scheme == schemas::WEBVIEW_PANEL || scheme == schemas::WALK_THROUGH
        }) {
            uri = None;
        }

        // Without a resource, show the workspace timeline if there is one
        let workspace = uri.is_none() && self.services.timeline_service.has_workspace_providers();

        let current = self.uri();
        let same_identity = uri.is_some() && self.services.uri_identity_service.ext_uri().is_equal(uri.as_ref(), current.as_ref(), false);
        // Fallback to match on fsPath if we are dealing with files or git schemes
        let same_fs_path = match (&uri, &current) {
            (Some(uri), Some(current)) => {
                let platform = Platform::detect();
                uri.fs_path(platform) == current.fs_path(platform) && is_file_or_git(uri) && is_file_or_git(current)
            }
            _ => false,
        };

        if (uri.is_none() && workspace && self.workspace.get()) || same_identity || same_fs_path {
            // If the uri hasn't changed, make sure we have valid caches
            for source in self.services.timeline_service.get_sources() {
                if self.excluded_sources.borrow().contains(&source.id) {
                    continue;
                }

                let timeline = self.timelines_by_source.borrow().get(&source.id).cloned();
                match timeline {
                    Some(timeline) if !timeline.stale() => continue,
                    Some(timeline) => self.reload_timeline(&timeline),
                    None => {
                        self.load_timeline_for_source(&source.id, uri.as_ref(), true, None, TimelineTriggerKind::Automatic);
                    }
                }
            }

            return;
        }

        self.set_uri_core(uri, workspace, false);
    }

    fn on_providers_changed(&self, event: &TimelineProvidersChangeEvent) {
        if let Some(removed) = &event.removed {
            for source in removed {
                let pending_request = self.pending_requests.borrow_mut().remove(source);
                if let Some(pending_request) = pending_request {
                    pending_request.cancel_and_dispose();
                }
                self.timelines_by_source.borrow_mut().remove(source);
            }

            self.refresh();
        }

        if let Some(added) = &event.added {
            self.load_timeline(true, Some(added.clone()), TimelineTriggerKind::Automatic);
        }

        // When following an editor without a resource, the workspace timeline is shown once there is a provider for it
        if self.follow_active_editor() && self.uri.borrow().is_none() && self.workspace.get() != self.services.timeline_service.has_workspace_providers() {
            self.on_active_editor_changed();
        }
    }

    fn on_timeline_changed(&self, event: &TimelineChangeEvent) {
        let affected = if event.workspace {
            self.workspace.get()
        } else {
            let current = self.uri();
            current.is_some() && event.uris.as_ref().is_none_or(|uris| uris.iter().any(|uri| self.services.uri_identity_service.ext_uri().is_equal(Some(uri), current.as_ref(), false)))
        };
        if !affected {
            return;
        }

        let timeline = self.timelines_by_source.borrow().get(&event.id).cloned();
        let Some(timeline) = timeline else { return };

        if self.is_body_visible() {
            self.reload_timeline(&timeline);
        } else {
            timeline.invalidate();
        }
    }

    fn on_editable_changed(&self) {
        let previous = self.edited_item.borrow().clone();
        let current = self.services.timeline_service.editable().map(|editable| editable.item);
        *self.edited_item.borrow_mut() = current.clone();

        if !self.is_body_visible() {
            return;
        }

        if current.is_none() && self.pending_refresh.get() {
            // Apply a refresh that was deferred while editing
            self.refresh();
        } else {
            self.rerender_item(previous.as_ref());
            self.rerender_item(current.as_ref());
        }

        // Restore focus when editing ended from the input box (e.g. Enter or Escape), but not when focus moved elsewhere
        if previous.is_some()
            && current.is_none()
            && let Some(body) = self.body()
            && body.is_focus_free_or_inside()
        {
            body.dom_focus();
        }
    }

    /// `onSelectionChanged()`.
    pub fn on_selection_changed(&self) {
        if !self.ensure_valid_items() {
            self.multi_selection.borrow_mut().clear();
            return;
        }
        let Some(body) = self.body() else { return };

        // Inline actions depend on whether their item is part of a multi-selection
        let selection: Vec<TimelineItemRef> = body.get_selection().iter().filter_map(|element| element.as_ref().and_then(|element| element.timeline_item()).cloned()).collect();
        let multi_selection = if selection.len() > 1 { selection } else { Vec::new() };
        let previous = std::mem::replace(&mut *self.multi_selection.borrow_mut(), multi_selection.clone());

        let mut changed: Vec<TimelineItemRef> = Vec::new();
        for item in previous.into_iter().chain(multi_selection) {
            if !changed.iter().any(|known| Rc::ptr_eq(known, &item)) {
                changed.push(item);
            }
        }

        for item in changed {
            let element = TreeElement::Item(item.clone());
            if body.has_element(&element) && !self.is_edited(&item) {
                body.rerender_element(&element);
            }
        }
    }

    fn is_edited(&self, item: &TimelineItemRef) -> bool {
        let item = item.borrow();
        self.edited_item.borrow().as_ref().is_some_and(|edited| edited.source == item.source && edited.handle == item.handle)
    }

    fn rerender_item(&self, identifier: Option<&TimelineItemIdentifier>) {
        let Some(identifier) = identifier else { return };

        let timeline = self.timelines_by_source.borrow().get(&identifier.source).cloned();
        let item = timeline.and_then(|timeline| {
            let items = timeline.items.borrow();
            let found = items.iter().find(|item| item.borrow().handle == identifier.handle).cloned();
            found
        });
        if let Some(item) = item
            && let Some(body) = self.body()
        {
            let element = TreeElement::Item(item);
            if body.has_element(&element) {
                body.rerender_element(&element);
            }
        }
    }

    // Título e mensagem

    /// `updateFilename(filename)`.
    pub fn update_filename(&self, filename: Option<&str>) {
        *self.filename.borrow_mut() = filename.map(str::to_owned);
        if self.follow_active_editor() || filename.is_none_or(str::is_empty) {
            self.view_pane.update_title_description(filename);
        } else {
            let pinned = format!("{} (pinned)", filename.unwrap_or_default());
            self.view_pane.update_title_description(Some(&pinned));
        }
    }

    /// `get message()`.
    pub fn message(&self) -> Option<String> {
        self.message.borrow().clone()
    }

    /// `set message(message)`.
    pub fn set_message(&self, message: Option<String>) {
        *self.message.borrow_mut() = message;
        self.update_message();
    }

    fn update_message(&self) {
        let Some(body) = self.body() else { return };
        match self.message() {
            Some(message) => body.show_message(&message),
            None => body.hide_message(),
        }
    }

    /// `setLoadingUriMessage()`.
    pub fn set_loading_uri_message(&self) {
        let label = self.get_target_label();
        self.update_filename(label.as_deref());
        if self.workspace.get() {
            self.set_message(Some(localize("timeline.loadingWorkspace", "Loading workspace timeline...", &[])));
        } else {
            let message = match label.as_deref().filter(|label| !label.is_empty()) {
                Some(label) => localize("timeline.loading", "Loading timeline for {0}...", &[label]),
                None => String::new(),
            };
            self.set_message(Some(message));
        }
    }

    // Estado de paginação

    fn has_visible_items(&self) -> bool {
        self.visible_item_count.get() > 0
    }

    /// `_visibleItemCount`.
    pub fn visible_item_count(&self) -> usize {
        self.visible_item_count.get()
    }

    /// `this._maxItemCount = value`.
    pub fn set_max_item_count(&self, value: f64) {
        self.max_item_count.set(value);
    }

    /// `this.pendingRequests.size`.
    pub fn pending_request_count(&self) -> usize {
        self.pending_requests.borrow().len()
    }

    fn is_body_visible(&self) -> bool {
        self.view_pane.is_body_visible()
    }

    fn clear(&self, cancel_pending: bool) {
        self.visible_item_count.set(0);
        self.max_item_count.set(self.page_size());
        self.timelines_by_source.borrow_mut().clear();

        if cancel_pending {
            // O cancelamento apaga a requisição de `pendingRequests`, então a iteração é sobre uma cópia
            let pending_requests: Vec<Rc<PendingRequest>> = self.pending_requests.borrow().values().cloned().collect();
            for pending_request in pending_requests {
                pending_request.cancel_and_dispose();
            }

            self.pending_requests.borrow_mut().clear();

            if !self.is_body_visible()
                && let Some(body) = self.body()
            {
                body.clear_children();
                self.is_empty.set(true);
            }
        }
    }

    // Carga

    /// `loadTimeline(reset, sources?, triggerKind = Automatic)`.
    pub fn load_timeline(&self, reset: bool, sources: Option<Vec<String>>, trigger_kind: TimelineTriggerKind) {
        // If we have no source, we are resetting all sources, so cancel everything in flight and reset caches
        if sources.is_none() {
            if reset {
                self.clear(true);
            }

            if self.is_empty.get() && self.has_target() {
                self.set_loading_uri_message();
            }
        }

        if !self.has_target() {
            self.clear(false);
            self.refresh();

            return;
        }

        if !self.is_body_visible() {
            return;
        }

        let mut has_pending_requests = false;

        let sources = sources.unwrap_or_else(|| self.services.timeline_service.get_sources().into_iter().map(|source| source.id).collect());
        let uri = self.uri();
        for source in sources {
            let requested = self.load_timeline_for_source(&source, uri.as_ref(), reset, None, trigger_kind);
            if requested {
                has_pending_requests = true;
            }
        }

        if !has_pending_requests {
            self.refresh();
        } else if self.is_empty.get() {
            self.set_loading_uri_message();
        }
    }

    /// `loadTimelineForSource(source, uri, reset, options?, triggerKind = Automatic)`: `uri` is the file or folder to load the
    /// timeline for, `None` for the workspace timeline.
    fn load_timeline_for_source(&self, source: &str, uri: Option<&URI>, reset: bool, options: Option<TimelineOptions>, trigger_kind: TimelineTriggerKind) -> bool {
        if self.excluded_sources.borrow().contains(source) {
            return false;
        }

        let timeline = self.timelines_by_source.borrow().get(&source.to_owned()).cloned();
        let page_size = self.page_size();

        // If we are paging, and there are no more items or we have enough cached items to cover the next page,
        // don't bother querying for more
        if !reset
            && options.as_ref().is_some_and(|options| options.cursor.is_some())
            && timeline.as_ref().is_some_and(|timeline| !timeline.more() || timeline.items.borrow().len() as f64 > timeline.last_rendered_index.get() as f64 + page_size)
        {
            return false;
        }

        let mut options = match options {
            Some(options) => options,
            None => {
                if !reset && timeline.as_ref().is_some_and(|timeline| !timeline.items.borrow().is_empty() && !timeline.more()) {
                    // If we are not resetting, have item(s), and already know there are no more to fetch, we're done here
                    return false;
                }
                let mut options = TimelineOptions::new(trigger_kind);
                options.cursor = if reset { None } else { timeline.as_ref().and_then(|timeline| timeline.cursor()) };
                options.limit = Some(page_size);
                options
            }
        };

        let pending_request = self.pending_requests.borrow().get(&source.to_owned()).cloned();
        if let Some(pending_request) = pending_request {
            options.cursor = pending_request.request.options.cursor.clone();

            // Requisições concorrentes somam os limites (o tratamento é o do original)
            options.limit = match (options.limit, pending_request.request.options.limit) {
                (Some(limit), Some(pending_limit)) => Some(limit + pending_limit),
                _ => None,
            };
            pending_request.cancel_and_dispose();
        }

        options.cache_results = Some(true);
        options.reset_cache = Some(reset);
        let token_source = CancellationTokenSource::new(None);
        let Some(new_request) = self.services.timeline_service.get_timeline(source, uri, &options, token_source.clone()) else {
            token_source.dispose();
            return false;
        };

        let disposables = DisposableStore::new();
        self.pending_requests.borrow_mut().insert(source.to_owned(), Rc::new(PendingRequest { request: new_request.clone(), disposables: disposables.clone() }));
        let disposable_source = token_source.clone();
        disposables.add(to_disposable(move || disposable_source.dispose()));
        let (weak, source_id, request) = (self.weak_self.clone(), source.to_owned(), new_request.clone());
        disposables.add(token_source.token().on_cancellation_requested().listen(move |_| {
            let Some(this) = weak.upgrade() else { return };
            let is_current = this.pending_requests.borrow().get(&source_id).is_some_and(|pending_request| same_request(&pending_request.request, &request));
            if is_current {
                this.pending_requests.borrow_mut().remove(&source_id);
            }
        }));

        self.handle_request(new_request);

        true
    }

    fn reload_timeline(&self, timeline: &Rc<TimelineAggregate>) {
        self.timelines_by_source.borrow_mut().remove(&timeline.source);
        // Re-query for at least all our existing cached (possibly visible) items to keep visual continuity
        let mut options = TimelineOptions::new(TimelineTriggerKind::Automatic);
        options.limit = Some((timeline.items.borrow().len() as f64).max(self.page_size()));
        let uri = self.uri();
        self.load_timeline_for_source(&timeline.source, uri.as_ref(), true, Some(options), TimelineTriggerKind::Automatic);
    }

    fn handle_request(&self, request: TimelineRequest) {
        // O `withProgress` do porte devolve `Promise<()>`: a resposta sai da tarefa por esta célula
        let response: Rc<RefCell<Option<Timeline>>> = Rc::new(RefCell::new(None));
        let captured = response.clone();
        let result = request.result.clone();
        let task: ProgressTask = Box::new(move |_progress| {
            result.then_result(move |outcome| {
                // Ignore
                if let Ok(timeline) = outcome {
                    *captured.borrow_mut() = timeline;
                }
                Ok(())
            })
        });
        let progress = self.services.progress_service.with_progress(ProgressOptions::new(ProgressLocationId::View(self.view_pane.id.clone())), task, None);

        let weak = self.weak_self.clone();
        progress.then_result(move |_| {
            if let Some(this) = weak.upgrade() {
                let response = response.borrow_mut().take();
                this.on_request_settled(&request, response);
            }
            Ok(())
        });
    }

    /// O que `handleRequest` faz depois do `await`.
    fn on_request_settled(&self, request: &TimelineRequest, response: Option<Timeline>) {
        // Ignore cancelled or replaced requests, including results from removed providers.
        let pending_request = self.pending_requests.borrow().get(&request.source).cloned();
        let Some(pending_request) = pending_request.filter(|pending_request| same_request(&pending_request.request, request)) else {
            return;
        };
        self.pending_requests.borrow_mut().remove(&request.source);
        pending_request.dispose_only();

        let uri_unchanged = same_uri(request.uri.as_ref(), self.uri.borrow().as_ref());
        let Some(response) = response.filter(|_| uri_unchanged) else {
            if self.pending_request_count() == 0 && self.pending_refresh.get() {
                self.refresh();
            }
            return;
        };

        let source = request.source.clone();

        let existing = self.timelines_by_source.borrow().get(&source).cloned();
        let updated = match existing {
            None => {
                self.timelines_by_source.borrow_mut().insert(source, Rc::new(TimelineAggregate::new(response)));
                true
            }
            Some(timeline) => timeline.add(response),
        };

        if updated {
            self.pending_refresh.set(true);

            // If we have visible items already and there are other pending requests, debounce for a bit to wait for other requests
            if self.has_visible_items() && self.pending_request_count() != 0 {
                self.refresh_debounced();
            } else {
                self.refresh();
            }
        } else if self.pending_request_count() == 0 {
            if self.pending_refresh.get() {
                self.refresh();
            } else if let Some(body) = self.body() {
                body.rerender();
            }
        }
    }

    /// `*getItems()`: os elementos da árvore, com o estado de paginação atualizado.
    fn get_items(&self) -> Vec<TreeElement> {
        let mut elements = Vec::new();
        let mut more = false;

        let timelines = self.timelines_by_source.borrow();
        if !self.has_target() || timelines.is_empty() {
            self.visible_item_count.set(0);

            return elements;
        }

        let max_count = self.max_item_count.get();
        let mut count: usize = 0;
        let excluded_sources = self.excluded_sources.borrow();

        if timelines.len() == 1 {
            let (source, timeline) = timelines.iter().next().expect("há uma timeline");

            timeline.last_rendered_index.set(-1);

            if excluded_sources.contains(source) {
                self.visible_item_count.set(0);

                return elements;
            }

            let items = timeline.items.borrow();
            if !items.is_empty() {
                // If we have any items, just say we have one for now, the real count will be updated below
                self.visible_item_count.set(1);
            }

            more = timeline.more();

            let mut last_relative_time: Option<String> = None;
            for item in items.iter() {
                {
                    let mut item = item.borrow_mut();
                    item.relative_time = None;
                    item.hide_relative_time = None;
                }

                count += 1;
                if count as f64 > max_count {
                    more = true;
                    break;
                }

                last_relative_time = update_relative_time(item, last_relative_time);
                elements.push(TreeElement::Item(item.clone()));
            }

            timeline.last_rendered_index.set(count as i64 - 1);
        } else {
            /// `{ timeline, iterator, nextItem }`: o iterador é a posição na cópia dos itens.
            struct SourceCursor {
                timeline: Rc<TimelineAggregate>,
                items: Vec<TimelineItemRef>,
                position: usize,
            }

            let mut sources: Vec<SourceCursor> = Vec::new();

            let mut has_any_items = false;
            let mut most_recent_end = 0.0_f64;

            for (source, timeline) in timelines.iter() {
                timeline.last_rendered_index.set(-1);

                if excluded_sources.contains(source) || timeline.stale() {
                    continue;
                }

                let items = timeline.items.borrow().clone();
                if !items.is_empty() {
                    has_any_items = true;
                }

                if timeline.more() {
                    more = true;

                    // `timeline.items[Math.min(maxCount, timeline.items.length - 1)]`
                    let index = max_count.min(items.len() as f64 - 1.0);
                    if index >= 0.0
                        && index.fract() == 0.0
                        && let Some(last) = items.get(index as usize)
                    {
                        let timestamp = last.borrow().timestamp;
                        if timestamp > most_recent_end {
                            most_recent_end = timestamp;
                        }
                    }
                }

                sources.push(SourceCursor { timeline: timeline.clone(), items, position: 0 });
            }

            self.visible_item_count.set(usize::from(has_any_items));

            let mut last_relative_time: Option<String> = None;
            loop {
                // getNextMostRecentSource: entre os empates, vale o último
                let mut next_source: Option<usize> = None;
                for (index, source) in sources.iter().enumerate() {
                    let Some(current) = source.items.get(source.position) else { continue };
                    let is_newer = next_source.is_none_or(|previous| {
                        let previous = &sources[previous];
                        current.borrow().timestamp >= previous.items[previous.position].borrow().timestamp
                    });
                    if is_newer {
                        next_source = Some(index);
                    }
                }
                let Some(next_source) = next_source else { break };

                let source = &mut sources[next_source];
                source.timeline.last_rendered_index.set(source.timeline.last_rendered_index.get() + 1);

                let item = source.items[source.position].clone();
                {
                    let mut item = item.borrow_mut();
                    item.relative_time = None;
                    item.hide_relative_time = None;
                }

                let timestamp = item.borrow().timestamp;
                if timestamp >= most_recent_end {
                    count += 1;
                    if count as f64 > max_count {
                        more = true;
                        break;
                    }

                    last_relative_time = update_relative_time(&item, last_relative_time);
                    elements.push(TreeElement::Item(item));
                }

                source.position += 1;
            }
        }

        self.visible_item_count.set(count);
        if count > 0 {
            let pending = self.pending_request_count() != 0;
            if more {
                elements.push(TreeElement::LoadMore(Rc::new(LoadMoreCommand::new(pending))));
            } else if pending {
                elements.push(TreeElement::LoadMore(Rc::new(LoadMoreCommand::new(true))));
            }
        }

        elements
    }

    /// `refresh()`.
    pub fn refresh(&self) {
        if !self.is_body_visible() {
            return;
        }

        // Refreshing re-renders all items, which would recreate the input box of an item being edited
        if self.services.timeline_service.editable().is_some() {
            self.pending_refresh.set(true);
            return;
        }

        let elements = self.get_items();
        if let Some(body) = self.body() {
            body.set_children(elements);
        }
        self.is_empty.set(!self.has_visible_items());

        if !self.has_target() {
            self.update_filename(None);
            self.set_message(Some(localize("timeline.editorCannotProvideTimeline", "The active editor cannot provide timeline information.", &[])));
        } else if self.is_empty.get() {
            if self.pending_request_count() != 0 {
                self.set_loading_uri_message();
            } else {
                self.update_filename(self.get_target_label().as_deref());
                let scm_provider_count = self.services.view_pane.context_key_service.get_context_key_value("scm.providerCount");
                let sources = self.services.timeline_service.get_sources();
                let (all_excluded, local_history_excluded, any_excluded) = {
                    let excluded_sources = self.excluded_sources.borrow();
                    (
                        !sources.is_empty() && sources.iter().all(|source| excluded_sources.contains(&source.id)),
                        excluded_sources.contains("timeline.localHistory"),
                        sources.iter().any(|source| excluded_sources.contains(&source.id)),
                    )
                };
                let message = if all_excluded {
                    localize("timeline.noTimelineSourcesEnabled", "All timeline sources have been filtered out.", &[])
                } else if !self.workspace.get() && self.services.view_pane.configuration_service.get_value(Some("workbench.localHistory.enabled"), None).is_truthy() && !local_history_excluded {
                    localize("timeline.noLocalHistoryYet", "Local History will track recent changes as you save them unless the file has been excluded or is too large.", &[])
                } else if any_excluded {
                    localize("timeline.noTimelineInfoFromEnabledSources", "No filtered timeline information was provided.", &[])
                } else {
                    localize("timeline.noTimelineInfo", "No timeline information was provided.", &[])
                };
                self.set_message(Some(message));
                if !scm_provider_count.is_truthy() {
                    let current = self.message().unwrap_or_default();
                    self.set_message(Some(format!("{current} {}", localize("timeline.noSCM", "Source Control has not been configured.", &[]))));
                }
            }
        } else {
            self.update_filename(self.get_target_label().as_deref());
            self.set_message(None);
        }

        self.pending_refresh.set(false);
    }

    /// `@debounce(500) refreshDebounced()`.
    pub fn refresh_debounced(&self) {
        let previous = self.refresh_timer.borrow_mut().take();
        if let Some(previous) = previous {
            runtime::clear_timeout(previous);
        }

        let weak = self.weak_self.clone();
        let timer = runtime::set_timeout(
            move || {
                if let Some(this) = weak.upgrade() {
                    this.refresh_timer.borrow_mut().take();
                    this.refresh();
                }
            },
            500,
        );
        *self.refresh_timer.borrow_mut() = Some(timer);
    }

    /// `ensureValidItems()`.
    pub fn ensure_valid_items(&self) -> bool {
        // If we don't have any non-excluded timelines, clear the tree and show the loading message
        let has_valid_timeline = {
            let excluded_sources = self.excluded_sources.borrow();
            let timelines = self.timelines_by_source.borrow();
            self.services.timeline_service.get_sources().iter().any(|source| !excluded_sources.contains(&source.id) && timelines.contains_key(&source.id))
        };
        if !self.has_visible_items() || !has_valid_timeline {
            if let Some(body) = self.body() {
                body.clear_children();
            }
            self.is_empty.set(true);

            self.set_loading_uri_message();

            return false;
        }

        true
    }
}

impl ViewPaneDelegate for TimelinePane {
    fn get_actions_context(&self) -> Option<Dynamic> {
        self.uri.borrow().as_ref().map(|uri| Dynamic::Object(ObjectRef::with_host(Rc::new(uri.clone()))))
    }
}

impl IDisposable for TimelinePane {
    fn dispose(&self) {
        let timer = self.refresh_timer.borrow_mut().take();
        if let Some(timer) = timer {
            runtime::clear_timeout(timer);
        }
        self.dispose_visibility_disposables();
        self.base.dispose();
        self.view_pane.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "TimelinePane"
    }
}
