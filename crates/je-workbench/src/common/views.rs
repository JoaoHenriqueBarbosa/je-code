//! Porte parcial de `src/vs/workbench/common/views.ts`: os tipos da árvore de visões que o host de extensões
//! (`extHostTreeViews.ts`) e o protocolo (`extHost.protocol.ts`) usam: `ITreeItem`, `TreeItemCollapsibleState`,
//! `ITreeItemLabel`, `TreeCommand`, `ITreeItemCheckboxState`, `IRevealOptions`, `IViewBadge` e o `NoTreeViewError`.
//!
//! Divergências:
//! - É um porte parcial: o resto de `views.ts` (`ICustomViewDescriptor`, `IEditableData`...) é da camada de interface e
//!   entra junto do porte dela. O `ITreeView`, o `ITreeViewDataProvider` e o `ITreeViewDragAndDropController` estão em
//!   `crate::common::tree_view_contracts` e o `ResolvableTreeItem` está neste arquivo. `TreeViewPaneHandleArg` e `TreeViewItemHandleArg` (os `$treeViewId` e
//!   `$treeItemHandle` que o `processArgument` do host lê) chegam como [`Dynamic`], então não têm struct aqui.
//! - Os registros e os descritores (`ViewContainersRegistry`, `ViewsRegistry`, `IViewDescriptor`, `IViewContainerModel`,
//!   `IViewDescriptorService`...) estão no bloco final do arquivo, com as suas próprias divergências.
//! - `UriComponents` é [`URI`], `ThemeIcon` é o de `je-base`, `IMarkdownString | string` é [`StringOrMarkdown`] e o
//!   `Command & { originalId?: string }` de `TreeCommand` é o [`ICommandDto`] do protocolo mais `original_id`.
//! - `IAccessibilityInformation` (`platform/accessibility/common/accessibility.ts`) é definido aqui, como
//!   [`AccessibilityInformation`], até esse arquivo ser portado.
//! - `ITreeItem.collapsibleState` é o enum [`TreeItemCollapsibleState`]. O original aceita qualquer número que a
//!   extensão devolver; o host de Rust lê um valor que não seja `0`, `1` ou `2` como `None`.
//! - `ITreeItem.description` é `string | boolean` ([`TreeItemDescription`]) e `IRevealOptions.expand` é
//!   `boolean | number` ([`RevealExpand`]).
//! - [`ITreeItem::to_dynamic`] não existe no original: é o objeto JavaScript do item sem as propriedades
//!   `undefined`, que é o que o `removeUnsetKeys` da suíte de testes calcula (e o que o RPC serializaria). Os
//!   `UriComponents` e o `ThemeIcon` saem como os valores de classe de `ext_host_values`.

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::themables::ThemeIcon;
use je_base::uri::URI;
use je_editor::languages::StringOrMarkdown;

use crate::api::common::ext_host_protocol::ICommandDto;
use crate::api::common::ext_host_type_converters::MarkdownString as MarkdownStringConverter;
use crate::api::common::ext_host_values::ApiValue;

/// `IViewBadge`.
#[derive(Clone, Debug, PartialEq)]
pub struct IViewBadge {
    pub tooltip: String,
    pub value: f64,
}

/// `IAccessibilityInformation`: `{ label: string; role?: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessibilityInformation {
    pub label: String,
    pub role: Option<String>,
}

/// `IRevealOptions.expand`: `boolean | number`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RevealExpand {
    Bool(bool),
    Depth(f64),
}

/// `IRevealOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IRevealOptions {
    pub select: Option<bool>,
    pub focus: Option<bool>,
    pub expand: Option<RevealExpand>,
}

/// `TreeItemCollapsibleState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum TreeItemCollapsibleState {
    #[default]
    None = 0,
    Collapsed = 1,
    Expanded = 2,
}

impl TreeItemCollapsibleState {
    /// O valor numérico do membro.
    pub fn value(self) -> i32 {
        self as i32
    }

    /// O membro de um valor numérico, se existir.
    pub fn from_value(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Collapsed),
            2 => Some(Self::Expanded),
            _ => None,
        }
    }
}

/// `ITreeItemLabel`.
#[derive(Clone, Debug)]
pub struct ITreeItemLabel {
    pub label: StringOrMarkdown,
    pub highlights: Option<Vec<(f64, f64)>>,
}

/// `TreeCommand`: `Command & { originalId?: string }`.
#[derive(Clone, Debug)]
pub struct TreeCommand {
    pub command: ICommandDto,
    pub original_id: Option<String>,
}

/// `ITreeItemCheckboxState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITreeItemCheckboxState {
    pub is_checked: bool,
    pub tooltip: Option<String>,
    pub accessibility_information: Option<AccessibilityInformation>,
}

/// `ITreeItem.description`: `string | boolean`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeItemDescription {
    Text(String),
    Bool(bool),
}

/// `ITreeItem`.
#[derive(Clone, Debug, Default)]
pub struct ITreeItem {
    pub handle: String,
    pub parent_handle: Option<String>,
    pub collapsible_state: TreeItemCollapsibleState,
    pub label: Option<ITreeItemLabel>,
    pub description: Option<TreeItemDescription>,
    pub icon: Option<URI>,
    pub icon_dark: Option<URI>,
    pub theme_icon: Option<ThemeIcon>,
    pub resource_uri: Option<URI>,
    pub tooltip: Option<StringOrMarkdown>,
    pub context_value: Option<String>,
    pub command: Option<TreeCommand>,
    pub children: Option<Vec<ITreeItem>>,
    pub parent: Option<Box<ITreeItem>>,
    pub accessibility_information: Option<AccessibilityInformation>,
    pub checkbox: Option<ITreeItemCheckboxState>,
    /// As propriedades que o objeto `ITreeItem` do original carrega além das conhecidas (o `mixin` do
    /// `ResolvableTreeItem` as copia todas): chave e valor, na ordem em que o objeto as recebeu.
    pub custom_properties: Vec<(String, Dynamic)>,
}

/// O objeto JavaScript só com as propriedades definidas (uma propriedade `undefined` não existe).
fn object_of(entries: Vec<(&str, Option<Dynamic>)>) -> Dynamic {
    Dynamic::object(entries.into_iter().filter_map(|(key, value)| value.map(|value| (key, value))))
}

fn string_or_markdown_to_dynamic(value: &StringOrMarkdown) -> Dynamic {
    match value {
        StringOrMarkdown::String(text) => Dynamic::string(text),
        StringOrMarkdown::Markdown(markdown) => MarkdownStringConverter::to_dto(markdown),
    }
}

fn accessibility_to_dynamic(information: &AccessibilityInformation) -> Dynamic {
    object_of(vec![("label", Some(Dynamic::string(&information.label))), ("role", information.role.as_deref().map(Dynamic::string))])
}

impl TreeCommand {
    /// O objeto do comando: o [`ICommandDto`] (com o `$ident`) e o `originalId`.
    pub fn to_dynamic(&self) -> Dynamic {
        object_of(vec![
            ("$ident", self.command.ident.as_deref().map(Dynamic::string)),
            ("id", Some(Dynamic::string(&self.command.id))),
            ("title", Some(Dynamic::string(&self.command.title))),
            ("tooltip", self.command.tooltip.as_deref().map(Dynamic::string)),
            ("arguments", self.command.arguments.as_ref().map(|arguments| Dynamic::array(arguments.iter().cloned()))),
            ("originalId", self.original_id.as_deref().map(Dynamic::string)),
        ])
    }
}

impl ITreeItemLabel {
    fn to_dynamic(&self) -> Dynamic {
        object_of(vec![
            ("label", Some(string_or_markdown_to_dynamic(&self.label))),
            (
                "highlights",
                self.highlights.as_ref().map(|highlights| {
                    Dynamic::array(highlights.iter().map(|(start, end)| Dynamic::array([Dynamic::Number(*start), Dynamic::Number(*end)])))
                }),
            ),
        ])
    }
}

impl ITreeItem {
    /// O item como objeto JavaScript, sem as propriedades `undefined` (ver as divergências do módulo).
    pub fn to_dynamic(&self) -> Dynamic {
        let mut entries = vec![
            ("handle", Some(Dynamic::string(&self.handle))),
            ("parentHandle", self.parent_handle.as_deref().map(Dynamic::string)),
            ("collapsibleState", Some(Dynamic::from(self.collapsible_state.value()))),
            ("label", self.label.as_ref().map(ITreeItemLabel::to_dynamic)),
            (
                "description",
                self.description.as_ref().map(|description| match description {
                    TreeItemDescription::Text(text) => Dynamic::string(text),
                    TreeItemDescription::Bool(flag) => Dynamic::Bool(*flag),
                }),
            ),
            ("icon", self.icon.as_ref().map(ApiValue::to_dynamic)),
            ("iconDark", self.icon_dark.as_ref().map(ApiValue::to_dynamic)),
            ("themeIcon", self.theme_icon.as_ref().map(ApiValue::to_dynamic)),
            ("resourceUri", self.resource_uri.as_ref().map(ApiValue::to_dynamic)),
            ("tooltip", self.tooltip.as_ref().map(string_or_markdown_to_dynamic)),
            ("contextValue", self.context_value.as_deref().map(Dynamic::string)),
            ("command", self.command.as_ref().map(TreeCommand::to_dynamic)),
            ("children", self.children.as_ref().map(|children| Dynamic::array(children.iter().map(ITreeItem::to_dynamic)))),
            ("parent", self.parent.as_ref().map(|parent| parent.to_dynamic())),
            ("accessibilityInformation", self.accessibility_information.as_ref().map(accessibility_to_dynamic)),
            (
                "checkbox",
                self.checkbox.as_ref().map(|checkbox| {
                    object_of(vec![
                        ("isChecked", Some(Dynamic::Bool(checkbox.is_checked))),
                        ("tooltip", checkbox.tooltip.as_deref().map(Dynamic::string)),
                        ("accessibilityInformation", checkbox.accessibility_information.as_ref().map(accessibility_to_dynamic)),
                    ])
                }),
            ),
        ];
        entries.extend(self.custom_properties.iter().map(|(key, value)| (key.as_str(), Some(value.clone()))));
        object_of(entries)
    }
}

/// O nome do `NoTreeViewError`.
pub const NO_TREE_VIEW_ERROR_NAME: &str = "NoTreeViewError";

/// `new NoTreeViewError(treeViewId)`.
pub fn no_tree_view_error(tree_view_id: &str) -> Error {
    Error::named(NO_TREE_VIEW_ERROR_NAME, localize("treeView.notRegistered", "No tree view with id '{0}' registered.", &[tree_view_id]))
}

/// `NoTreeViewError.is(err)`.
pub fn is_no_tree_view_error(error: &Error) -> bool {
    error.name == NO_TREE_VIEW_ERROR_NAME
}

/// O `resolve` que o `ResolvableTreeItem` recebe: pede ao host de extensões o item resolvido (`$resolve`).
pub type TreeItemResolver = Rc<dyn Fn(CancellationToken) -> Promise<Option<ITreeItem>>>;

/// `ResolvableTreeItem`: o item de árvore que resolve o tooltip e o comando sob demanda.
///
/// Divergências:
/// - O original é um objeto que implementa `ITreeItem` (copia as propriedades do item com `mixin`) e é mutado por
///   identidade. Aqui o item vive num `RefCell` dentro de um `Rc`: quem guarda a identidade guarda o `Rc`, e os valores
///   que cruzam o contrato [`ITreeItem`] são cópias ([`ResolvableTreeItem::to_item`]).
/// - `resolve` é o método [`ResolvableTreeItem::resolve`] (o campo de função do original).
pub struct ResolvableTreeItem {
    item: RefCell<ITreeItem>,
    resolve_fn: Option<TreeItemResolver>,
    resolved: Cell<bool>,
}

impl ResolvableTreeItem {
    /// `new ResolvableTreeItem(treeItem, resolve?)`.
    pub fn new(tree_item: ITreeItem, resolve: Option<TreeItemResolver>) -> Rc<Self> {
        Rc::new(Self { item: RefCell::new(tree_item), resolve_fn: resolve, resolved: Cell::new(false) })
    }

    /// `hasResolve`.
    pub fn has_resolve(&self) -> bool {
        self.resolve_fn.is_some()
    }

    /// `resetResolve()`.
    pub fn reset_resolve(&self) {
        self.resolved.set(false);
    }

    /// `resolve(token)`: resolve o tooltip e o comando uma única vez (só se não foi cancelado).
    pub fn resolve(self: &Rc<Self>, token: CancellationToken) -> Promise<()> {
        let this = Rc::clone(self);
        async_fn(async move {
            if let Some(resolve) = &this.resolve_fn
                && !this.resolved.get()
            {
                let resolved_item = resolve(token.clone()).await?;
                if let Some(resolved_item) = resolved_item {
                    // Resolvable elements. Currently tooltip and command.
                    let mut item = this.item.borrow_mut();
                    if item.tooltip.is_none() {
                        item.tooltip = resolved_item.tooltip;
                    }
                    if item.command.is_none() {
                        item.command = resolved_item.command;
                    }
                }
            }
            if !token.is_cancellation_requested() {
                this.resolved.set(true);
            }
            Ok(())
        })
    }

    /// O item inteiro (as propriedades que o objeto do original carrega: `children`, `parent`, `checkbox`...).
    pub fn to_item(&self) -> ITreeItem {
        self.item.borrow().clone()
    }

    /// `asTreeItem()`: só as propriedades de `ITreeItem` que o original copia (sem `parent`, `checkbox` e as
    /// propriedades extras).
    pub fn as_tree_item(&self) -> ITreeItem {
        let item = self.item.borrow();
        ITreeItem {
            handle: item.handle.clone(),
            parent_handle: item.parent_handle.clone(),
            collapsible_state: item.collapsible_state,
            label: item.label.clone(),
            description: item.description.clone(),
            icon: item.icon.clone(),
            icon_dark: item.icon_dark.clone(),
            theme_icon: item.theme_icon.clone(),
            resource_uri: item.resource_uri.clone(),
            tooltip: item.tooltip.clone(),
            context_value: item.context_value.clone(),
            command: item.command.clone(),
            children: item.children.clone(),
            accessibility_information: item.accessibility_information.clone(),
            ..ITreeItem::default()
        }
    }

    /// Muda o item no lugar (a escrita de propriedade do objeto do original).
    pub fn update<R>(&self, change: impl FnOnce(&mut ITreeItem) -> R) -> R {
        change(&mut self.item.borrow_mut())
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Registros e descritores de `views.ts`: `ViewContainersRegistry`, `ViewsRegistry`, `IViewContainerDescriptor`,
// `IViewDescriptor`, `IViewContainerModel`, `IViewDescriptorService` e o que os cerca.
//
// Divergências deste bloco:
// - **Registros.** `Registry.add(Extensions.ViewContainersRegistry, new ViewContainersRegistryImpl())` e
//   `Registry.add(Extensions.ViewsRegistry, new ViewsRegistry())`, que o módulo do original executa ao ser carregado, são
//   feitos na primeira chamada de [`view_containers_registry`] e de [`views_registry`] (o `Registry.as(...)` do original),
//   que devolvem os registros concretos: a interface (`IViewContainersRegistry`, `IViewsRegistry`) e a sua única
//   implementação são a mesma struct, então os aliases [`IViewContainersRegistry`] e [`IViewsRegistry`] nomeiam o tipo.
// - **Identidade.** `ViewContainer` e `IViewDescriptor` são objetos comparados por identidade no original (chaves de `Map`,
//   `includes`). Os dois viram `Rc` ([`ViewContainerRc`], [`ViewDescriptorRc`]) e a identidade é `Rc::ptr_eq`.
//   `ViewContainer` não é o próprio descritor: [`ViewContainer`] guarda o [`IViewContainerDescriptor`] recebido por
//   `registerViewContainer` (com o `openCommandActionDescriptor` já resolvido) e o `requestedIndex` mutável.
// - **`ctorDescriptor`.** `SyncDescriptor<IViewPaneContainer>` e `SyncDescriptor<IView>` são `Option` porque o teste do
//   `ViewContainerModel` passa `new SyncDescriptor(<any>{})` e `null!`, e porque o `ViewPaneContainer` (camada de
//   interface, `browser/parts/views/viewPaneContainer.ts`) ainda não foi portado.
// - **`ThemeIcon | URI`** é [`ViewIcon`]; `string | string[]` do `remoteAuthority` é [`RemoteAuthority`];
//   `ContextKeyExpression | 'default'` do `when` do conteúdo de boas-vindas é [`ViewContentWhen`].
// - **`defaultViewIcon`** (`registerIcon('default-view-icon', Codicon.window, ...)`) é o `ThemeIcon` de id
//   `default-view-icon`, que é o que `registerIcon` devolve (o registro de ícones ainda não foi portado).
// - **`getViewContainerLocation` e `getDefaultViewContainerLocation`** devolvem `Option` (o `| null` da interface; o
//   `undefined` do registro para um contêiner que não está registrado).
// - **Não portados** (dependem de interfaces da camada de UI): `ICustomViewDescriptor` e `IEditableData`. O `ITreeView`, o
//   `ITreeViewDataProvider` e o `ITreeViewDragAndDropController` estão em `crate::common::tree_view_contracts`; o
//   `ResolvableTreeItem` está acima; o `ITreeViewDescriptor` é o campo `tree_view` do [`IViewDescriptor`].
// ---------------------------------------------------------------------------------------------------------------------

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::collator::locale_compare;
use je_base::errors;
use je_base::event::{Emitter, Event};
use je_base::html_content::MarkdownString;
use je_base::lifecycle::{ById, Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::map::SetMap;
use je_base::nls::LocalizedString;
use je_editor::code_action::platform::IProgressIndicator;
use je_extensions::manifest::ExtensionIdentifier;
use je_instantiation::{SyncDescriptor, delayed_proxy, service_id};
use je_platform::contextkey::ContextKeyExpression;
use je_platform::keybinding::keybindings_registry::Keybindings;
use je_platform::registry::registry;

use crate::common::tree_view_contracts::ITreeView;

/// `VIEWS_LOG_ID`.
pub const VIEWS_LOG_ID: &str = "views";

/// `VIEWS_LOG_NAME`.
pub fn views_log_name() -> String {
    localize("views log", "Views", &[])
}

/// `defaultViewIcon`.
pub fn default_view_icon() -> ThemeIcon {
    ThemeIcon::from_id("default-view-icon")
}

/// `namespace Extensions`: os ids dos registros no `Registry`.
pub struct Extensions;

impl Extensions {
    pub const VIEW_CONTAINERS_REGISTRY: &'static str = "workbench.registry.view.containers";
    pub const VIEWS_REGISTRY: &'static str = "workbench.registry.view";
}

/// `ViewContainerLocation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ViewContainerLocation {
    Sidebar = 0,
    Panel = 1,
    AuxiliaryBar = 2,
}

impl ViewContainerLocation {
    /// O valor numérico do membro.
    pub fn value(self) -> i32 {
        self as i32
    }

    /// O membro de um valor numérico, se existir.
    pub fn from_value(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Sidebar),
            1 => Some(Self::Panel),
            2 => Some(Self::AuxiliaryBar),
            _ => None,
        }
    }
}

/// `ViewContainerLocationToString`.
pub fn view_container_location_to_string(view_container_location: ViewContainerLocation) -> &'static str {
    match view_container_location {
        ViewContainerLocation::Sidebar => "sidebar",
        ViewContainerLocation::Panel => "panel",
        ViewContainerLocation::AuxiliaryBar => "auxiliarybar",
    }
}

/// `WindowEnablement`: em qual janela uma view ou um contêiner de views fica visível.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum WindowEnablement {
    /// Visible only in the editor window.
    Editor = 1,
    /// Visible only in agent sessions window.
    Sessions = 2,
    /// Visible in both editor and agent sessions windows.
    Both = 3,
}

/// `ThemeIcon | URI`.
#[derive(Clone, Debug)]
pub enum ViewIcon {
    Theme(ThemeIcon),
    Uri(URI),
}

/// `string | ILocalizedString` do `title` de [`OpenCommandActionDescriptor`].
#[derive(Clone, Debug)]
pub enum OpenCommandTitle {
    Plain(String),
    Localized(LocalizedString),
}

/// `IKeybindings & { when?: ContextKeyExpression }`.
#[derive(Clone)]
pub struct OpenCommandKeybindings {
    pub keybindings: Keybindings,
    pub when: Option<ContextKeyExpression>,
}

/// `OpenCommandActionDescriptor`.
#[derive(Clone)]
pub struct OpenCommandActionDescriptor {
    pub id: String,
    pub title: Option<OpenCommandTitle>,
    pub mnemonic_title: Option<String>,
    pub order: Option<f64>,
    pub keybindings: Option<OpenCommandKeybindings>,
}

impl OpenCommandActionDescriptor {
    /// `{ id }`.
    pub fn new(id: &str) -> Self {
        Self { id: id.to_string(), title: None, mnemonic_title: None, order: None, keybindings: None }
    }
}

/// `ViewOrderDelegate`.
pub trait ViewOrderDelegate {
    fn get_order(&self, group: Option<&str>) -> Option<f64>;
}

/// `IViewContainerDescriptor`.
#[derive(Clone)]
pub struct IViewContainerDescriptor {
    /// The id of the view container.
    pub id: String,
    /// The title of the view container.
    pub title: LocalizedString,
    /// Icon representation of the View container.
    pub icon: Option<ViewIcon>,
    /// Order of the view container.
    pub order: Option<f64>,
    /// IViewPaneContainer Ctor to instantiate.
    pub ctor_descriptor: Option<SyncDescriptor<dyn IViewPaneContainer>>,
    /// Descriptor for open view container command. If not provided, view container info (id, title) is used.
    pub open_command_action_descriptor: Option<OpenCommandActionDescriptor>,
    /// Storage id to use to store the view container state. If not provided, it will be derived.
    pub storage_id: Option<String>,
    /// If enabled, view container is not shown if it has no active views.
    pub hide_if_empty: bool,
    /// Id of the extension that contributed the view container.
    pub extension_id: Option<ExtensionIdentifier>,
    pub always_use_container_info: bool,
    pub view_order_delegate: Option<Rc<dyn ViewOrderDelegate>>,
    pub reject_added_views: bool,
    /// Specifies in which window this view container should be enabled. Defaults to `WindowEnablement::Editor`.
    pub window_enablement: Option<WindowEnablement>,
    /// O valor inicial do `requestedIndex` (ver [`ViewContainer::requested_index`]).
    pub requested_index: Option<usize>,
}

impl IViewContainerDescriptor {
    /// O descritor com só o `id` e o `title`, que são os obrigatórios.
    pub fn new(id: &str, title: LocalizedString) -> Self {
        Self {
            id: id.to_string(),
            title,
            icon: None,
            order: None,
            ctor_descriptor: None,
            open_command_action_descriptor: None,
            storage_id: None,
            hide_if_empty: false,
            extension_id: None,
            always_use_container_info: false,
            view_order_delegate: None,
            reject_added_views: false,
            window_enablement: None,
            requested_index: None,
        }
    }
}

/// `ViewContainer` (`interface ViewContainer extends IViewContainerDescriptor`): o descritor registrado. Lê os campos do
/// descritor por `Deref`; o `openCommandActionDescriptor` é o resolvido pelo registro e o `requestedIndex` é mutável.
pub struct ViewContainer {
    descriptor: IViewContainerDescriptor,
    requested_index: Cell<Option<usize>>,
}

impl ViewContainer {
    /// O contêiner do descritor, com o `openCommandActionDescriptor` que o registro resolveu.
    fn new(mut descriptor: IViewContainerDescriptor, open_command_action_descriptor: Option<OpenCommandActionDescriptor>) -> Rc<Self> {
        descriptor.open_command_action_descriptor = open_command_action_descriptor;
        let requested_index = Cell::new(descriptor.requested_index);
        Rc::new(Self { descriptor, requested_index })
    }

    /// `requestedIndex`.
    pub fn requested_index(&self) -> Option<usize> {
        self.requested_index.get()
    }

    /// `viewContainer.requestedIndex = requestedIndex`.
    pub fn set_requested_index(&self, requested_index: Option<usize>) {
        self.requested_index.set(requested_index);
    }
}

impl Deref for ViewContainer {
    type Target = IViewContainerDescriptor;

    fn deref(&self) -> &IViewContainerDescriptor {
        &self.descriptor
    }
}

/// O `ViewContainer` do original, por identidade.
pub type ViewContainerRc = Rc<ViewContainer>;

/// `{ viewContainer: ViewContainer; viewContainerLocation: ViewContainerLocation }`: o payload de `onDidRegister` e
/// `onDidDeregister`.
#[derive(Clone)]
pub struct ViewContainerRegistration {
    pub view_container: ViewContainerRc,
    pub view_container_location: ViewContainerLocation,
}

/// O `options` de `registerViewContainer`.
#[derive(Clone, Copy, Debug, Default)]
pub struct RegisterViewContainerOptions {
    pub is_default: bool,
    pub do_not_register_open_command: bool,
}

/// `IViewContainersRegistry`.
pub type IViewContainersRegistry = ViewContainersRegistry;

/// `ViewContainersRegistryImpl`.
pub struct ViewContainersRegistry {
    base: Rc<Disposable>,
    on_did_register: Rc<Emitter<ViewContainerRegistration>>,
    on_did_deregister: Rc<Emitter<ViewContainerRegistration>>,
    /// `Map<ViewContainerLocation, ViewContainer[]>`, na ordem de inserção.
    view_containers: RefCell<Vec<(ViewContainerLocation, Vec<ViewContainerRc>)>>,
    default_view_containers: RefCell<Vec<ViewContainerRc>>,
}

impl ViewContainersRegistry {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_register = Emitter::new();
        base.register(on_did_register.clone());
        let on_did_deregister = Emitter::new();
        base.register(on_did_deregister.clone());
        Rc::new(Self {
            base,
            on_did_register,
            on_did_deregister,
            view_containers: RefCell::new(Vec::new()),
            default_view_containers: RefCell::new(Vec::new()),
        })
    }

    /// An event that is triggered when a view container is registered.
    pub fn on_did_register(&self) -> Event<ViewContainerRegistration> {
        self.on_did_register.event()
    }

    /// An event that is triggered when a view container is deregistered.
    pub fn on_did_deregister(&self) -> Event<ViewContainerRegistration> {
        self.on_did_deregister.event()
    }

    /// All registered view containers.
    pub fn all(&self) -> Vec<ViewContainerRc> {
        self.view_containers.borrow().iter().flat_map(|(_, containers)| containers.iter().cloned()).collect()
    }

    /// Registers a view container to given location. No op if a view container is already registered.
    pub fn register_view_container(
        &self,
        view_container_descriptor: IViewContainerDescriptor,
        view_container_location: ViewContainerLocation,
        options: Option<RegisterViewContainerOptions>,
    ) -> ViewContainerRc {
        if let Some(existing) = self.get(&view_container_descriptor.id) {
            return existing;
        }

        let options = options.unwrap_or_default();
        let open_command_action_descriptor = if options.do_not_register_open_command {
            None
        } else {
            Some(view_container_descriptor.open_command_action_descriptor.clone().unwrap_or_else(|| OpenCommandActionDescriptor::new(&view_container_descriptor.id)))
        };
        let view_container = ViewContainer::new(view_container_descriptor, open_command_action_descriptor);
        {
            let mut view_containers = self.view_containers.borrow_mut();
            match view_containers.iter_mut().find(|(location, _)| *location == view_container_location) {
                Some((_, containers)) => containers.push(view_container.clone()),
                None => view_containers.push((view_container_location, vec![view_container.clone()])),
            }
        }
        if options.is_default {
            self.default_view_containers.borrow_mut().push(view_container.clone());
        }
        self.on_did_register.fire(ViewContainerRegistration { view_container: view_container.clone(), view_container_location });
        view_container
    }

    /// Deregisters the given view container. No op if the view container is not registered.
    pub fn deregister_view_container(&self, view_container: &ViewContainerRc) {
        let removed_from = {
            let mut view_containers = self.view_containers.borrow_mut();
            let found = view_containers.iter().enumerate().find_map(|(position, (location, containers))| {
                containers.iter().position(|candidate| Rc::ptr_eq(candidate, view_container)).map(|index| (position, *location, index))
            });
            found.map(|(position, location, index)| {
                view_containers[position].1.remove(index);
                if view_containers[position].1.is_empty() {
                    view_containers.remove(position);
                }
                location
            })
        };
        if let Some(view_container_location) = removed_from {
            self.on_did_deregister.fire(ViewContainerRegistration { view_container: view_container.clone(), view_container_location });
        }
    }

    /// Returns the view container with given id.
    pub fn get(&self, id: &str) -> Option<ViewContainerRc> {
        self.all().into_iter().find(|view_container| view_container.id == id)
    }

    /// Returns all view containers in the given location.
    pub fn get_view_containers(&self, location: ViewContainerLocation) -> Vec<ViewContainerRc> {
        self.view_containers.borrow().iter().find(|(candidate, _)| *candidate == location).map(|(_, containers)| containers.clone()).unwrap_or_default()
    }

    /// Returns the view container location (`undefined` no original quando o contêiner não está registrado).
    pub fn get_view_container_location(&self, container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        let locations: Vec<ViewContainerLocation> = self.view_containers.borrow().iter().map(|(location, _)| *location).collect();
        locations.into_iter().find(|location| self.get_view_containers(*location).iter().any(|view_container| view_container.id == container.id))
    }

    /// Return the default view containers from the given location.
    pub fn get_default_view_containers(&self, location: ViewContainerLocation) -> Vec<ViewContainerRc> {
        let default_view_containers = self.default_view_containers.borrow().clone();
        default_view_containers.into_iter().filter(|view_container| self.get_view_container_location(view_container) == Some(location)).collect()
    }
}

impl IDisposable for ViewContainersRegistry {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `Registry.as<IViewContainersRegistry>(Extensions.ViewContainersRegistry)`: o registro de contêineres da thread.
pub fn view_containers_registry() -> Rc<ViewContainersRegistry> {
    let registry = registry();
    if !registry.knows(Extensions::VIEW_CONTAINERS_REGISTRY) {
        registry.add(Extensions::VIEW_CONTAINERS_REGISTRY, ViewContainersRegistry::new());
    }

    registry.as_::<ViewContainersRegistry>(Extensions::VIEW_CONTAINERS_REGISTRY).expect("o registro de contêineres de views foi adicionado acima")
}

/// `IView`.
pub trait IView {
    fn id(&self) -> String;

    fn focus(&self);

    fn is_visible(&self) -> bool;

    fn is_body_visible(&self) -> bool;

    fn set_expanded(&self, expanded: bool) -> bool;

    fn get_progress_indicator(&self) -> Option<Rc<dyn IProgressIndicator>>;
}

/// `IViewPaneContainer`.
pub trait IViewPaneContainer {
    fn on_did_add_views(&self) -> Event<Vec<Rc<dyn IView>>>;
    fn on_did_remove_views(&self) -> Event<Vec<Rc<dyn IView>>>;
    fn on_did_change_view_visibility(&self) -> Event<Rc<dyn IView>>;

    fn views(&self) -> Vec<Rc<dyn IView>>;

    fn set_visible(&self, visible: bool);
    fn is_visible(&self) -> bool;
    fn focus(&self);
    fn get_actions_context(&self) -> Dynamic;
    fn get_view(&self, view_id: &str) -> Option<Rc<dyn IView>>;
    fn toggle_view_visibility(&self, view_id: &str);
}

/// `remoteAuthority?: string | string[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteAuthority {
    One(String),
    Many(Vec<String>),
}

/// O `focusCommand?: { id: string; keybindings?: IKeybindings }` de `IViewDescriptor`.
#[derive(Clone, Debug)]
pub struct ViewFocusCommand {
    pub id: String,
    pub keybindings: Option<Keybindings>,
}

/// `IViewDescriptor`.
#[derive(Clone)]
pub struct IViewDescriptor {
    pub type_: Option<String>,
    pub id: String,
    pub name: LocalizedString,
    pub ctor_descriptor: Option<SyncDescriptor<dyn IView>>,
    pub when: Option<ContextKeyExpression>,
    pub order: Option<f64>,
    pub weight: Option<f64>,
    pub collapsed: bool,
    pub can_toggle_visibility: bool,
    pub can_move_view: bool,
    pub container_icon: Option<ViewIcon>,
    pub container_title: Option<String>,
    pub single_view_pane_container_title: Option<String>,
    /// Applies only to newly created views.
    pub hide_by_default: bool,
    pub workspace: bool,
    pub focus_command: Option<ViewFocusCommand>,
    /// For contributed remote explorer views.
    pub group: Option<String>,
    pub remote_authority: Option<RemoteAuthority>,
    pub virtual_workspace: Option<String>,
    pub open_command_action_descriptor: Option<OpenCommandActionDescriptor>,
    pub accessibility_help_content: Option<MarkdownString>,
    /// Specifies in which window this view should be visible. Defaults to the main workbench only.
    pub window_enablement: Option<WindowEnablement>,
    /// `ITreeViewDescriptor.treeView`: a árvore do descritor de uma view de árvore (`None` nas outras views). O
    /// original distingue `ITreeViewDescriptor extends IViewDescriptor` por conversão de tipo; o `Rc` do descritor não
    /// tem conversão, então o campo mora no descritor.
    pub tree_view: Option<Rc<dyn ITreeView>>,
}

impl IViewDescriptor {
    /// O descritor com só o `id` e o `name`, que são os obrigatórios.
    pub fn new(id: &str, name: LocalizedString) -> Self {
        Self {
            type_: None,
            id: id.to_string(),
            name,
            ctor_descriptor: None,
            when: None,
            order: None,
            weight: None,
            collapsed: false,
            can_toggle_visibility: false,
            can_move_view: false,
            container_icon: None,
            container_title: None,
            single_view_pane_container_title: None,
            hide_by_default: false,
            workspace: false,
            focus_command: None,
            group: None,
            remote_authority: None,
            virtual_workspace: None,
            open_command_action_descriptor: None,
            accessibility_help_content: None,
            window_enablement: None,
            tree_view: None,
        }
    }
}

/// O `IViewDescriptor` do original, por identidade.
pub type ViewDescriptorRc = Rc<IViewDescriptor>;

/// `IViewDescriptorRef`.
#[derive(Clone)]
pub struct IViewDescriptorRef {
    pub view_descriptor: ViewDescriptorRc,
    pub index: usize,
}

/// `IAddedViewDescriptorRef` (`extends IViewDescriptorRef`).
#[derive(Clone)]
pub struct IAddedViewDescriptorRef {
    pub view_descriptor: ViewDescriptorRc,
    pub index: usize,
    pub collapsed: bool,
    pub size: Option<f64>,
}

/// `IAddedViewDescriptorState`.
#[derive(Clone)]
pub struct IAddedViewDescriptorState {
    pub view_descriptor: ViewDescriptorRc,
    pub collapsed: Option<bool>,
    pub visible: Option<bool>,
}

/// O `{ added, removed }` de `onDidChangeAllViewDescriptors` e `onDidChangeActiveViewDescriptors`.
#[derive(Clone, Default)]
pub struct ViewDescriptorsChange {
    pub added: Vec<ViewDescriptorRc>,
    pub removed: Vec<ViewDescriptorRc>,
}

/// O `{ title?, icon?, keybindingId?, badgeEnablement? }` de `onDidChangeContainerInfo`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContainerInfoChange {
    pub title: bool,
    pub icon: bool,
    pub keybinding_id: bool,
    pub badge_enablement: bool,
}

/// O `{ from, to }` de `onDidMoveVisibleViewDescriptors`.
#[derive(Clone)]
pub struct ViewDescriptorMove {
    pub from: IViewDescriptorRef,
    pub to: IViewDescriptorRef,
}

/// O `{ id, size }` de `setSizes`.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewSize {
    pub id: String,
    pub size: f64,
}

/// `IViewContainerModel`.
pub trait IViewContainerModel {
    fn view_container(&self) -> ViewContainerRc;

    fn title(&self) -> String;
    fn icon(&self) -> Option<ViewIcon>;
    fn keybinding_id(&self) -> Option<String>;
    fn on_did_change_container_info(&self) -> Event<ContainerInfoChange>;

    fn all_view_descriptors(&self) -> Vec<ViewDescriptorRc>;
    fn on_did_change_all_view_descriptors(&self) -> Event<ViewDescriptorsChange>;

    fn active_view_descriptors(&self) -> Vec<ViewDescriptorRc>;
    fn on_did_change_active_view_descriptors(&self) -> Event<ViewDescriptorsChange>;

    fn visible_view_descriptors(&self) -> Vec<ViewDescriptorRc>;
    fn on_did_add_visible_view_descriptors(&self) -> Event<Vec<IAddedViewDescriptorRef>>;
    fn on_did_remove_visible_view_descriptors(&self) -> Event<Vec<IViewDescriptorRef>>;
    fn on_did_move_visible_view_descriptors(&self) -> Event<ViewDescriptorMove>;

    fn is_visible(&self, id: &str) -> bool;
    fn set_visible(&self, id: &str, visible: bool);

    fn is_collapsed(&self, id: &str) -> bool;
    fn set_collapsed(&self, id: &str, collapsed: bool);

    fn get_size(&self, id: &str) -> Option<f64>;
    fn set_sizes(&self, new_sizes: &[ViewSize]);

    /// `move(from, to)` (`move` é palavra reservada).
    fn move_(&self, from: &str, to: &str);
}

/// `ViewContentGroups`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewContentGroups {
    Open,
    Debug,
    Scm,
    More,
}

impl ViewContentGroups {
    /// O valor de texto do membro.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "2_open",
            Self::Debug => "4_debug",
            Self::Scm => "5_scm",
            Self::More => "9_more",
        }
    }
}

/// `when?: ContextKeyExpression | 'default'` de `IViewContentDescriptor`.
#[derive(Clone)]
pub enum ViewContentWhen {
    Expression(ContextKeyExpression),
    Default,
}

/// `IViewContentDescriptor`.
#[derive(Clone)]
pub struct IViewContentDescriptor {
    pub content: String,
    /// Whether to render all but the first button as secondary if there are buttons in the `content` property.
    pub render_secondary_buttons: Option<bool>,
    pub when: Option<ViewContentWhen>,
    pub group: Option<String>,
    pub order: Option<f64>,
    pub precondition: Option<ContextKeyExpression>,
}

impl IViewContentDescriptor {
    /// O descritor só com o `content`, que é o obrigatório.
    pub fn new(content: &str) -> Self {
        Self { content: content.to_string(), render_secondary_buttons: None, when: None, group: None, order: None, precondition: None }
    }
}

/// `{ views: IViewDescriptor[]; viewContainer: ViewContainer }`: o payload de `onViewsRegistered` e de
/// `onViewsDeregistered`, e o argumento de `registerViews2`.
#[derive(Clone)]
pub struct ViewsRegistration {
    pub views: Vec<ViewDescriptorRc>,
    pub view_container: ViewContainerRc,
}

/// `{ views: IViewDescriptor[]; from: ViewContainer; to: ViewContainer }`.
#[derive(Clone)]
pub struct ViewsContainerChange {
    pub views: Vec<ViewDescriptorRc>,
    pub from: ViewContainerRc,
    pub to: ViewContainerRc,
}

/// `compareViewContentDescriptors`.
fn compare_view_content_descriptors(a: &IViewContentDescriptor, b: &IViewContentDescriptor) -> Ordering {
    let a_group = a.group.as_deref().unwrap_or(ViewContentGroups::More.as_str());
    let b_group = b.group.as_deref().unwrap_or(ViewContentGroups::More.as_str());
    if a_group != b_group {
        return locale_compare(a_group, b_group);
    }
    a.order.unwrap_or(5.0).partial_cmp(&b.order.unwrap_or(5.0)).unwrap_or(Ordering::Equal)
}

/// `IViewsRegistry`.
pub type IViewsRegistry = ViewsRegistry;

/// `ViewsRegistry`.
pub struct ViewsRegistry {
    base: Rc<Disposable>,
    this: Weak<ViewsRegistry>,
    on_views_registered: Rc<Emitter<Vec<ViewsRegistration>>>,
    on_views_deregistered: Rc<Emitter<ViewsRegistration>>,
    on_did_change_container: Rc<Emitter<ViewsContainerChange>>,
    on_did_change_view_welcome_content: Rc<Emitter<String>>,
    view_containers: RefCell<Vec<ViewContainerRc>>,
    /// `Map<ViewContainer, IViewDescriptor[]>`, na ordem de inserção.
    views: RefCell<Vec<(ViewContainerRc, Vec<ViewDescriptorRc>)>>,
    view_welcome_contents: RefCell<SetMap<String, ById<IViewContentDescriptor>>>,
}

impl ViewsRegistry {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_views_registered = Emitter::new();
        base.register(on_views_registered.clone());
        let on_views_deregistered = Emitter::new();
        base.register(on_views_deregistered.clone());
        let on_did_change_container = Emitter::new();
        base.register(on_did_change_container.clone());
        let on_did_change_view_welcome_content = Emitter::new();
        base.register(on_did_change_view_welcome_content.clone());
        Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            on_views_registered,
            on_views_deregistered,
            on_did_change_container,
            on_did_change_view_welcome_content,
            view_containers: RefCell::new(Vec::new()),
            views: RefCell::new(Vec::new()),
            view_welcome_contents: RefCell::new(SetMap::new()),
        })
    }

    pub fn on_views_registered(&self) -> Event<Vec<ViewsRegistration>> {
        self.on_views_registered.event()
    }

    pub fn on_views_deregistered(&self) -> Event<ViewsRegistration> {
        self.on_views_deregistered.event()
    }

    pub fn on_did_change_container(&self) -> Event<ViewsContainerChange> {
        self.on_did_change_container.event()
    }

    pub fn on_did_change_view_welcome_content(&self) -> Event<String> {
        self.on_did_change_view_welcome_content.event()
    }

    pub fn register_views(&self, views: Vec<ViewDescriptorRc>, view_container: &ViewContainerRc) {
        self.register_views2(vec![ViewsRegistration { views, view_container: view_container.clone() }]);
    }

    pub fn register_views2(&self, views: Vec<ViewsRegistration>) {
        for registration in &views {
            self.add_views(&registration.views, &registration.view_container);
        }
        self.on_views_registered.fire(views);
    }

    pub fn deregister_views(&self, view_descriptors: &[ViewDescriptorRc], view_container: &ViewContainerRc) {
        let views = self.remove_views(view_descriptors, view_container);
        if !views.is_empty() {
            self.on_views_deregistered.fire(ViewsRegistration { views, view_container: view_container.clone() });
        }
    }

    pub fn move_views(&self, views_to_move: &[ViewDescriptorRc], view_container: &ViewContainerRc) {
        let containers: Vec<ViewContainerRc> = self.views.borrow().iter().map(|(container, _)| container.clone()).collect();
        for container in containers {
            if !Rc::ptr_eq(&container, view_container) {
                let views = self.remove_views(views_to_move, &container);
                if !views.is_empty() {
                    self.add_views(&views, view_container);
                    self.on_did_change_container.fire(ViewsContainerChange { views, from: container, to: view_container.clone() });
                }
            }
        }
    }

    pub fn get_views(&self, loc: &ViewContainerRc) -> Vec<ViewDescriptorRc> {
        self.views.borrow().iter().find(|(container, _)| Rc::ptr_eq(container, loc)).map(|(_, views)| views.clone()).unwrap_or_default()
    }

    pub fn get_view(&self, id: &str) -> Option<ViewDescriptorRc> {
        let view_containers = self.view_containers.borrow().clone();
        view_containers.iter().find_map(|view_container| self.get_views(view_container).into_iter().find(|view| view.id == id))
    }

    pub fn get_view_container(&self, view_id: &str) -> Option<ViewContainerRc> {
        let view_containers = self.view_containers.borrow().clone();
        view_containers.into_iter().find(|view_container| self.get_views(view_container).iter().any(|view| view.id == view_id))
    }

    pub fn register_view_welcome_content(&self, id: &str, view_content: IViewContentDescriptor) -> DisposableRef {
        let view_content = Rc::new(view_content);
        self.view_welcome_contents.borrow_mut().add(id.to_string(), ById(view_content.clone()));
        self.on_did_change_view_welcome_content.fire(id.to_string());

        self.welcome_content_disposable(id, view_content)
    }

    /// `registerViewWelcomeContent2<TKey>`: o `Map<TKey, IViewContentDescriptor>` é a lista de pares na ordem do mapa.
    pub fn register_view_welcome_content2<TKey: Clone>(&self, id: &str, view_content_map: Vec<(TKey, IViewContentDescriptor)>) -> Vec<(TKey, DisposableRef)> {
        let mut disposables = Vec::new();

        for (key, content) in view_content_map {
            let content = Rc::new(content);
            self.view_welcome_contents.borrow_mut().add(id.to_string(), ById(content.clone()));

            disposables.push((key, self.welcome_content_disposable(id, content)));
        }
        self.on_did_change_view_welcome_content.fire(id.to_string());

        disposables
    }

    pub fn get_view_welcome_content(&self, id: &str) -> Vec<Rc<IViewContentDescriptor>> {
        let mut result: Vec<Rc<IViewContentDescriptor>> = Vec::new();
        self.view_welcome_contents.borrow().for_each(&id.to_string(), |descriptor| result.push(descriptor.0.clone()));
        result.sort_by(|a, b| compare_view_content_descriptors(a, b));
        result
    }

    /// O `toDisposable(() => { delete; fire })` das duas formas de `registerViewWelcomeContent`.
    fn welcome_content_disposable(&self, id: &str, content: Rc<IViewContentDescriptor>) -> DisposableRef {
        let this = self.this.clone();
        let id = id.to_string();
        to_disposable(move || {
            if let Some(this) = this.upgrade() {
                this.view_welcome_contents.borrow_mut().delete(&id, &ById(content));
                this.on_did_change_view_welcome_content.fire(id);
            }
        })
    }

    fn add_views(&self, view_descriptors: &[ViewDescriptorRc], view_container: &ViewContainerRc) {
        {
            let mut views = self.views.borrow_mut();
            if !views.iter().any(|(container, _)| Rc::ptr_eq(container, view_container)) {
                views.push((view_container.clone(), Vec::new()));
                self.view_containers.borrow_mut().push(view_container.clone());
            }
        }
        for view_descriptor in view_descriptors {
            if self.get_view(&view_descriptor.id).is_some() {
                errors::throw(Error::new(localize("duplicateId", "A view with id '{0}' is already registered", &[view_descriptor.id.as_str()])));
            }
            let mut views = self.views.borrow_mut();
            if let Some((_, container_views)) = views.iter_mut().find(|(container, _)| Rc::ptr_eq(container, view_container)) {
                container_views.push(view_descriptor.clone());
            }
        }
    }

    fn remove_views(&self, view_descriptors: &[ViewDescriptorRc], view_container: &ViewContainerRc) -> Vec<ViewDescriptorRc> {
        let mut views = self.views.borrow_mut();
        let Some(position) = views.iter().position(|(container, _)| Rc::ptr_eq(container, view_container)) else {
            return Vec::new();
        };
        let mut views_to_deregister = Vec::new();
        let mut remaining_views = Vec::new();
        for view in &views[position].1 {
            if !view_descriptors.iter().any(|candidate| Rc::ptr_eq(candidate, view)) {
                remaining_views.push(view.clone());
            } else {
                views_to_deregister.push(view.clone());
            }
        }
        if !views_to_deregister.is_empty() {
            if !remaining_views.is_empty() {
                views[position].1 = remaining_views;
            } else {
                views.remove(position);
                let mut view_containers = self.view_containers.borrow_mut();
                if let Some(index) = view_containers.iter().position(|container| Rc::ptr_eq(container, view_container)) {
                    view_containers.remove(index);
                }
            }
        }
        views_to_deregister
    }
}

impl IDisposable for ViewsRegistry {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `Registry.as<IViewsRegistry>(Extensions.ViewsRegistry)`: o registro de views da thread.
pub fn views_registry() -> Rc<ViewsRegistry> {
    let registry = registry();
    if !registry.knows(Extensions::VIEWS_REGISTRY) {
        registry.add(Extensions::VIEWS_REGISTRY, ViewsRegistry::new());
    }

    registry.as_::<ViewsRegistry>(Extensions::VIEWS_REGISTRY).expect("o registro de views foi adicionado acima")
}

/// `ViewVisibilityState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewVisibilityState {
    Default = 0,
    Expand = 1,
}

/// `{ container: ViewContainer; location: ViewContainerLocation }`.
#[derive(Clone)]
pub struct ViewContainerAndLocation {
    pub container: ViewContainerRc,
    pub location: ViewContainerLocation,
}

/// O `{ added, removed }` de `onDidChangeViewContainers`.
#[derive(Clone, Default)]
pub struct ViewContainersChange {
    pub added: Vec<ViewContainerAndLocation>,
    pub removed: Vec<ViewContainerAndLocation>,
}

/// O `{ viewContainer, from, to }` de `onDidChangeContainerLocation`.
#[derive(Clone)]
pub struct ViewContainerLocationChange {
    pub view_container: ViewContainerRc,
    pub from: ViewContainerLocation,
    pub to: ViewContainerLocation,
}

/// O `{ views, from, to }` de `onDidChangeLocation`.
#[derive(Clone)]
pub struct ViewsLocationChange {
    pub views: Vec<ViewDescriptorRc>,
    pub from: ViewContainerLocation,
    pub to: ViewContainerLocation,
}

/// `IViewDescriptorService`.
pub trait IViewDescriptorService {
    // ViewContainers
    fn view_containers(&self) -> Vec<ViewContainerRc>;
    fn on_did_change_view_containers(&self) -> Event<ViewContainersChange>;

    fn get_default_view_container(&self, location: ViewContainerLocation) -> Option<ViewContainerRc>;
    fn get_view_container_by_id(&self, id: &str) -> Option<ViewContainerRc>;
    fn is_view_container_removed_permanently(&self, id: &str) -> bool;
    fn get_default_view_container_location(&self, view_container: &ViewContainerRc) -> Option<ViewContainerLocation>;
    fn get_view_container_location(&self, view_container: &ViewContainerRc) -> Option<ViewContainerLocation>;
    fn get_view_containers_by_location(&self, location: ViewContainerLocation) -> Vec<ViewContainerRc>;
    fn get_view_container_model(&self, view_container: &ViewContainerRc) -> Rc<dyn IViewContainerModel>;

    fn on_did_change_container_location(&self) -> Event<ViewContainerLocationChange>;
    fn move_view_container_to_location(&self, view_container: &ViewContainerRc, location: ViewContainerLocation, requested_index: Option<usize>, reason: Option<&str>);

    fn get_view_container_badge_enablement_state(&self, id: &str) -> bool;
    fn set_view_container_badge_enablement_state(&self, id: &str, badges_enabled: bool);

    // Views
    fn get_view_descriptor_by_id(&self, id: &str) -> Option<ViewDescriptorRc>;
    fn get_view_container_by_view_id(&self, id: &str) -> Option<ViewContainerRc>;
    fn get_default_container_by_id(&self, id: &str) -> Option<ViewContainerRc>;
    fn get_view_location_by_id(&self, id: &str) -> Option<ViewContainerLocation>;

    fn can_move_views(&self) -> bool;

    fn on_did_change_container(&self) -> Event<ViewsContainerChange>;
    fn move_views_to_container(&self, views: &[ViewDescriptorRc], view_container: &ViewContainerRc, visibility_state: Option<ViewVisibilityState>, reason: Option<&str>);

    fn on_did_change_location(&self) -> Event<ViewsLocationChange>;
    fn move_view_to_location(&self, view: &ViewDescriptorRc, location: ViewContainerLocation, reason: Option<&str>);

    fn reset(&self);
}

delayed_proxy!(pub ViewDescriptorServiceProxy for dyn IViewDescriptorService {
    methods {
        fn view_containers(&self) -> Vec<ViewContainerRc>;
        fn get_default_view_container(&self, location: ViewContainerLocation) -> Option<ViewContainerRc>;
        fn get_view_container_by_id(&self, id: &str) -> Option<ViewContainerRc>;
        fn is_view_container_removed_permanently(&self, id: &str) -> bool;
        fn get_default_view_container_location(&self, view_container: &ViewContainerRc) -> Option<ViewContainerLocation>;
        fn get_view_container_location(&self, view_container: &ViewContainerRc) -> Option<ViewContainerLocation>;
        fn get_view_containers_by_location(&self, location: ViewContainerLocation) -> Vec<ViewContainerRc>;
        fn get_view_container_model(&self, view_container: &ViewContainerRc) -> Rc<dyn IViewContainerModel>;
        fn move_view_container_to_location(&self, view_container: &ViewContainerRc, location: ViewContainerLocation, requested_index: Option<usize>, reason: Option<&str>);
        fn get_view_container_badge_enablement_state(&self, id: &str) -> bool;
        fn set_view_container_badge_enablement_state(&self, id: &str, badges_enabled: bool);
        fn get_view_descriptor_by_id(&self, id: &str) -> Option<ViewDescriptorRc>;
        fn get_view_container_by_view_id(&self, id: &str) -> Option<ViewContainerRc>;
        fn get_default_container_by_id(&self, id: &str) -> Option<ViewContainerRc>;
        fn get_view_location_by_id(&self, id: &str) -> Option<ViewContainerLocation>;
        fn can_move_views(&self) -> bool;
        fn move_views_to_container(&self, views: &[ViewDescriptorRc], view_container: &ViewContainerRc, visibility_state: Option<ViewVisibilityState>, reason: Option<&str>);
        fn move_view_to_location(&self, view: &ViewDescriptorRc, location: ViewContainerLocation, reason: Option<&str>);
        fn reset(&self);
    }
    events {
        fn on_did_change_view_containers(&self) -> ViewContainersChange;
        fn on_did_change_container_location(&self) -> ViewContainerLocationChange;
        fn on_did_change_container(&self) -> ViewsContainerChange;
        fn on_did_change_location(&self) -> ViewsLocationChange;
    }
});

service_id!(pub I_VIEW_DESCRIPTOR_SERVICE: dyn IViewDescriptorService = "viewDescriptorService", proxy ViewDescriptorServiceProxy);
