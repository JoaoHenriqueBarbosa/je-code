//! Porte de `src/vs/workbench/contrib/timeline/browser/timelinePane.ts`: os tipos auxiliares (`TreeElement`,
//! `LoadMoreCommand`, `TimelineAggregate`, `TimelineIdentityProvider`, `TimelineActionContext`, `getMultiSelection`,
//! `updateRelativeTime`, as chaves de contexto), a classe [`TimelinePane`] (campos, construtor, `followActiveEditor`,
//! `pageOnScroll`, `pageSize`, `setUri`, `onActiveEditorChanged`, `onProvidersChanged`, `onTimelineChanged`,
//! `onEditableChanged`, `onSelectionChanged`, `loadTimeline`, `loadTimelineForSource`, `reloadTimeline`, `handleRequest`,
//! `getItems`, `refresh`, `refreshDebounced`, as mensagens de vazio e o estado de paginação, `renderBody`, `layoutBody`,
//! `renderHeaderTitle`, `focus`, `setExpanded`, `setVisible`, `loadMore`, `ensureValidItems`, `compareSelected`,
//! `onContextMenu`), o corpo da árvore ([`TimelineBody`]) com o [`TimelineTreeRenderer`], o
//! [`TimelineKeyboardNavigationLabelProvider`], o [`TimelineListVirtualDelegate`] e o [`TimelineActionRunner`], e os comandos
//! do painel ([`TimelinePaneCommands`]).
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
//! - **Árvore e mensagem.** `$message`, `$tree`, `tree` e `treeRenderer` nascem em `renderBody`. A lógica de carga fala com eles
//!   pelo trait [`ITimelineBody`], que o [`TimelineBody`] implementa sobre a `WorkbenchObjectTree<TreeElement, FuzzyScore>` e
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
//! - **`getItems` e `mostRecentEnd`.** `timeline.items[Math.min(maxCount, length - 1)]` lança `TypeError` no original quando o
//!   índice não é inteiro dentro dos limites (uma timeline vazia com cursor, ou `timeline.pageSize` fracionário); aqui o
//!   índice inválido não contribui para o `mostRecentEnd`, em vez de abortar o `refresh`.
//! - **`TimelinePane.TITLE`.** `localize2('timeline', "Timeline")` é [`timeline_pane_title`] (o texto localizado) e
//!   [`TIMELINE_PANE_TITLE_ORIGINAL`] (o original).
//!
//! - **Corpo.** `renderBody` cria a mensagem, a árvore (`WorkbenchObjectTree<TreeElement, FuzzyScore>`) e o renderer, e entrega
//!   tudo à parte A pelo [`TimelineBody`] (que implementa [`ITimelineBody`]). As peças que o original cria com
//!   `createInstance` (a árvore, o renderer, o `WorkbenchHoverDelegate`, o `InlineEditInputBox`, o `TimelinePaneCommands`) entram
//!   por [`TimelineBodyServices`] e [`TimelinePaneServices`], como o `ViewPaneServices.create_toolbar` da base.
//! - **Comandos.** O `TimelinePaneCommands` guarda o painel por [`ITimelinePaneCommandsHost`] (só o que as ações chamam: o `id`,
//!   `reset`, `showWorkspaceTimeline`, `compareSelected` e a troca de `followActiveEditor`), um trait porque o teste do original
//!   o instancia com `upcastPartial<TimelinePane>({ id })`; o painel o guarda por referência fraca (o ciclo do original é do
//!   coletor de lixo). `reset` e `showWorkspaceTimeline` moram no impl desse trait, não em métodos próprios.
//! - **Acesso do teste.** O teste do original lê membros `private` (`clear`, `loadTimelineForSource`, `onProvidersChanged`,
//!   `onEditableChanged`, `onTimelineChanged`, `pendingRequests`, `timelinesBySource`, `excludedSources`): aqui os métodos são
//!   `pub` e os mapas saem por [`TimelinePane::pending_request`], [`TimelinePane::timeline_for_source`] e
//!   [`TimelinePane::set_excluded_sources`].
//! - **Acessibilidade e `getActionViewItem`.** A árvore do `je-widgets` não tem `accessibilityProvider` nem as opções ARIA, então
//!   o [`TimelineAccessibilityProvider`] guarda os três textos e papéis do original sem ligação. O menu de contexto do porte não
//!   aceita o `getActionViewItem` (o rótulo do atalho de cada ação), que fica por conta de quem desenha o menu.
//! - **Ícones e tooltip.** O `je-ui` não tem conteúdo de imagem: um item com `icon`/`iconDark` (URI) deixa o ícone sem desenho
//!   (a classe do ícone fica), e sem tipo de tema (`isDark(theme.type)`) a escolha entre os dois não existe. O `themeIcon` e a cor
//!   dele saem do `IThemeService`. O tooltip em markdown vai com o texto simples como alternativa (`renderAsPlaintext`).
//! - **Linha.** O `.timeline-timestamp-container` e o `.actions` ficam dentro do `.monaco-icon-label` no original (um `flex`); aqui
//!   são irmãos do rótulo e o [`TimelineTreeRenderer`] reparte a largura da linha entre os quatro (`layout_row`). As ações
//!   aparecem com o ponteiro sobre a linha (as classes `selected` e `focused` da linha não chegam ao template), e o traço de
//!   `.timeline-timestamp--duplicate::before` não é desenhado (o carimbo repetido só guarda os 10px).
//! - **Mensagem.** O `$message` quebra o texto em linhas (`position: absolute` com `padding: 10px 22px 0 22px`): o `je-ui` não
//!   quebra linha de texto, então [`TimelineBody`] mede e reparte as palavras a cada layout.
//! - **`renderHeaderTitle`.** Chega pelo gancho [`ViewPaneDelegate::render_header_title`], chamado pela `ViewPane` depois do
//!   `super.renderHeaderTitle`.
//! - **`onDidOpen` e `API_OPEN_EDITOR_COMMAND_ID`.** Os dois ids (de `editorCommands.ts`, ainda não portado) são
//!   [`API_OPEN_EDITOR_COMMAND_ID`] e [`API_OPEN_DIFF_EDITOR_COMMAND_ID`]; o `IOpenEvent` que vai nos argumentos é a carga nativa de
//!   um objeto ([`ObjectRef::with_host`]).
//! - **Contexto das ações.** O `TimelineActionContext` (`{ uri, item }`) viaja pelo `Dynamic` como carga nativa
//!   ([`timeline_action_context_to_dynamic`]); o contexto que as extensões recebem é o objeto `{ $mid, handle, source, uri }`.
//! - **Filtros por fonte.** O `excluded` do original é um `Set` que guarda a ordem de inserção; aqui é uma lista sem repetição.
//! - **`MultiDiffEditorInput`.** Ainda não foi portado: o `instanceof` é a comparação do `editor_type_id` com o
//!   `MultiDiffEditorInput.ID` ([`MULTI_DIFF_EDITOR_INPUT_ID`], definido no arquivo do input).

use je_platform::resources::host_platform;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, ActionRunner, IActionRunner, RunAction, SubmenuAction};
use je_base::async_::async_fn;
use je_base::cancellation::CancellationTokenSource;
use je_base::collator::{Collator, CollatorOptions, Sensitivity};
use je_base::date::from_now;
use je_base::dynamic::{Dynamic, ObjectRef, Truthy, json_stringify};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::filters::{FuzzyScore, create_matches};
use je_base::json::{Value, parse_str};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::map::OrderedMap;
use je_base::markdown_plaintext::render_as_plaintext;
use je_base::marshalling_ids::MarshalledId;
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::parts::contextmenu::IContextMenuEvent;
use je_base::promise::Promise;
use je_base::runtime::{self, TimerId};
use je_base::strings::escape_reg_exp_characters;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;
use je_editor::services::theme_service::IThemeService;
use je_instantiation::ServicesAccessor;
use je_platform::accessibility::IAccessibilityInformation;
use je_platform::action::{Category, CommandAction, CommandActionTitle, Icon, Title, Toggled, localize2};
use je_platform::actions::menu_entry_action_view_item::{PrimaryAndSecondaryActions, get_context_menu_actions};
use je_platform::actions::menu_ids;
use je_platform::actions::{Action2, Action2MenuItem, Action2Options, AnyMenuItem, IMenuService, MenuActionOptions, MenuId, MenuItem, menu_registry, register_action2};
use je_platform::commands::commands::{CommandHandler, ICommandService, commands_registry};
use je_platform::contextkey::contextkey::ContextRegExp;
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression, ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};
use je_platform::contextview::context_view::{AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuMenuDelegate, IContextMenuService};
use je_platform::contextview::view_element::ViewElement;
use je_platform::label::label::ILabelService;
use je_platform::list::list_service::{workbench_list_double_selection, workbench_list_multi_selection};
use je_platform::progress::progress::{IProgressService, ProgressLocationId, ProgressOptions, ProgressTask};
use je_platform::storage::{I_STORAGE_SERVICE, StorageScope, StorageService, StorageTarget};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_render::kurbo::Rect;
use je_storage::StorageValue;
use je_theme::to_rgba8;
use je_ui::{Content, EventKind, NodeId, PointerEvents, Role, Ui};
use je_widgets::action_bar::{ActionBar, IActionBarOptions, IActionOptions, IActionViewItemProvider};
use je_widgets::action_view_items::IActionViewItemOptions;
use je_widgets::context_view_api::IContextViewProvider;
use je_widgets::dom::{create_el, hide, is_ancestor_of_active_element, show};
use je_widgets::hover_api::{HoverContent, HoverPlacement, IHoverDelegate, ManagedHoverContent};
use je_widgets::icon_label::{IIconLabelCreationOptions, IIconLabelValueOptions, IconLabel, IconLabelStyles, IconLabelText, IconLabelTitle};
use je_widgets::input_box::InputBoxStyles;
use je_widgets::list::list_styles::IListStyles;
use je_widgets::list::list_types::{IIdentityProvider, IKeyboardNavigationLabelProvider, IListVirtualDelegate, TemplateData};
use je_widgets::text::{measure_text, ui_font};
use je_widgets::tree::abstract_tree::AbstractTreeOptions;
use je_widgets::tree::object_tree::{ObjectTreeMethods, ObjectTreeOptions};
use je_widgets::tree::object_tree_model::{ObjectTreeModel, ObjectTreeModelSetChildrenOptions};
use je_widgets::tree::tree_types::{IObjectTreeElement, ITreeModel, ITreeRenderer, TreeElementRenderDetails, TreeMouseEvent, TreeNodeApi};

use crate::api::common::ext_host_values::ApiValue;
use crate::browser::parts::views::inline_edit_input_box::{InlineEditInputBox, InlineEditInputBoxOptions};
use crate::browser::parts::views::view_pane::{IViewPaneOptions, ListOverrideStyleColors, ViewPane, ViewPaneDelegate, ViewPaneServices};
use crate::browser::resource_navigator::IOpenEvent;
use crate::browser::workbench_tree::{IWorkbenchTreeOptions, WorkbenchObjectTree, WorkbenchTreeServices};
use crate::common::editor::editor_input::EditorInput;
use crate::common::editor::{EditorInputOrUntyped, EditorResourceAccessor, EditorResourceAccessorOptions, SideBySideEditor, UntypedEditorInput};
use crate::common::views::ViewContainerLocation;
use crate::contrib::multi_diff_editor::browser::multi_diff_editor_input::MULTI_DIFF_EDITOR_INPUT_ID;
use crate::contrib::timeline::common::timeline::{
    ITimelineService, Timeline, TimelineChangeEvent, TimelineItem, TimelineItemIdentifier, TimelineOptions, TimelineProvidersChangeEvent, TimelineRequest, TimelineSource,
    TimelineTriggerKind,
};
use crate::contrib::timeline::common::timeline_service::timeline_has_workspace_provider_context;
use crate::services::editor::editor_service::IEditorService;
use crate::services::extensions::extensions::IExtensionService;
use crate::services::views::views_service::IViewPane;

/// `ItemHeight`.
pub const ITEM_HEIGHT: f64 = 22.0;

/// `API_OPEN_EDITOR_COMMAND_ID` (`editorCommands.ts`, ainda não portado).
pub const API_OPEN_EDITOR_COMMAND_ID: &str = "_workbench.open";

/// `API_OPEN_DIFF_EDITOR_COMMAND_ID` (`editorCommands.ts`, ainda não portado).
pub const API_OPEN_DIFF_EDITOR_COMMAND_ID: &str = "_workbench.diff";

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

    /// `element.themeIcon`.
    pub fn theme_icon(&self) -> Option<ThemeIcon> {
        match self {
            TreeElement::Item(item) => item.borrow().theme_icon.clone(),
            TreeElement::LoadMore(command) => command.theme_icon(),
        }
    }

    /// `element.icon ?? element.iconDark` como "tem imagem": o `je-ui` não desenha imagens (ver as divergências do módulo).
    pub fn has_icon_uri(&self) -> bool {
        match self {
            TreeElement::Item(item) => {
                let item = item.borrow();
                item.icon.is_some() || item.icon_dark.is_some()
            }
            TreeElement::LoadMore(_) => false,
        }
    }

    /// `element.tooltip`.
    pub fn tooltip(&self) -> Option<StringOrMarkdown> {
        match self {
            TreeElement::Item(item) => item.borrow().tooltip.clone(),
            TreeElement::LoadMore(_) => None,
        }
    }

    /// `element.accessibilityInformation`.
    pub fn accessibility_information(&self) -> Option<IAccessibilityInformation> {
        match self {
            TreeElement::Item(item) => item.borrow().accessibility_information.clone(),
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

/// O `TimelineActionContext` como contexto das ações (a carga nativa de um objeto, ver as divergências do módulo).
pub fn timeline_action_context_to_dynamic(context: &TimelineActionContext) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(context.clone())))
}

/// O `TimelineActionContext` que o contexto carrega, se for um.
pub fn timeline_action_context_from_dynamic(context: &Dynamic) -> Option<Rc<TimelineActionContext>> {
    match context {
        Dynamic::Object(object) => object.host::<TimelineActionContext>(),
        _ => None,
    }
}

/// `toActionContext(item, uri)`: o objeto `{ $mid, handle, source, uri }` que os comandos das extensões recebem.
fn to_action_context(item: &TimelineItemRef, uri: Option<&URI>) -> Dynamic {
    let item = item.borrow();
    Dynamic::object([
        ("$mid", Dynamic::from(MarshalledId::TimelineActionContext as i32)),
        ("handle", Dynamic::string(&item.handle)),
        ("source", Dynamic::string(&item.source)),
        ("uri", uri_to_dynamic(uri)),
    ])
}

/// `uri` como argumento de um comando: o `URI`, ou `undefined` para a timeline do workspace.
fn uri_to_dynamic(uri: Option<&URI>) -> Dynamic {
    uri.map_or(Dynamic::Undefined, URI::to_dynamic)
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

/// O que o painel lê do armazenamento: o texto de `timeline.excludeSources` e a lista de fontes que ele descreve, sem repetição e
/// na ordem do texto (`new Set(JSON.parse(text))`; um JSON inválido ou que não é uma lista de textos é a lista vazia).
fn read_excluded_sources(storage: &dyn StorageService) -> (String, Vec<String>) {
    let raw = storage.get(EXCLUDE_SOURCES_KEY, StorageScope::Profile, Some("[]")).unwrap_or_else(|| "[]".to_owned());
    let mut errors = Vec::new();
    let mut sources: Vec<String> = Vec::new();
    if let Some(Value::Array(values)) = parse_str(&raw, &mut errors).filter(|_| errors.is_empty()) {
        for value in values {
            if let Value::String(source) = value {
                let source = source.to_string_lossy();
                if !sources.contains(&source) {
                    sources.push(source);
                }
            }
        }
    }
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

    /// `tree.setFocus([element])`.
    fn set_focus(&self, element: &TreeElement);

    /// `tree.layout(height, width)` (e a mensagem, que acompanha a largura).
    fn layout(&self, height: f64, width: f64);

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

/// `new WorkbenchHoverDelegate(mode, { instantHover }, { position: { hoverPosition: RIGHT, clippingElement } })`: o que a
/// fábrica do delegado de hover recebe (o `RIGHT` com inversão quando falta espaço é da fábrica).
#[derive(Clone)]
pub struct TimelineHoverDelegateArgs {
    pub placement: HoverPlacement,
    pub instant_hover: bool,
    pub clipping_element: NodeId,
}

/// `createInstance(WorkbenchHoverDelegate, ...)`.
pub type TimelineHoverDelegateFactory = Rc<dyn Fn(&TimelineHoverDelegateArgs) -> Rc<dyn IHoverDelegate>>;

/// Os serviços do corpo (`renderBody`), as peças que o original cria com `createInstance`.
#[derive(Clone)]
pub struct TimelineBodyServices {
    /// Os serviços da `WorkbenchObjectTree` (`IContextKeyService`, `IListService`, `IConfigurationService` e `defaultListStyles`).
    pub tree: WorkbenchTreeServices,
    /// `IThemeService` do renderer (a cor do `themeIcon`).
    pub theme_service: Rc<dyn IThemeService>,
    pub create_hover_delegate: TimelineHoverDelegateFactory,
    /// O estilo do texto das linhas (o `foreground` dele é o do ícone sem cor e o do texto da mensagem).
    pub icon_label_styles: IconLabelStyles,
    /// `defaultInputBoxStyles`, lido a cada edição (o tema pode mudar).
    pub input_box_styles: Rc<dyn Fn() -> InputBoxStyles>,
    pub context_view_provider: Option<Rc<dyn IContextViewProvider>>,
    /// `getLocationBasedColors().listOverrideStyles` resolvido pelo tema (`overrideStyles` da árvore).
    pub list_override_styles: Rc<dyn Fn(&ListOverrideStyleColors) -> IListStyles>,
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
    pub body: TimelineBodyServices,
}

/// `TimelinePane`.
pub struct TimelinePane {
    base: Rc<Disposable>,
    store: Rc<DisposableStore>,
    weak_self: Weak<TimelinePane>,
    view_pane: Rc<ViewPane>,
    services: TimelinePaneServices,
    commands: RefCell<Option<Rc<TimelinePaneCommands>>>,

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
            commands: RefCell::new(None),
            body: RefCell::new(None),
            visibility_disposables: RefCell::new(None),
            follow_active_editor_context,
            timeline_exclude_sources_context,
            workspace_context,
            excluded_sources: RefCell::new(excluded_sources.into_iter().collect()),
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

        let delegate: Rc<dyn ViewPaneDelegate> = this.clone();
        let delegate: Weak<dyn ViewPaneDelegate> = Rc::downgrade(&delegate);
        this.view_pane.set_delegate(delegate);

        let host: Rc<dyn ITimelinePaneCommandsHost> = this.clone();
        let commands = TimelinePaneCommands::new(
            Rc::downgrade(&host),
            this.services.timeline_service.clone(),
            this.services.storage_service.clone(),
            this.services.view_pane.context_key_service.clone(),
            this.services.view_pane.menu_service.clone(),
        );
        this.base.register(commands.clone());
        *this.commands.borrow_mut() = Some(commands);

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

    /// `this.pendingRequests.get(source)?.request`.
    pub fn pending_request(&self, source: &str) -> Option<TimelineRequest> {
        self.pending_requests.borrow().get(&source.to_owned()).map(|pending_request| pending_request.request.clone())
    }

    /// `this.timelinesBySource.get(source)`.
    pub fn timeline_for_source(&self, source: &str) -> Option<Rc<TimelineAggregate>> {
        self.timelines_by_source.borrow().get(&source.to_owned()).cloned()
    }

    /// `this.excludedSources = sources`.
    pub fn set_excluded_sources(&self, sources: HashSet<String>) {
        *self.excluded_sources.borrow_mut() = sources;
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
        *self.excluded_sources.borrow_mut() = excluded_sources.into_iter().collect();

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
                uri.fs_path(host_platform()) == current.fs_path(host_platform()) && is_file_or_git(uri) && is_file_or_git(current)
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

    pub fn on_providers_changed(&self, event: &TimelineProvidersChangeEvent) {
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

    pub fn on_timeline_changed(&self, event: &TimelineChangeEvent) {
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

    pub fn on_editable_changed(&self) {
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

    pub fn clear(&self, cancel_pending: bool) {
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
    pub fn load_timeline_for_source(&self, source: &str, uri: Option<&URI>, reset: bool, options: Option<TimelineOptions>, trigger_kind: TimelineTriggerKind) -> bool {
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

// O corpo da vista (`renderBody` e o que depende dele)

impl TimelinePane {
    fn commands(&self) -> Rc<TimelinePaneCommands> {
        self.commands.borrow().clone().expect("o construtor cria os comandos")
    }

    /// `loadMore(item)`.
    pub fn load_more(&self, item: &Rc<LoadMoreCommand>) {
        if item.loading() {
            return;
        }

        item.set_loading(true);
        if let Some(body) = self.body() {
            body.rerender_element(&TreeElement::LoadMore(item.clone()));
        }

        if self.pending_request_count() != 0 {
            return;
        }

        self.set_max_item_count(self.visible_item_count() as f64 + self.page_size());
        self.load_timeline(false, None, TimelineTriggerKind::Automatic);
    }

    /// O `onDidOpen` do `renderBody`.
    fn on_did_open(&self, event: &IOpenEvent<TreeElement>) {
        if event.browser_event.is_none() || !self.ensure_valid_items() {
            return;
        }
        let Some(body) = self.body() else { return };

        // Fora de uma seleção de um elemento o `item` é `undefined`, e a raiz (`null`) não abre nada
        let selection = body.get_selection();
        let [Some(item)] = selection.as_slice() else { return };

        if let Some(timeline_item) = item.timeline_item() {
            let command = timeline_item.borrow().command.clone();
            if let Some(command) = command {
                let mut args = command.arguments.clone().unwrap_or_default();
                if command.id == API_OPEN_EDITOR_COMMAND_ID || command.id == API_OPEN_DIFF_EDITOR_COMMAND_ID {
                    // Some commands owned by us should receive the `IOpenEvent` as context to open properly
                    args.push(Dynamic::Object(ObjectRef::with_host(Rc::new(event.clone()))));
                }

                let _ = self.services.command_service.execute_command(&command.id, args);
            }
        } else if let Some(load_more) = item.load_more_command() {
            self.load_more(load_more);
        }
    }

    /// `onContextMenu(commands, treeEvent)`.
    fn on_context_menu(&self, tree_event: &TreeMouseEvent<Option<TreeElement>>) {
        // `treeEvent.element === null`: nada sob o ponteiro, ou a raiz
        let Some(Some(item)) = tree_event.element.clone() else { return };

        if !self.ensure_valid_items() {
            return;
        }
        let Some(body) = self.body() else { return };

        body.set_focus(&item);
        let selection = item.timeline_item().and_then(|timeline_item| get_multi_selection(timeline_item, &body.get_selection()));
        let actions = self.commands().get_item_context_actions(&item, selection.as_deref());
        if actions.is_empty() {
            return;
        }

        let anchor_node = tree_event.browser_event.target.unwrap_or_else(|| self.view_pane.element());
        let get_selection: Rc<dyn Fn() -> Vec<Option<TreeElement>>> = {
            let body = body.clone();
            Rc::new(move || body.get_selection())
        };
        let weak = self.weak_self.clone();
        self.services.view_pane.context_menu_service.show_context_menu(ContextMenuRequest::Menu(IContextMenuMenuDelegate {
            menu_id: None,
            menu_action_options: None,
            get_actions_context: Some(Rc::new(move |_event: Option<&IContextMenuEvent>| {
                let uri = weak.upgrade().and_then(|pane| pane.uri());
                Some(timeline_action_context_to_dynamic(&TimelineActionContext { uri, item: item.clone() }))
            })),
            context_key_service: None,
            get_actions: Some(Rc::new(move || actions.clone())),
            base: Rc::new(TimelineContextMenuDelegate {
                anchor: AnchorTarget::Element(ViewElement::from_native(Rc::new(anchor_node))),
                body,
                action_runner: TimelineActionRunner::new(get_selection),
            }),
        }));
    }

    /// `setVisible(visible)`.
    pub fn set_visible(&self, visible: bool) {
        if visible {
            let _ = self.services.extension_service.activate_by_event("onView:timeline", None);
            let visibility_disposables = self.replace_visibility_disposables();

            let weak = self.weak_self.clone();
            visibility_disposables.add(self.services.editor_service.on_did_active_editor_change().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_active_editor_changed();
                }
            }));

            // Refresh the view on focus to update the relative timestamps
            let weak = self.weak_self.clone();
            visibility_disposables.add(self.view_pane.on_did_focus().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.refresh_debounced();
                }
            }));

            self.view_pane.set_visible(visible);

            self.on_active_editor_changed();
        } else {
            self.dispose_visibility_disposables();

            self.view_pane.set_visible(visible);
        }
    }
}

/// `TimelinePaneCommands` conhece do painel só o que as ações dele chamam (ver as divergências do módulo).
pub trait ITimelinePaneCommandsHost {
    /// `pane.id`.
    fn id(&self) -> String;

    /// `pane.reset()`.
    fn reset(&self);

    /// `pane.showWorkspaceTimeline()`.
    fn show_workspace_timeline(&self);

    /// `pane.followActiveEditor = !pane.followActiveEditor`: devolve o valor novo.
    fn toggle_follow_active_editor(&self) -> bool;

    /// `pane.compareSelected()`.
    fn compare_selected(&self) -> Promise<()>;
}

impl ITimelinePaneCommandsHost for TimelinePane {
    fn id(&self) -> String {
        self.view_pane.id.clone()
    }

    fn reset(&self) {
        self.load_timeline(true, None, TimelineTriggerKind::Invoke);
    }

    fn show_workspace_timeline(&self) {
        self.set_uri_core(None, true, true);
    }

    fn toggle_follow_active_editor(&self) -> bool {
        let value = !self.follow_active_editor();
        self.set_follow_active_editor(value);
        value
    }

    fn compare_selected(&self) -> Promise<()> {
        let Some(body) = self.body() else { return Promise::resolved(()) };
        let mut selection: Vec<TimelineItemRef> = body.get_selection().iter().filter_map(|element| element.as_ref().and_then(TreeElement::timeline_item).cloned()).collect();
        if selection.len() != 2 {
            return Promise::resolved(());
        }

        selection.sort_by(|a, b| {
            let difference = a.borrow().timestamp - b.borrow().timestamp;
            if difference < 0.0 {
                Ordering::Less
            } else if difference > 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        });
        let (older, newer) = (selection[0].borrow(), selection[1].borrow());
        let (Some(older_resource), Some(newer_resource)) = (older.resource_uri.clone(), newer.resource_uri.clone()) else {
            return Promise::resolved(());
        };

        let label = localize(
            "timeline.compareSelectedLabel",
            "{0} ({1}) \u{2194} {2} ({3})",
            &[&self.services.label_service.get_uri_basename_label(&older_resource), &older.label, &self.services.label_service.get_uri_basename_label(&newer_resource), &newer.label],
        );
        let opened = self.services.editor_service.open_editor(UntypedEditorInput {
            original: Some(Box::new(UntypedEditorInput::from_resource(older_resource))),
            modified: Some(Box::new(UntypedEditorInput::from_resource(newer_resource))),
            label: Some(label),
            ..UntypedEditorInput::default()
        });
        async_fn(async move {
            opened.await?;
            Ok(())
        })
    }
}

/// O `IContextMenuDelegate` do menu de um item: a âncora, o foco de volta na árvore ao cancelar e o runner das ações (as ações
/// e o contexto vão pelo [`IContextMenuMenuDelegate`]).
struct TimelineContextMenuDelegate {
    anchor: AnchorTarget,
    body: Rc<dyn ITimelineBody>,
    action_runner: Rc<ActionRunner>,
}

impl IContextMenuDelegate for TimelineContextMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        self.anchor.clone()
    }

    fn get_actions(&self) -> Vec<ActionRef> {
        Vec::new()
    }

    fn on_hide(&self, did_cancel: bool) {
        if did_cancel {
            self.body.dom_focus();
        }
    }

    fn action_runner(&self) -> Option<Rc<dyn IActionRunner>> {
        Some(self.action_runner.clone())
    }
}

impl ViewPaneDelegate for TimelinePane {
    fn get_actions_context(&self) -> Option<Dynamic> {
        self.uri.borrow().as_ref().map(|uri| Dynamic::Object(ObjectRef::with_host(Rc::new(uri.clone()))))
    }

    fn render_header_title(&self, container: NodeId) {
        self.services.view_pane.ui.tree_mut().add_class(container, "timeline-view");
    }

    fn render_body(&self, container: NodeId) {
        let ui = self.services.view_pane.ui.clone();
        let (message_node, tree_node) = {
            let mut tree = ui.tree_mut();
            tree.add_class(container, "tree-explorer-viewlet-tree-view");
            tree.add_class(container, "timeline-tree-view");

            let message_node = create_el(&mut tree, container, Role::Generic, "message");
            tree.add_class(message_node, "timeline-subtle");
            {
                let style = tree.style_mut(message_node);
                style.opacity = 0.5;
                style.pointer_events = PointerEvents::None;
                style.z_index = 1;
            }

            let tree_node = create_el(&mut tree, container, Role::Generic, "customview-tree");
            tree.add_class(tree_node, "file-icon-themable-tree");
            tree.add_class(tree_node, "hide-arrows");
            tree.set_fill(tree_node, true, true);
            (message_node, tree_node)
        };

        // O renderer lê a seleção da árvore, que nasce depois dele
        let tree_slot: Rc<RefCell<Weak<TimelineTree>>> = Rc::new(RefCell::new(Weak::new()));
        let get_selection: Rc<dyn Fn() -> Vec<Option<TreeElement>>> = {
            let tree_slot = tree_slot.clone();
            Rc::new(move || tree_slot.borrow().upgrade().map(|tree| tree.get_selection()).unwrap_or_default())
        };

        let create_action_view_item = self.services.view_pane.create_action_view_item.clone();
        let action_view_item_provider: IActionViewItemProvider = Rc::new(move |action: &ActionRef, options: &IActionViewItemOptions| {
            let menu_as_child = action.as_any().is::<SubmenuAction>();
            create_action_view_item(action, options, menu_as_child)
        });

        let renderer = TimelineTreeRenderer::new(TimelineTreeRendererArgs {
            ui: ui.clone(),
            services: self.services.body.clone(),
            context_menu_service: self.services.view_pane.context_menu_service.clone(),
            commands: self.commands(),
            timeline_service: self.services.timeline_service.clone(),
            view_container_location: self.services.view_pane.view_descriptor_service.get_view_location_by_id(&self.view_pane.id),
            clipping_element: tree_node,
            get_selection,
            action_view_item_provider,
        });
        self.base.register(renderer.clone());
        let weak = self.weak_self.clone();
        self.base.register(renderer.on_did_scroll_to_end().listen(move |item| {
            if let Some(this) = weak.upgrade()
                && this.page_on_scroll()
            {
                this.load_more(item);
            }
        }));

        let tree_renderer: Rc<dyn ITreeRenderer<TimelineModel>> = renderer.clone();
        let tree = TimelineTree::new(
            "TimelinePane",
            &ui,
            tree_node,
            Rc::new(TimelineListVirtualDelegate),
            vec![tree_renderer],
            ObjectTreeOptions {
                tree: AbstractTreeOptions {
                    identity_provider: Some(Rc::new(TimelineIdentityProvider)),
                    keyboard_navigation_label_provider: Some(Rc::new(TimelineKeyboardNavigationLabelProvider)),
                    ..AbstractTreeOptions::default()
                },
                sorter: None,
            },
            IWorkbenchTreeOptions {
                override_styles: Some((self.services.body.list_override_styles)(&self.view_pane.get_location_based_colors().list_override_styles)),
                ..IWorkbenchTreeOptions::default()
            },
            &self.services.body.tree,
        );
        *tree_slot.borrow_mut() = Rc::downgrade(&tree);
        self.base.register(tree.widget().clone());

        timeline_view_focused_context().bind_to(&*tree.context_key_service());

        let weak = self.weak_self.clone();
        self.base.register(tree.on_context_menu().listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.on_context_menu(event);
            }
        }));
        let weak = self.weak_self.clone();
        self.base.register(tree.on_did_change_selection().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_selection_changed();
            }
        }));
        let weak = self.weak_self.clone();
        self.base.register(tree.on_did_open().listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.on_did_open(event);
            }
        }));

        let body = Rc::new(TimelineBody {
            ui,
            container,
            message_node,
            message: RefCell::new(String::new()),
            message_lines: RefCell::new(Vec::new()),
            width: Cell::new(0.0),
            text_styles: self.services.body.icon_label_styles,
            tree,
            renderer,
        });
        self.set_body(body);

        self.set_message(Some(localize("timeline.editorCannotProvideTimeline", "The active editor cannot provide timeline information.", &[])));
    }

    fn layout_body(&self, height: f64, width: f64) {
        if let Some(body) = self.body() {
            body.layout(height, width);
        }
    }
}

impl IViewPane for TimelinePane {
    /// `setExpanded(expanded)`.
    fn set_expanded(&self, expanded: bool) -> bool {
        let changed = self.view_pane.set_expanded(expanded);

        if changed && self.is_body_visible() {
            if !self.follow_active_editor() {
                self.set_uri_core(self.uri(), self.workspace(), true);
            } else {
                self.on_active_editor_changed();
            }
        }

        changed
    }

    /// `focus()`.
    fn focus(&self) {
        self.view_pane.focus();
        if let Some(body) = self.body() {
            body.dom_focus();
        }
    }
}

/// A árvore do painel: `WorkbenchObjectTree<TreeElement, FuzzyScore>`.
type TimelineTree = WorkbenchObjectTree<TreeElement, FuzzyScore>;

/// O modelo da árvore do painel.
type TimelineModel = ObjectTreeModel<TreeElement, FuzzyScore>;

/// O nó que o renderer recebe.
type TimelineNode = <TimelineModel as ITreeModel>::Node;

/// `padding: 10px 22px 0 22px` do `.message`.
const MESSAGE_PADDING_X: f64 = 22.0;
const MESSAGE_PADDING_TOP: f64 = 10.0;

/// Reparte `text` em linhas de no máximo `width` (as palavras que não cabem vão para a linha seguinte; uma palavra maior que a
/// linha fica sozinha numa).
fn wrap_words(text: &str, width: f64, measure: impl Fn(&str) -> f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split(' ') {
        let candidate = if current.is_empty() { word.to_owned() } else { format!("{current} {word}") };
        if current.is_empty() || measure(&candidate) <= width {
            current = candidate;
        } else {
            lines.push(std::mem::replace(&mut current, word.to_owned()));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// O corpo que nasce em `renderBody`: a mensagem (`$message`), a árvore e o renderer. Implementa o que a lógica de carga usa
/// dele ([`ITimelineBody`]).
pub struct TimelineBody {
    ui: Ui,
    /// `$container`.
    container: NodeId,
    /// `$message`.
    message_node: NodeId,
    message: RefCell<String>,
    message_lines: RefCell<Vec<NodeId>>,
    width: Cell<f64>,
    text_styles: IconLabelStyles,
    tree: Rc<TimelineTree>,
    renderer: Rc<TimelineTreeRenderer>,
}

impl TimelineBody {
    /// Quebra a mensagem em linhas de acordo com a largura do último `layout`.
    fn layout_message(&self) {
        let text = self.message.borrow().clone();
        let width = (self.width.get() - 2.0 * MESSAGE_PADDING_X).max(0.0);
        let styles = self.text_styles.text;
        let style = ui_font(styles.font_size, styles.font_weight);
        let lines = wrap_words(&text, width, |candidate| measure_text(candidate, &style));

        let mut tree = self.ui.tree_mut();
        for line in self.message_lines.borrow_mut().drain(..) {
            tree.remove(line);
        }
        let mut nodes = Vec::with_capacity(lines.len());
        for (index, line) in lines.iter().enumerate() {
            let node = create_el(&mut tree, self.message_node, Role::Generic, "");
            tree.style_mut(node).content = Content::Text { text: line.clone(), style: style.clone(), color: styles.foreground, line_height: ITEM_HEIGHT };
            tree.set_bounds(node, Rect::new(0.0, index as f64 * ITEM_HEIGHT, width, (index + 1) as f64 * ITEM_HEIGHT));
            nodes.push(node);
        }
        tree.set_bounds(self.message_node, Rect::new(MESSAGE_PADDING_X, MESSAGE_PADDING_TOP, MESSAGE_PADDING_X + width, MESSAGE_PADDING_TOP + lines.len() as f64 * ITEM_HEIGHT));
        *self.message_lines.borrow_mut() = nodes;
    }
}

impl ITimelineBody for TimelineBody {
    fn render_height(&self) -> f64 {
        self.tree.render_height()
    }

    fn set_children(&self, elements: Vec<TreeElement>) {
        self.tree.set_children(&None, elements.into_iter().map(IObjectTreeElement::new).collect(), ObjectTreeModelSetChildrenOptions::default());
    }

    fn clear_children(&self) {
        self.tree.set_children(&None, Vec::new(), ObjectTreeModelSetChildrenOptions::default());
    }

    fn rerender(&self) {
        self.tree.rerender_element(None);
    }

    fn rerender_element(&self, element: &TreeElement) {
        self.tree.rerender_element(Some(element));
    }

    fn has_element(&self, element: &TreeElement) -> bool {
        self.tree.has_element(element)
    }

    fn get_selection(&self) -> Vec<Option<TreeElement>> {
        self.tree.get_selection()
    }

    fn dom_focus(&self) {
        self.tree.dom_focus();
    }

    fn set_focus(&self, element: &TreeElement) {
        self.tree.set_focus(&[Some(element.clone())], None);
    }

    fn layout(&self, height: f64, width: f64) {
        self.width.set(width);
        self.tree.layout(Some(height), Some(width));
        self.layout_message();
    }

    fn show_message(&self, message: &str) {
        {
            let mut tree = self.ui.tree_mut();
            tree.remove_class(self.message_node, "hide");
            show(&mut tree, self.message_node);
        }
        *self.message.borrow_mut() = message.to_owned();
        self.layout_message();
    }

    fn hide_message(&self) {
        self.message.borrow_mut().clear();
        self.layout_message();
        let mut tree = self.ui.tree_mut();
        tree.add_class(self.message_node, "hide");
        hide(&mut tree, self.message_node);
    }

    fn set_uri(&self, uri: Option<&URI>) {
        self.renderer.set_uri(uri.cloned());
    }

    fn is_focus_free_or_inside(&self) -> bool {
        match self.ui.focused() {
            None => true,
            Some(focused) => focused == self.ui.root() || is_ancestor_of_active_element(&self.ui, self.container),
        }
    }
}

/// `TimelineIdentityProvider` para os elementos `T | null` da árvore (a raiz não tem identidade).
impl IIdentityProvider<Option<TreeElement>> for TimelineIdentityProvider {
    fn get_id(&self, item: &Option<TreeElement>) -> String {
        item.as_ref().map(TreeElement::handle).unwrap_or_default()
    }
}

/// O que o `accessibilityProvider` da árvore devolve: os textos e papéis do original (ver as divergências do módulo).
pub struct TimelineAccessibilityProvider;

impl TimelineAccessibilityProvider {
    /// `getAriaLabel(element)`.
    pub fn get_aria_label(&self, element: &TreeElement) -> String {
        if let Some(command) = element.load_more_command() {
            return command.aria_label();
        }
        match element.accessibility_information() {
            Some(information) => information.label,
            None => localize("timeline.aria.item", "{0}: {1}", &[&element.relative_time_full_word().unwrap_or_default(), &element.label()]),
        }
    }

    /// `getRole(element)`.
    pub fn get_role(&self, element: &TreeElement) -> String {
        if element.load_more_command().is_some() {
            return "treeitem".to_owned();
        }
        element.accessibility_information().and_then(|information| information.role).filter(|role| !role.is_empty()).unwrap_or_else(|| "treeitem".to_owned())
    }

    /// `getWidgetAriaLabel()`.
    pub fn get_widget_aria_label(&self) -> String {
        localize("timeline", "Timeline", &[])
    }
}

/// `TimelineKeyboardNavigationLabelProvider`.
pub struct TimelineKeyboardNavigationLabelProvider;

impl IKeyboardNavigationLabelProvider<Option<TreeElement>> for TimelineKeyboardNavigationLabelProvider {
    fn get_keyboard_navigation_label(&self, element: &Option<TreeElement>) -> Vec<Option<String>> {
        vec![element.as_ref().map(TreeElement::label)]
    }
}

/// `TimelineElementTemplate.id`.
pub const TIMELINE_ELEMENT_TEMPLATE_ID: &str = "TimelineElementTemplate";

/// `TimelineListVirtualDelegate`.
pub struct TimelineListVirtualDelegate;

impl IListVirtualDelegate<TreeElement> for TimelineListVirtualDelegate {
    fn get_height(&self, _element: &TreeElement) -> f64 {
        ITEM_HEIGHT
    }

    fn get_template_id(&self, _element: &TreeElement) -> String {
        TIMELINE_ELEMENT_TEMPLATE_ID.to_owned()
    }
}

/// `TimelineActionRunner`: executa os comandos sobre os itens da timeline com `(item, uri, source, selectedItems?)`, ver
/// `vscode.TimelineItem.contextValue`. É o [`ActionRunner`] com o `runAction` sobrescrito (o contexto é um
/// [`TimelineActionContext`], ver [`timeline_action_context_to_dynamic`]).
pub struct TimelineActionRunner;

impl TimelineActionRunner {
    /// `new TimelineActionRunner(getSelection)`.
    pub fn new(get_selection: Rc<dyn Fn() -> Vec<Option<TreeElement>>>) -> Rc<ActionRunner> {
        let run_action: RunAction = Rc::new(move |action: &ActionRef, context: Option<Dynamic>| -> Promise<()> {
            // A desestruturação `{ uri, item }` de um contexto ausente lança `TypeError`
            let Some(context) = context.as_ref().and_then(timeline_action_context_from_dynamic) else {
                errors::throw(Error::type_error("Cannot destructure property 'uri' of 'undefined' as it is undefined."))
            };
            let TimelineActionContext { uri, item } = (*context).clone();

            let Some(timeline_item) = item.timeline_item().cloned() else {
                // Um item que não é da timeline (o "Load more") roda a ação sem argumentos
                let promise = action.run(&[]);
                return async_fn(async move {
                    promise.await?;
                    Ok(())
                });
            };

            // Commands only receive the selected items of the provider of the item
            let source = timeline_item.borrow().source.clone();
            let selection = get_multi_selection(&timeline_item, &get_selection()).map(|items| items.into_iter().filter(|selected| selected.borrow().source == source).collect::<Vec<_>>());

            let promise = action.run(&[
                to_action_context(&timeline_item, uri.as_ref()),
                uri_to_dynamic(uri.as_ref()),
                Dynamic::string(&source),
                selection.map_or(Dynamic::Undefined, |items| Dynamic::array(items.iter().map(|selected| to_action_context(selected, uri.as_ref())))),
            ]);
            async_fn(async move {
                promise.await?;
                Ok(())
            })
        });
        ActionRunner::with_run_action(run_action)
    }
}

/// Os argumentos do [`TimelineTreeRenderer`] (`commands`, `viewContainerLocation`, `clippingElement`, `getSelection` e os
/// serviços injetados).
pub struct TimelineTreeRendererArgs {
    pub ui: Ui,
    pub services: TimelineBodyServices,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub commands: Rc<TimelinePaneCommands>,
    pub timeline_service: Rc<dyn ITimelineService>,
    pub view_container_location: Option<ViewContainerLocation>,
    pub clipping_element: NodeId,
    pub get_selection: Rc<dyn Fn() -> Vec<Option<TreeElement>>>,
    /// `createActionViewItem.bind(undefined, this.instantiationService)`.
    pub action_view_item_provider: IActionViewItemProvider,
}

/// `TimelineElementTemplate`: a linha (`.custom-view-tree-node-item`) com o ícone, o rótulo, o carimbo de tempo e as ações.
struct TimelineElementTemplate {
    ui: Ui,
    container: NodeId,
    icon: NodeId,
    icon_label: Rc<IconLabel>,
    timestamp_container: NodeId,
    timestamp: NodeId,
    actions_container: NodeId,
    action_bar: Rc<ActionBar>,
    /// A caixa de edição em linha (`timeline-item-editing`), no lugar do rótulo, do carimbo e das ações.
    editor: RefCell<Option<Rc<InlineEditInputBox>>>,
    hovered: Cell<bool>,
    template_disposables: Rc<DisposableStore>,
    element_disposables: Rc<DisposableStore>,
}

/// O `margin-right` de 6px depois do ícone de 16px.
const ICON_COLUMN_WIDTH: f64 = 22.0;

/// `.timeline-timestamp--duplicate .timeline-timestamp { width: 10px }`.
const DUPLICATE_TIMESTAMP_WIDTH: f64 = 10.0;

impl TimelineElementTemplate {
    /// `reset()`.
    fn reset(&self) {
        self.element_disposables.clear();
        *self.editor.borrow_mut() = None;
        self.hovered.set(false);
        {
            let mut tree = self.ui.tree_mut();
            tree.set_class_name(self.icon, "");
            tree.style_mut(self.icon).content = Content::None;
        }
        self.action_bar.clear();
    }

    /// `.actions` aparecem com o ponteiro sobre a linha, e só fora da edição.
    fn update_actions_visibility(&self) {
        let visible = self.hovered.get() && self.editor.borrow().is_none() && !self.action_bar.is_empty();
        self.ui.tree_mut().style_mut(self.actions_container).visible = visible;
        self.layout_row();
    }

    /// O `display: flex` da linha: reparte a largura entre o ícone, o rótulo (`flex: 1`), o carimbo e as ações.
    fn layout_row(&self) {
        let row_width = self.ui.tree().bounds(self.container).width();

        let editor = self.editor.borrow().clone();
        if let Some(editor) = editor {
            let input_width = (row_width - ICON_COLUMN_WIDTH).max(0.0);
            editor.input_box().set_width(input_width);
            self.ui.tree_mut().set_bounds(editor.input_box().element(), Rect::new(ICON_COLUMN_WIDTH, 0.0, ICON_COLUMN_WIDTH + input_width, ITEM_HEIGHT));
            return;
        }

        let actions_width = if self.ui.tree().style(self.actions_container).visible { (0..self.action_bar.length()).map(|index| self.action_bar.get_width(index)).sum::<f64>() } else { 0.0 };
        let timestamp_width = {
            let tree = self.ui.tree();
            if tree.has_class(self.timestamp_container, "timeline-timestamp--duplicate") {
                DUPLICATE_TIMESTAMP_WIDTH
            } else {
                match &tree.style(self.timestamp).content {
                    Content::Text { text, style, .. } => measure_text(text, style),
                    _ => 0.0,
                }
            }
        };

        // `.timeline-timestamp-container { margin-left: 2px; margin-right: 4px }`
        let timestamp_container_width = 2.0 + timestamp_width + 4.0;
        let label_width = (row_width - ICON_COLUMN_WIDTH - timestamp_container_width - actions_width).max(0.0);
        let timestamp_x = ICON_COLUMN_WIDTH + label_width;
        let actions_x = timestamp_x + timestamp_container_width;

        {
            let mut tree = self.ui.tree_mut();
            tree.set_bounds(self.icon, Rect::new(0.0, 0.0, 16.0, ITEM_HEIGHT));
            tree.set_bounds(self.icon_label.element(), Rect::new(ICON_COLUMN_WIDTH, 0.0, timestamp_x, ITEM_HEIGHT));
            tree.set_bounds(self.timestamp_container, Rect::new(timestamp_x, 0.0, actions_x, ITEM_HEIGHT));
            tree.set_bounds(self.timestamp, Rect::new(2.0, 0.0, 2.0 + timestamp_width, ITEM_HEIGHT));
            tree.set_bounds(self.actions_container, Rect::new(actions_x, 0.0, actions_x + actions_width, ITEM_HEIGHT));
        }
        self.icon_label.layout(Some(label_width));
    }
}

/// `TimelineTreeRenderer`.
pub struct TimelineTreeRenderer {
    base: Rc<Disposable>,
    ui: Ui,
    services: TimelineBodyServices,
    context_menu_service: Rc<dyn IContextMenuService>,
    commands: Rc<TimelinePaneCommands>,
    timeline_service: Rc<dyn ITimelineService>,
    hover_delegate: Rc<dyn IHoverDelegate>,
    action_view_item_provider: IActionViewItemProvider,
    get_selection: Rc<dyn Fn() -> Vec<Option<TreeElement>>>,
    uri: RefCell<Option<URI>>,
    on_did_scroll_to_end: Rc<Emitter<Rc<LoadMoreCommand>>>,
}

impl TimelineTreeRenderer {
    pub fn new(args: TimelineTreeRendererArgs) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_scroll_to_end = Emitter::new();
        base.register(on_did_scroll_to_end.clone());

        let in_panel = args.view_container_location == Some(ViewContainerLocation::Panel);
        let hover_delegate = (args.services.create_hover_delegate)(&TimelineHoverDelegateArgs {
            placement: if in_panel { HoverPlacement::Mouse } else { HoverPlacement::Element },
            instant_hover: !in_panel,
            clipping_element: args.clipping_element,
        });

        Rc::new(Self {
            base,
            ui: args.ui,
            services: args.services,
            context_menu_service: args.context_menu_service,
            commands: args.commands,
            timeline_service: args.timeline_service,
            hover_delegate,
            action_view_item_provider: args.action_view_item_provider,
            get_selection: args.get_selection,
            uri: RefCell::new(None),
            on_did_scroll_to_end,
        })
    }

    /// `onDidScrollToEnd`.
    pub fn on_did_scroll_to_end(&self) -> Event<Rc<LoadMoreCommand>> {
        self.on_did_scroll_to_end.event()
    }

    /// `setUri(uri)`.
    pub fn set_uri(&self, uri: Option<URI>) {
        *self.uri.borrow_mut() = uri;
    }

    fn template(template_data: &TemplateData) -> &Rc<TimelineElementTemplate> {
        template_data.downcast_ref::<Rc<TimelineElementTemplate>>().expect("o template do TimelineTreeRenderer")
    }
}

impl IDisposable for TimelineTreeRenderer {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "TimelineTreeRenderer"
    }
}

impl ITreeRenderer<TimelineModel> for TimelineTreeRenderer {
    fn template_id(&self) -> String {
        TIMELINE_ELEMENT_TEMPLATE_ID.to_owned()
    }

    fn render_template(&self, container: NodeId) -> TemplateData {
        let ui = &self.ui;
        let template_disposables = DisposableStore::new();

        ui.tree_mut().add_class(container, "custom-view-tree-node-item");
        let icon = create_el(&mut ui.tree_mut(), container, Role::Image, "custom-view-tree-node-item-icon");

        let icon_label = IconLabel::new(
            ui,
            container,
            Some(IIconLabelCreationOptions { support_highlights: true, support_icons: true, hover_delegate: Some(self.hover_delegate.clone()), ..IIconLabelCreationOptions::default() }),
            self.services.icon_label_styles,
        );
        template_disposables.add(icon_label.clone());

        let timestamp_container = create_el(&mut ui.tree_mut(), container, Role::Generic, "timeline-timestamp-container");
        let timestamp = create_el(&mut ui.tree_mut(), timestamp_container, Role::Generic, "timeline-timestamp");
        let actions_container = create_el(&mut ui.tree_mut(), container, Role::Generic, "actions");
        {
            let mut tree = ui.tree_mut();
            tree.style_mut(timestamp_container).opacity = 0.5;
            tree.style_mut(actions_container).visible = false;
        }
        let action_bar = ActionBar::new(ui, actions_container, IActionBarOptions { action_view_item_provider: Some(self.action_view_item_provider.clone()), ..IActionBarOptions::default() });
        template_disposables.add(action_bar.clone());

        let element_disposables = template_disposables.add_rc(DisposableStore::new());
        let template = Rc::new(TimelineElementTemplate {
            ui: ui.clone(),
            container,
            icon,
            icon_label,
            timestamp_container,
            timestamp,
            actions_container,
            action_bar,
            editor: RefCell::new(None),
            hovered: Cell::new(false),
            template_disposables: template_disposables.clone(),
            element_disposables,
        });

        // `.monaco-list-row:hover .actions`
        for (kind, hovered) in [(EventKind::PointerEnter, true), (EventKind::PointerLeave, false)] {
            let weak = Rc::downgrade(&template);
            template_disposables.add(ui.listen(container, kind, false, move |_| {
                if let Some(template) = weak.upgrade() {
                    template.hovered.set(hovered);
                    template.update_actions_visibility();
                }
            }));
        }

        Rc::new(template) as Rc<dyn Any>
    }

    fn render_element(&self, node: &Rc<TimelineNode>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let template = Self::template(template_data);
        template.reset();

        let item = node.element().expect("elemento da árvore");
        let ui = &self.ui;

        // O ícone: a imagem de um `icon`/`iconDark` o `je-ui` não desenha (ver as divergências do módulo)
        let theme_icon = if item.has_icon_uri() { None } else { item.theme_icon() };
        let icon_color = theme_icon.as_ref().and_then(|icon| icon.color.as_ref()).and_then(|color| self.services.theme_service.get_color_theme().get_color(&color.id, None)).map(|color| to_rgba8(&color));
        {
            let mut tree = ui.tree_mut();
            tree.set_class_name(template.icon, "custom-view-tree-node-item-icon");
            if let Some(theme_icon) = &theme_icon {
                for class_name in theme_icon.as_class_name().split(' ') {
                    tree.add_class(template.icon, class_name);
                }
                tree.style_mut(template.icon).content =
                    Content::Codicon { id: theme_icon.id.clone(), size: 16.0, color: icon_color.unwrap_or(self.services.icon_label_styles.text.foreground) };
            }
        }

        let tooltip = item.tooltip().filter(StringOrMarkdown::is_truthy).map(|tooltip| match tooltip {
            StringOrMarkdown::String(text) => IconLabelTitle::Text(text),
            StringOrMarkdown::Markdown(markdown) => IconLabelTitle::Markdown(ManagedHoverContent::Markdown {
                markdown_not_supported_fallback: Some(render_as_plaintext(&markdown.value.to_string_lossy())),
                markdown: Some(HoverContent::Markdown(markdown)),
            }),
        });
        let filter_data = node.filter_data();
        template.icon_label.set_label(
            IconLabelText::Single(item.label()),
            item.description().as_deref(),
            Some(&IIconLabelValueOptions { title: tooltip, matches: Some(create_matches(filter_data.as_deref())), ..IIconLabelValueOptions::default() }),
        );

        // O carimbo de tempo (`timeline-timestamp--duplicate` quando repete o do item de cima)
        let styles = self.services.icon_label_styles.text;
        {
            let mut tree = ui.tree_mut();
            tree.style_mut(template.timestamp).content =
                Content::Text { text: item.relative_time().unwrap_or_default(), style: ui_font(styles.font_size, styles.font_weight), color: styles.foreground, line_height: ITEM_HEIGHT };
            tree.set_label(template.timestamp, item.relative_time_full_word().unwrap_or_default());
            tree.toggle_class(template.timestamp_container, "timeline-timestamp--duplicate", item.timeline_item().is_some() && item.hide_relative_time() == Some(true));
        }

        let selection = item.timeline_item().and_then(|timeline_item| get_multi_selection(timeline_item, &(self.get_selection)()));
        template.action_bar.set_context(Some(timeline_action_context_to_dynamic(&TimelineActionContext { uri: self.uri.borrow().clone(), item: item.clone() })));
        template.action_bar.set_action_runner(TimelineActionRunner::new(self.get_selection.clone()));
        template.action_bar.push(&self.commands.get_item_actions(&item, selection.as_deref()), IActionOptions { item: IActionViewItemOptions { icon: Some(true), label: Some(false), ..IActionViewItemOptions::default() }, index: None });
        template.action_bar.layout();

        let editable = self.timeline_service.editable();
        let editable_data = item.timeline_item().and_then(|timeline_item| {
            let timeline_item = timeline_item.borrow();
            editable.filter(|editable| editable.item.source == timeline_item.source && editable.item.handle == timeline_item.handle).map(|editable| editable.data)
        });
        {
            let mut tree = ui.tree_mut();
            tree.toggle_class(template.container, "timeline-item-editing", editable_data.is_some());
            for element in [template.icon_label.element(), template.timestamp_container] {
                tree.style_mut(element).visible = editable_data.is_none();
            }
        }
        if let Some(editable_data) = editable_data {
            let editor = InlineEditInputBox::new(
                ui,
                template.container,
                self.services.context_view_provider.clone(),
                self.context_menu_service.clone(),
                (self.services.input_box_styles)(),
                editable_data,
                InlineEditInputBoxOptions {
                    aria_label: localize("timelineInputAriaLabel", "Type timeline item name. Press Enter to confirm or Escape to cancel.", &[]),
                    value: None,
                    selection: None,
                },
            );
            template.element_disposables.add(editor.clone());
            *template.editor.borrow_mut() = Some(editor);
        }
        template.update_actions_visibility();

        // If we are rendering the load more item, we've scrolled to the end, so trigger an event
        if let Some(load_more) = item.load_more_command() {
            let (emitter, load_more) = (self.on_did_scroll_to_end.clone(), load_more.clone());
            runtime::set_timeout(move || emitter.fire(load_more), 0);
        }
    }

    fn dispose_element(&self, _node: &Rc<TimelineNode>, _index: i64, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        let template = Self::template(template_data);
        template.element_disposables.clear();
        template.action_bar.action_runner().dispose();
    }

    fn dispose_template(&self, template_data: &TemplateData) {
        Self::template(template_data).template_disposables.dispose();
    }
}

// Os comandos do painel

/// `registerIcon('timeline-refresh', Codicon.refresh, ...)`.
fn timeline_refresh_icon() -> ThemeIcon {
    ThemeIcon::from_id("timeline-refresh")
}

/// `registerIcon('timeline-pin', Codicon.pin, ...)`.
fn timeline_pin_icon() -> ThemeIcon {
    ThemeIcon::from_id("timeline-pin")
}

/// `registerIcon('timeline-unpin', Codicon.pinned, ...)`.
fn timeline_unpin_icon() -> ThemeIcon {
    ThemeIcon::from_id("timeline-unpin")
}

/// O comando que alterna o `followActiveEditor`.
const TOGGLE_FOLLOW_ACTIVE_EDITOR_COMMAND_ID: &str = "timeline.toggleFollowActiveEditor";

/// `localize2(key, message)` como título de comando.
fn localized_title(key: &str, message: &str) -> Title {
    let localized = localize2(key, message);
    Title::Localized(CommandActionTitle { value: localized.value, original: localized.original, mnemonic_title: None })
}

/// `category: localize2('timeline', "Timeline")`.
fn timeline_category() -> Category {
    Category::Localized(localize2("timeline", "Timeline"))
}

/// O item de menu de uma ação: `{ id, group, order?, when? }`.
fn action_menu_item(id: MenuId, group: &str, order: Option<f64>, when: Option<ContextKeyExpression>) -> Action2MenuItem {
    Action2MenuItem { id, null_precondition: false, alt: None, when, group: Some(group.to_owned()), order, is_hidden_by_default: None }
}

/// As três ações do painel que só chamam o painel (`timeline.refresh`, `timeline.showWorkspaceTimeline` e
/// `timeline.compareSelected`).
struct PaneCommandAction {
    pane: Weak<dyn ITimelinePaneCommandsHost>,
    options: Action2Options,
    run: fn(&dyn ITimelinePaneCommandsHost),
}

impl Action2 for PaneCommandAction {
    fn desc(&self) -> Action2Options {
        self.options.clone()
    }

    fn run(&self, _accessor: &ServicesAccessor, _args: &[Dynamic]) {
        if let Some(pane) = self.pane.upgrade() {
            (self.run)(&*pane);
        }
    }
}

/// `timeline.toggleExcludeSource:<id>`: liga e desliga uma fonte no filtro.
struct ToggleExcludeSourceAction {
    source: TimelineSource,
    /// O `excluded` do original: compartilhado entre as ações e mantido entre as execuções.
    excluded: Rc<RefCell<Vec<String>>>,
}

impl Action2 for ToggleExcludeSourceAction {
    fn desc(&self) -> Action2Options {
        let pattern = format!("\\b{}\\b", escape_reg_exp_characters(&self.source.id.encode_utf16().collect::<Vec<u16>>()).to_string_lossy());
        let regex = ContextRegExp::new(&pattern, "").expect("o texto escapado é uma expressão regular válida");
        Action2Options {
            command: CommandAction {
                toggled: Some(Toggled::Expression(ContextKeyExpr::regex(timeline_exclude_sources().key(), regex).negate())),
                ..CommandAction::new(format!("timeline.toggleExcludeSource:{}", self.source.id), Title::Plain(self.source.label.clone()))
            },
            menu: vec![action_menu_item(menu_ids::timeline_filter_sub_menu(), "navigation", None, None)],
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let text = {
            let mut excluded = self.excluded.borrow_mut();
            match excluded.iter().position(|id| *id == self.source.id) {
                Some(index) => {
                    excluded.remove(index);
                }
                None => excluded.push(self.source.id.clone()),
            }
            json_stringify(&Dynamic::array(excluded.iter().map(|id| Dynamic::string(id)))).expect("uma lista de textos tem representação JSON")
        };

        accessor.get(I_STORAGE_SERVICE).store(EXCLUDE_SOURCES_KEY, StorageValue::String(text), StorageScope::Profile, StorageTarget::User);
    }
}

/// O contexto de um item para o menu: `view`, o `contextValue` e o que a seleção diz (os atalhos da lista não valem para itens
/// fora da seleção, então ficam por item).
fn item_context_keys(element: &TreeElement, selection: Option<&[TimelineItemRef]>) -> Vec<(String, Dynamic)> {
    vec![
        ("timelineItem".to_owned(), element.context_value().map_or(Dynamic::Undefined, |value| Dynamic::string(&value))),
        (workbench_list_multi_selection().key().to_owned(), Dynamic::Bool(selection.is_some())),
        (workbench_list_double_selection().key().to_owned(), Dynamic::Bool(selection.is_some_and(|selection| selection.len() == 2))),
        (
            timeline_can_compare_selection_context().key().to_owned(),
            Dynamic::Bool(selection.is_some_and(|selection| selection.len() == 2 && selection.iter().all(|item| item.borrow().resource_uri.is_some()))),
        ),
    ]
}

/// `TimelinePaneCommands`.
pub struct TimelinePaneCommands {
    base: Rc<Disposable>,
    source_disposables: Rc<DisposableStore>,
    pane: Weak<dyn ITimelinePaneCommandsHost>,
    timeline_service: Rc<dyn ITimelineService>,
    storage_service: Rc<dyn StorageService>,
    context_key_service: Rc<dyn IContextKeyService>,
    menu_service: Rc<dyn IMenuService>,
    weak_self: Weak<TimelinePaneCommands>,
}

impl TimelinePaneCommands {
    pub fn new(
        pane: Weak<dyn ITimelinePaneCommandsHost>,
        timeline_service: Rc<dyn ITimelineService>,
        storage_service: Rc<dyn StorageService>,
        context_key_service: Rc<dyn IContextKeyService>,
        menu_service: Rc<dyn IMenuService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let source_disposables = DisposableStore::new();
        base.register(source_disposables.clone());

        let title_menu = menu_ids::timeline_title();
        let pane_action = |command: CommandAction, menu: Action2MenuItem, run: fn(&dyn ITimelinePaneCommandsHost)| {
            register_action2(Rc::new(PaneCommandAction { pane: pane.clone(), options: Action2Options { command, menu: vec![menu], keybinding: Vec::new(), f1: false }, run }))
        };

        base.register(pane_action(
            CommandAction { icon: Some(Icon::Theme(timeline_refresh_icon())), category: Some(timeline_category()), ..CommandAction::new("timeline.refresh", localized_title("refresh", "Refresh")) },
            action_menu_item(title_menu.clone(), "navigation", Some(99.0), None),
            |pane| pane.reset(),
        ));

        base.register(pane_action(
            CommandAction { category: Some(timeline_category()), ..CommandAction::new("timeline.showWorkspaceTimeline", localized_title("timeline.showWorkspaceTimeline", "Pin Workspace Timeline")) },
            action_menu_item(
                title_menu.clone(),
                "1_workspace",
                None,
                ContextKeyExpr::and([timeline_has_workspace_provider_context().expr(), timeline_for_workspace_context().to_negated()]),
            ),
            |pane| pane.show_workspace_timeline(),
        ));

        base.register(pane_action(
            CommandAction::new("timeline.compareSelected", localized_title("timeline.compareSelected", "Compare Selected")),
            action_menu_item(menu_ids::timeline_item_context(), "3_compare", Some(0.0), Some(timeline_can_compare_selection_context().expr())),
            |pane| {
                let _ = pane.compare_selected();
            },
        ));

        let toggle_pane = pane.clone();
        let toggle: CommandHandler = Rc::new(move |_accessor, _args| toggle_pane.upgrade().map_or(Dynamic::Undefined, |pane| Dynamic::Bool(pane.toggle_follow_active_editor())));
        base.register(commands_registry().register_command_handler(TOGGLE_FOLLOW_ACTIVE_EDITOR_COMMAND_ID, Some(toggle)));

        for (when, title_key, title_message, icon) in [
            (timeline_follow_active_editor_context().expr(), "timeline.toggleFollowActiveEditorCommand.follow", "Pin Current Timeline", timeline_pin_icon()),
            (timeline_follow_active_editor_context().to_negated(), "timeline.toggleFollowActiveEditorCommand.unfollow", "Unpin Current Timeline", timeline_unpin_icon()),
        ] {
            base.register(menu_registry().append_menu_item(
                &title_menu,
                AnyMenuItem::Menu(MenuItem {
                    group: Some("navigation".to_owned()),
                    order: Some(98.0),
                    when: Some(when),
                    ..MenuItem::new(CommandAction {
                        icon: Some(Icon::Theme(icon)),
                        category: Some(timeline_category()),
                        ..CommandAction::new(TOGGLE_FOLLOW_ACTIVE_EDITOR_COMMAND_ID, localized_title(title_key, title_message))
                    })
                }),
            ));
        }

        let this = Rc::new_cyclic(|weak| Self { base, source_disposables, pane, timeline_service, storage_service, context_key_service, menu_service, weak_self: weak.clone() });

        let weak = this.weak_self.clone();
        this.base.register(this.timeline_service.on_did_change_providers().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_timeline_source_filters();
            }
        }));
        this.update_timeline_source_filters();

        this
    }

    /// `getItemActions(element, selection)`: `selection` são os itens selecionados quando `element` faz parte de uma seleção
    /// múltipla.
    pub fn get_item_actions(&self, element: &TreeElement, selection: Option<&[TimelineItemRef]>) -> Vec<ActionRef> {
        self.get_actions(&menu_ids::timeline_item_context(), item_context_keys(element, selection)).primary
    }

    /// `getItemContextActions(element, selection)`.
    pub fn get_item_context_actions(&self, element: &TreeElement, selection: Option<&[TimelineItemRef]>) -> Vec<ActionRef> {
        self.get_actions(&menu_ids::timeline_item_context(), item_context_keys(element, selection)).secondary
    }

    fn get_actions(&self, menu_id: &MenuId, context: Vec<(String, Dynamic)>) -> PrimaryAndSecondaryActions {
        let pane_id = self.pane.upgrade().map(|pane| pane.id()).expect("o painel vive enquanto os comandos são usados");
        let mut overlay = vec![("view".to_owned(), Dynamic::string(&pane_id))];
        overlay.extend(context);
        let context_key_service = self.context_key_service.create_overlay(overlay);

        let menu = self.menu_service.get_menu_actions(menu_id, &*context_key_service, Some(&MenuActionOptions { should_forward_args: true, ..MenuActionOptions::default() }));
        get_context_menu_actions(&menu, Some("inline"))
    }

    fn update_timeline_source_filters(&self) {
        self.source_disposables.clear();

        let (_, excluded) = read_excluded_sources(self.storage_service.as_ref());
        let excluded = Rc::new(RefCell::new(excluded));
        for source in self.timeline_service.get_sources() {
            self.source_disposables.add(register_action2(Rc::new(ToggleExcludeSourceAction { source, excluded: excluded.clone() })));
        }
    }
}

impl IDisposable for TimelinePaneCommands {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "TimelinePaneCommands"
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
