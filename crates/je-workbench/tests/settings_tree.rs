//! Porte de `src/vs/workbench/contrib/preferences/test/browser/settingsTree.test.ts`.
//!
//! Suíte `SettingsTree renderer`, caso a caso e na mesma ordem. Os oito `test(...)` do original são oito `#[test]`; os `for` de fora de um `test(...)` (os valores padrão do
//! array, os alvos, as chaves do sandbox e os domínios permitidos) são laços dentro do teste de Rust, porque o original registra
//! um teste por combinação e um `#[test]` conta o literal `test(...)`.
//!
//! Divergências:
//! - Os dublês de serviços (`TestSettingsConfiguration`, `StubManagedSettingsService` sobre o `ManagedStore`, o `ILanguageService`
//!   parcial, o `ConfigurationRegistration` e os construtores de configurações) vivem em `support/preferences_support.rs`, e os de
//!   teclado, comando, abertura de recurso, área de transferência e menu de contexto em `support/notification_doubles.rs`. O
//!   `workbenchInstantiationService` e o `instantiationService.stub(...)` são o [`SettingsTreeServices`] que `create_services`
//!   monta e o [`SettingTreeRenderersServices`] que [`renderer_services`] monta. O `TestContextMenuService` é o
//!   [`RecordingContextMenu`], que guarda o delegado de `showContextMenu`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada teste; `suiteSetup`/`suiteTeardown` do registro da
//!   configuração `sandboxRendererPresentationTest` são a [`ConfigurationRegistration`] criada no começo de cada teste.
//! - `document.createElement('div')` é um nó da árvore do `je-ui`. `container.querySelector*` e `textContent` são as buscas na
//!   subárvore e a concatenação dos textos dos nós; `aria-label` é o rótulo do nó; `element.click()` e o `dblclick` são o evento
//!   `Click` e `DoubleClick` despachados no nó.
//! - `querySelector('[role="checkbox"]').getAttribute('aria-checked')` e `aria-disabled` são os atributos do nó do `Checkbox`
//!   (`Tree::attribute`). O `<select>` do enum é o `SelectBox` customizado (ver `select_box.rs`), que grava o atributo
//!   `disabled` no nó (`querySelector('select').disabled` é `Tree::attribute(..., "disabled").is_some()`), e
//!   `selectedOptions[0].text` é `selected_option_text`.
//! - `mock<ITreeNode>() { element }` é o `TreeNode` com o elemento e os demais campos zerados.
//! - A propriedade `AgentSandboxRetryWithAllowNetworkRequests` não está entre as que `terminal_chat_agent_tools_*` devolve (só as
//!   de apresentação gerenciada), então a mensagem de depreciação é o texto do original (`markdownDeprecationMessage`).
//! - `TestSettingRenderer extends AbstractSettingRenderer` troca o `renderSettingToolbar` por uma barra falsa e o
//!   `IInstantiationService` por um que cria indicadores vazios; aqui o [`AbstractSettingRenderer`] é o real, com o `ToolBar`
//!   real (o descarte dele remove o nó `monaco-toolbar` do template) e os indicadores reais. `element.inspectSelf = () => {}`
//!   não existe: o `inspect_self` roda sobre o `TestConfigurationService` vazio.
//! - `await scoped.setUserConfiguration(...)` e `await action.run(element)` são a chamada direta (o efeito é imediato).
//! - A subclasse de `TestConfigurationService` que sobrescreve `inspect` é a [`TestSettingsConfiguration`] com `stub_inspect_with`
//!   (o `super.inspect()` é o `inspect` de um segundo serviço sobre o mesmo objeto, [`TestSettingsConfiguration::base`]).

mod support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::actions::ActionRef;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::dynamic::json_stringify;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable, none, to_disposable};
use je_base::network::{FileAccess, FileAccessContext, RemoteAuthorities};
use je_base::path::ProcessEnvironment;
use je_base::path::Platform;
use je_base::policy::PolicyValue;
use je_base::uri::URI;
use je_platform::accessibility::accessibility_service::{IAccessibilityHost, IMediaQueryList};
use je_platform::configuration::registry::ConfigurationScope;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextview::context_view::{HideData, IContextViewDelegate as IPlatformContextViewDelegate, IContextViewService};
use je_platform::contextview::view_element::ViewElement;
use je_platform::network_filter::settings::AgentNetworkDomainSettingId;
use je_platform::policy::copilot_managed_settings::{
    COPILOT_SANDBOX_ALLOWED_HOSTS_KEY, COPILOT_SANDBOX_ALLOW_BYPASS_KEY, COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY, COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY,
    COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY, COPILOT_SANDBOX_ENABLED_KEY, COPILOT_SANDBOX_LSP_SERVERS_KEY, COPILOT_SANDBOX_MCP_SERVERS_KEY,
};
use je_platform::resources::ext_uri;
use je_platform::sandbox::settings::AgentSandboxSettingId;
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_platform::user_data_sync::user_data_sync::{IUserDataSyncEnablementService, SyncResource};
use je_theme::{ColorTheme, ThemeType};
use je_ui::{Event as UiEvent, EventKind, NodeId, Role, Ui};
use je_widgets::context_view_api::{IContextMenuDelegate, IContextMenuProvider, IContextViewDelegate, IContextViewProvider};
use je_widgets::dom::create_el;
use je_widgets::hover_api::{
    HoverTarget, IDelayedHoverOptions, IHoverDelegate, IHoverDelegate2, IHoverLifecycleOptions, IHoverOptions, IHoverWidget, IManagedHover, IManagedHoverOptions,
    ManagedHoverContentOrFactory,
};
use je_widgets::tree::index_tree_model::TreeNode;
use je_widgets::tree::tree_types::{ITreeRenderer, TreeVisibility};
use je_workbench::contrib::preferences::settings_tree_models::SettingsTreeElement;
use je_workbench::contrib::preferences::preferences_widgets::SettingsTarget;
use je_workbench::contrib::preferences::settings_markdown_view::OwnedMarkdownEnvironment;
use je_workbench::contrib::preferences::settings_tree_models::{
    SettingsTreeGroupChild, SettingsTreeGroupElement, SettingsTreeModel, SettingsTreeServices, SettingsTreeSettingElement,
};
use je_workbench::contrib::preferences::settings_tree_renderers::{
    AbstractSettingRenderer, ISettingChangeEvent, ISettingEnumItemTemplate, ISettingBoolItemTemplate, ISettingItemTemplate, SettingRendererServices, SettingRendererStyles,
    SettingTreeRenderers, SettingTreeRenderersServices, SettingsObjectTreeModel, SettingsTreeNode,
};
use je_workbench::contrib::preferences::settings_widgets::{SettingsWidgetServices, SettingsWidgetStyles};
use je_workbench::services::configuration::common::configuration::APPLY_ALL_PROFILES_SETTING;
use je_workbench::services::preferences::preferences::{ISetting, SettingSchemaType};

use crate::notification_doubles::{NoClipboard, NoCommandService, NoKeybindingService, RecordingOpener};
use crate::support::preferences_support::common::{LeakGuard, assert_deep_strict_equal, dyn_strs, ensure_no_disposables_are_leaked_in_test_suite, obj, s};
use crate::support::preferences_support::{
    ConfigurationRegistration, ManagedStore, StubManagedSettingsService, TestSettingsConfiguration, copy_of, create_services, first_defined, first_element,
    new_test_service, null_managed_settings_service, register_sandbox_presentation_configuration, toc_entry, view_state,
};

// ---------------------------------------------------------------------------------------------------------------------
// Dublês

/// `undefined!` no lugar do `IContextViewService` de `SettingTreeRenderers`: só o `cancelSuggesters` o usaria.
struct NoContextViewService;

impl IContextViewService for NoContextViewService {
    fn show_context_view(&self, _delegate: Rc<dyn IPlatformContextViewDelegate>, _container: Option<Rc<ViewElement>>, _shadow_root: bool) {
        panic!("showContextView is not a function")
    }

    fn hide_context_view(&self, _data: Option<HideData>) {}

    fn layout(&self) {}

    fn get_context_view_element(&self) -> Rc<ViewElement> {
        panic!("getContextViewElement is not a function")
    }
}

/// O provedor de vista de contexto dos campos: nenhum caso abre uma sugestão.
struct NoContextViewProvider;

impl IContextViewProvider for NoContextViewProvider {
    fn show_context_view(&self, _delegate: Rc<dyn IContextViewDelegate>, _container: Option<NodeId>) {
        panic!("showContextView is not a function")
    }

    fn hide_context_view(&self) {}

    fn layout(&self) {}
}

struct NullManagedHover;

impl IDisposable for NullManagedHover {
    fn dispose(&self) {}
}

impl IManagedHover for NullManagedHover {
    fn show(&self, _focus: bool) {}
    fn hide(&self) {}
    fn update(&self, _tooltip: ManagedHoverContentOrFactory, _options: Option<IManagedHoverOptions>) {}
}

/// `{ setupDelayedHover: () => Disposable.None }`: nenhum hover aparece.
struct NoHover;

impl IHoverDelegate2 for NoHover {
    fn show_delayed_hover(&self, _: IDelayedHoverOptions, _: HoverTarget, _: IHoverLifecycleOptions) -> Option<Rc<dyn IHoverWidget>> {
        None
    }
    fn setup_delayed_hover(&self, _: NodeId, _: Rc<dyn Fn() -> IDelayedHoverOptions>, _: Option<IHoverLifecycleOptions>) -> DisposableRef {
        none()
    }
    fn setup_delayed_hover_at_mouse(&self, _: NodeId, _: Rc<dyn Fn() -> IDelayedHoverOptions>, _: Option<IHoverLifecycleOptions>) -> DisposableRef {
        none()
    }
    fn show_instant_hover(&self, _: IHoverOptions, _: bool) -> Option<Rc<dyn IHoverWidget>> {
        None
    }
    fn hide_hover(&self, _: bool) {}
    fn show_and_focus_last_hover(&self) {}
    fn setup_managed_hover(&self, _: Rc<dyn IHoverDelegate>, _: NodeId, _: ManagedHoverContentOrFactory, _: Option<IManagedHoverOptions>) -> Rc<dyn IManagedHover> {
        Rc::new(NullManagedHover)
    }
    fn show_managed_hover(&self, _: NodeId) {}
}

/// `aria.alert` e `aria.status` sem leitor de tela.
struct SilentAccessibilityHost;

impl IAccessibilityHost for SilentAccessibilityHost {
    fn match_media(&self, _query: &str) -> Rc<dyn IMediaQueryList> {
        panic!("matchMedia is not a function")
    }
    fn alert(&self, _message: &str) {}
    fn status(&self, _message: &str) {}
}

/// `instantiationService.stub(IUserDataSyncEnablementService, { isEnabled: () => false })`.
struct DisabledUserDataSync;

impl IUserDataSyncEnablementService for DisabledUserDataSync {
    fn on_did_change_enablement(&self) -> Event<bool> {
        Event::none()
    }
    fn is_enabled(&self) -> bool {
        false
    }
    fn can_toggle_enablement(&self) -> bool {
        false
    }
    fn set_enablement(&self, _enabled: bool) {}
    fn on_did_change_resource_enablement(&self) -> Event<(SyncResource, bool)> {
        Event::none()
    }
    fn is_resource_enabled(&self, _resource: SyncResource, _default_value: Option<bool>) -> bool {
        false
    }
    fn set_resource_enablement(&self, _resource: SyncResource, _enabled: bool) {}
    fn get_resource_sync_state_version(&self, _resource: SyncResource) -> Option<String> {
        None
    }
    fn is_resource_enablement_configured(&self, _resource: SyncResource) -> bool {
        false
    }
}

/// `new class extends TestContextMenuService { delegate; override showContextMenu(delegate) { this.delegate = delegate; } }`.
#[derive(Default)]
struct RecordingContextMenu {
    delegate: RefCell<Option<Rc<dyn IContextMenuDelegate>>>,
}

impl IContextMenuProvider for RecordingContextMenu {
    fn show_context_menu(&self, delegate: Rc<dyn IContextMenuDelegate>) {
        *self.delegate.borrow_mut() = Some(delegate);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Montagem

fn theme() -> ColorTheme {
    ColorTheme::unloaded("", ThemeType::Dark, HashMap::new())
}

/// Os serviços do `SettingTreeRenderers` (o que `createInstance(SettingTreeRenderers)` injeta).
fn renderer_services(ui: &Ui, tree_services: &SettingsTreeServices, context_menu: Rc<dyn IContextMenuProvider>) -> SettingTreeRenderersServices {
    let theme = theme();
    let widgets = SettingsWidgetServices {
        ui: ui.clone(),
        context_view_provider: Rc::new(NoContextViewProvider),
        hover_service: Rc::new(NoHover),
        styles: SettingsWidgetStyles::from_theme(&theme),
    };
    let markdown_environment = Rc::new(OwnedMarkdownEnvironment {
        ext_uri: Rc::new(ext_uri()),
        env: Rc::new(ProcessEnvironment),
        file_access: Rc::new(FileAccess::new(Rc::new(ProcessEnvironment), FileAccessContext::default(), RemoteAuthorities::new(false))),
    });
    SettingTreeRenderersServices {
        renderer: SettingRendererServices {
            widgets,
            styles: SettingRendererStyles::from_theme(&theme),
            opener_service: Rc::new(RecordingOpener::default()),
            command_service: Rc::new(NoCommandService),
            context_menu_provider: context_menu,
            keybinding_service: Rc::new(NoKeybindingService),
            configuration_service: tree_services.configuration_service.clone(),
            language_service: tree_services.language_service.clone(),
            user_data_sync_enablement_service: Rc::new(DisabledUserDataSync),
            markdown_environment,
            accessibility_host: Rc::new(SilentAccessibilityHost),
            telemetry_service: null_telemetry_service(),
        },
        context_view_service: Rc::new(NoContextViewService),
        clipboard_service: Rc::new(NoClipboard),
        product_service: tree_services.product_service.clone(),
    }
}

/// `registry.registerConfiguration(configurationNode)` do `suiteSetup`.
fn suite_setup() -> ConfigurationRegistration {
    register_sandbox_presentation_configuration("sandboxRendererPresentationTest")
}

/// `container.querySelectorAll(...)` por classe, na ordem do documento.
fn with_class(ui: &Ui, root: NodeId, class: &str) -> Vec<NodeId> {
    let tree = ui.tree();
    tree.subtree(root).into_iter().filter(|node| tree.has_class(*node, class)).collect()
}

/// `container.querySelectorAll('[aria-label="..."]')`.
fn with_label(ui: &Ui, root: NodeId, label: &str) -> Vec<NodeId> {
    let tree = ui.tree();
    tree.subtree(root).into_iter().filter(|node| tree.label(*node) == Some(label)).collect()
}

/// `element.textContent`: o texto concatenado da subárvore. O Markdown de um bloco é um parágrafo só (um nó), então o
/// espaço entre as palavras já está no texto, como no nó de texto do DOM.
fn text_content(ui: &Ui, root: NodeId) -> String {
    ui.tree().text_content(root)
}

fn click(ui: &Ui, node: NodeId) {
    ui.dispatch(&mut UiEvent::new(EventKind::Click, node));
}

fn double_click(ui: &Ui, node: NodeId) {
    ui.dispatch(&mut UiEvent::new(EventKind::DoubleClick, node));
}

fn boolean(value: bool) -> Dynamic {
    Dynamic::Bool(value)
}

fn number(value: usize) -> Dynamic {
    Dynamic::Number(value as f64)
}

/// `mock<ITreeNode<SettingsTreeElement, never>>() { element }`.
fn tree_node(element: &Rc<SettingsTreeSettingElement>) -> Rc<SettingsTreeNode> {
    Rc::new(TreeNode {
        parent: RefCell::new(Weak::new()),
        element: Some(SettingsTreeGroupChild::Setting(element.clone())),
        children: RefCell::new(Vec::new()),
        depth: 1,
        visible_children_count: Cell::new(0),
        visible_child_index: Cell::new(0),
        collapsible: Cell::new(false),
        collapsed: Cell::new(false),
        render_node_count: Cell::new(1),
        visibility: Cell::new(TreeVisibility::Visible),
        visible: Cell::new(true),
        filter_data: RefCell::new(None),
        last_diff_ids: RefCell::new(None),
    })
}

/// O que `renderer.renderTemplate(container)` e `renderer.renderElement(node, 0, template)` montam para um elemento.
struct RenderedRow {
    ui: Ui,
    container: NodeId,
    renderer: Rc<dyn ITreeRenderer<SettingsObjectTreeModel>>,
    template: Rc<dyn std::any::Any>,
    node: Rc<SettingsTreeNode>,
}

impl RenderedRow {
    /// `renderers.allRenderers.find(renderer => renderer.templateId === templateId)`, o `renderTemplate` com o `disposeTemplate`
    /// registrado no `store`.
    fn new(store: &LeakGuard, ui: &Ui, renderers: &SettingTreeRenderers, template_id: &str, element: &Rc<SettingsTreeSettingElement>) -> Self {
        let renderer = renderers.all_renderers.iter().find(|renderer| renderer.template_id() == template_id).expect("o renderizador do template existe").clone();
        let root = ui.root();
        let container = create_el(&mut ui.tree_mut(), root, Role::Generic, "");
        let template = renderer.render_template(container);
        {
            let (renderer, template) = (renderer.clone(), template.clone());
            store.add(to_disposable(move || renderer.dispose_template(&template)));
        }
        Self { ui: ui.clone(), container, renderer, template, node: tree_node(element) }
    }

    /// `renderer.renderElement(node, 0, template)`.
    fn render(&self) {
        self.renderer.render_element(&self.node, 0, &self.template, None);
    }
}

/// `{ value, manualReset }` de cada mudança, como o `deepStrictEqual` do original as compara.
fn changes_of(changes: &Rc<RefCell<Vec<(Dynamic, bool)>>>) -> Dynamic {
    Dynamic::array(changes.borrow().iter().map(|(value, manual_reset)| obj(vec![("value", value.clone()), ("manualReset", boolean(*manual_reset))])))
}

/// `store.add(renderers.onDidChangeSetting(({ value, manualReset }) => changes.push({ value, manualReset })))`.
fn record_changes(store: &LeakGuard, renderers: &SettingTreeRenderers) -> Rc<RefCell<Vec<(Dynamic, bool)>>> {
    let changes: Rc<RefCell<Vec<(Dynamic, bool)>>> = Rc::default();
    let recorded = changes.clone();
    store.add(renderers.on_did_change_setting.listen(move |event: &ISettingChangeEvent| recorded.borrow_mut().push((event.value.clone(), event.manual_reset))));
    changes
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('array overrides')

/// O `renderArray(settingsTarget, defaultValue, inherited)` da suíte.
struct ArrayRender {
    row: RenderedRow,
    renderers: Rc<SettingTreeRenderers>,
    context_menu: Rc<RecordingContextMenu>,
    changes: Rc<RefCell<Vec<(Dynamic, bool)>>>,
    scoped: Rc<TestConfigurationService>,
    key: &'static str,
    element: Rc<SettingsTreeSettingElement>,
    user_value: Vec<&'static str>,
}

impl ArrayRender {
    fn new(store: &LeakGuard, settings_target: SettingsTarget, default_value: &[&'static str], inherited: bool) -> Self {
        let key = "terminal.integrated.commandsToSkipShell";
        let mut user_value: Vec<&'static str> = default_value.to_vec();
        user_value.push("-workbench.action.quickOpen");

        let defaults = new_test_service(store, Some(obj(vec![(key, dyn_strs(default_value))])));
        let scoped = new_test_service(store, if inherited { Some(obj(Vec::<(&str, Dynamic)>::new())) } else { Some(obj(vec![(key, dyn_strs(&user_value))])) });
        let configuration = TestSettingsConfiguration::new(store, Some(obj(vec![(key, dyn_strs(&user_value)), (APPLY_ALL_PROFILES_SETTING, Dynamic::array([]))])));
        let base = configuration.base(store);
        {
            let (defaults, scoped, settings_target) = (defaults.clone(), scoped.clone(), settings_target.clone());
            configuration.service.stub_inspect_with(move |setting, overrides| {
                let inspected = base.inspect(setting, overrides);
                let default_value = defaults.get_value(Some(setting), None);
                let scoped_value = scoped.get_value(Some(setting), None);
                let mut result = copy_of(&*inspected);
                result.workspace_value = if settings_target == SettingsTarget::Workspace { scoped_value.clone() } else { Dynamic::Undefined };
                result.workspace_folder_value = if matches!(settings_target, SettingsTarget::Resource(_)) { scoped_value.clone() } else { Dynamic::Undefined };
                result.value = first_defined(&[&scoped_value, &inspected.user_value(), &default_value]);
                result.default_value = default_value;
                Rc::new(result)
            });
        }
        let (services, _) = create_services(store, &configuration, true, null_managed_settings_service());
        let model = store.add_rc(SettingsTreeModel::new(view_state(settings_target), true, services.clone()));
        let setting = ISetting {
            key: key.to_owned(),
            r#type: Some(SettingSchemaType::Single("array".to_owned())),
            array_item_type: Some("string".to_owned()),
            description: Vec::new(),
            scope: Some(ConfigurationScope::Resource),
            ..ISetting::default()
        };
        model.update(&toc_entry("test", "Test", vec![Rc::new(setting)]));
        let element = first_element(&model, key);

        let ui = Ui::new();
        let context_menu = Rc::new(RecordingContextMenu::default());
        let renderers = store.add_rc(SettingTreeRenderers::new(renderer_services(&ui, &services, context_menu.clone())));
        let row = RenderedRow::new(store, &ui, &renderers, "settings.array.template", &element);
        row.render();
        let changes = record_changes(store, &renderers);
        Self { row, renderers, context_menu, changes, scoped, key, element, user_value }
    }

    fn remove_last_item(&self) {
        let remove = with_label(&self.row.ui, self.row.container, "Remove Item").last().copied();
        let remove = remove.expect("The inherited array item must be removable");
        click(&self.row.ui, remove);
    }

    fn reset(&self) {
        self.renderers.show_context_menu(&self.element, self.row.container);
        let delegate = self.context_menu.delegate.borrow().clone();
        let actions = delegate.expect("o menu de contexto abriu").get_actions();
        let action: ActionRef = actions.into_iter().find(|action| action.id() == "settings.resetSetting").expect("a ação de reiniciar existe");
        let _ = action.run(&[Dynamic::from(ObjectRef::with_host(self.element.clone()))]);
    }
}

#[test]
fn preserves_the_explicit_default_array_when_clearing_an_inherited_agents_value() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = suite_setup();

    for default_value in [Vec::<&'static str>::new(), vec!["default-command"]] {
        let rendered = ArrayRender::new(&store, SettingsTarget::Workspace, &default_value, true);
        rendered.remove_last_item();
        let first_change = rendered.changes.borrow()[0].0.clone();
        let _ = rendered.scoped.set_user_configuration(rendered.key, first_change, None);
        rendered.row.render();

        assert_deep_strict_equal(
            &obj(vec![("changes", changes_of(&rendered.changes)), ("displayed", rendered.element.value()), ("configured", boolean(rendered.element.is_configured()))]),
            &obj(vec![
                ("changes", Dynamic::array([obj(vec![("value", dyn_strs(&default_value)), ("manualReset", boolean(false))])])),
                ("displayed", dyn_strs(&default_value)),
                ("configured", boolean(true)),
            ]),
        );
    }
}

#[test]
fn clearing_a_configured_array_respects_the_target() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = suite_setup();

    for (name, target, expected) in [
        ("Workspace", SettingsTarget::Workspace, Dynamic::array([])),
        ("Folder", SettingsTarget::Resource(URI::file("/workspace", Platform::Posix)), Dynamic::array([])),
        ("User", SettingsTarget::UserLocal, Dynamic::Undefined),
    ] {
        let rendered = ArrayRender::new(&store, target, &[], false);
        rendered.remove_last_item();
        assert_deep_strict_equal(
            &changes_of(&rendered.changes),
            &Dynamic::array([obj(vec![("value", expected), ("manualReset", boolean(false))])]),
        );
        let _ = name;
    }
}

#[test]
fn reset_setting_still_removes_the_agents_array_override_and_restores_inheritance() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = suite_setup();

    let rendered = ArrayRender::new(&store, SettingsTarget::Workspace, &[], false);
    rendered.reset();
    let first_change = rendered.changes.borrow()[0].0.clone();
    let _ = rendered.scoped.set_user_configuration(rendered.key, first_change, None);
    rendered.row.render();

    assert_deep_strict_equal(
        &obj(vec![("changes", changes_of(&rendered.changes)), ("displayed", rendered.element.value()), ("configured", boolean(rendered.element.is_configured()))]),
        &obj(vec![
            ("changes", Dynamic::array([obj(vec![("value", Dynamic::Undefined), ("manualReset", boolean(true))])])),
            ("displayed", dyn_strs(&rendered.user_value)),
            ("configured", boolean(false)),
        ]),
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// renders managed state and unlocks <key> after policy removal

#[test]
fn renders_managed_state_and_unlocks_each_sandbox_setting_after_policy_removal() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = suite_setup();

    for key in [
        AgentSandboxSettingId::AgentSandboxEnabled,
        AgentSandboxSettingId::AgentSandboxAllowUnsandboxedCommands,
        AgentSandboxSettingId::AgentSandboxAllowNetwork,
        AgentSandboxSettingId::AgentSandboxAllowLocalNetwork,
        AgentSandboxSettingId::AgentSandboxAllowDevToolAccess,
        AgentSandboxSettingId::AgentSandboxMcpServers,
        AgentSandboxSettingId::AgentSandboxLspServers,
    ] {
        let is_enabled = matches!(key, AgentSandboxSettingId::AgentSandboxEnabled);
        let is_server_sandbox = matches!(key, AgentSandboxSettingId::AgentSandboxMcpServers | AgentSandboxSettingId::AgentSandboxLspServers);
        let key = key.as_str();

        let initial = if is_enabled { s("off") } else { boolean(!is_server_sandbox) };
        let configuration = TestSettingsConfiguration::new(&store, Some(obj(vec![(key, initial), (APPLY_ALL_PROFILES_SETTING, Dynamic::array([]))])));

        // `required`, `allowBypass`, `allowOutbound` e `sandboxServers` do original, gravados no `ManagedStore` a cada leitura.
        let managed = Rc::new(ManagedStore::default());
        let apply = |required: bool, allow_bypass: Option<bool>, allow_outbound: Option<bool>, sandbox_servers: Option<bool>| {
            let set = |managed_key: &str, value: Option<bool>| match value {
                Some(value) => managed.set(managed_key, Some(PolicyValue::Boolean(value))),
                None => managed.delete(managed_key),
            };
            set(COPILOT_SANDBOX_MCP_SERVERS_KEY, sandbox_servers);
            set(COPILOT_SANDBOX_LSP_SERVERS_KEY, sandbox_servers);
            set(COPILOT_SANDBOX_ALLOW_OUTBOUND_KEY, allow_outbound);
            set(COPILOT_SANDBOX_ALLOW_LOCAL_NETWORK_KEY, allow_outbound);
            set(COPILOT_SANDBOX_ALLOW_DEV_TOOL_ACCESS_KEY, allow_outbound);
            set(COPILOT_SANDBOX_ALLOW_BYPASS_KEY, allow_bypass);
            set(COPILOT_SANDBOX_ENABLED_KEY, required.then_some(true));
        };
        apply(true, Some(false), Some(false), Some(true));

        let (services, _) = create_services(&store, &configuration, false, Rc::new(StubManagedSettingsService::new(managed.clone())));
        let model = store.add_rc(SettingsTreeModel::new(view_state(SettingsTarget::UserLocal), true, services.clone()));
        let setting = ISetting {
            key: key.to_owned(),
            r#type: Some(SettingSchemaType::Single(if is_enabled { "string" } else { "boolean" }.to_owned())),
            r#enum: is_enabled.then(|| vec!["off".to_owned(), "on".to_owned()]),
            description: vec!["Sandbox setting".to_owned()],
            scope: Some(ConfigurationScope::Resource),
            ..ISetting::default()
        };
        model.update(&toc_entry("test", "Test", vec![Rc::new(setting)]));
        let element = first_element(&model, key);

        let ui = Ui::new();
        let renderers = store.add_rc(SettingTreeRenderers::new(renderer_services(&ui, &services, Rc::new(RecordingContextMenu::default()))));
        let row = RenderedRow::new(&store, &ui, &renderers, if is_enabled { "settings.enum.template" } else { "settings.bool.template" }, &element);

        let render = || {
            row.render();
            let (disabled, value) = if is_enabled {
                let template = row.template.downcast_ref::<ISettingEnumItemTemplate>().expect("o template do enum");
                (ui.tree().attribute(template.select_element, "disabled").is_some(),s(&template.select_box.selected_option_text().expect("uma opção selecionada")))
            } else {
                let template = row.template.downcast_ref::<ISettingBoolItemTemplate>().expect("o template do bool");
                let node = template.checkbox.dom_node();
                let tree = ui.tree();
                (tree.attribute(node, "aria-disabled") == Some("true"), boolean(tree.attribute(node, "aria-checked") == Some("true")))
            };
            obj(vec![("disabled", boolean(disabled)), ("value", value), ("indicator", boolean(text_content(&ui, row.container).contains("Managed by organization")))])
        };
        let expected = |disabled: bool, value: Dynamic, indicator: bool| obj(vec![("disabled", boolean(disabled)), ("value", value), ("indicator", boolean(indicator))]);

        let managed_state = render();
        apply(true, Some(true), Some(true), Some(false));
        let bypass_allowed = render();
        apply(false, None, None, None);
        let removed = render();
        assert_deep_strict_equal(
            &obj(vec![("managed", managed_state), ("bypassAllowed", bypass_allowed), ("removed", removed)]),
            &obj(vec![
                ("managed", expected(true, if is_enabled { s("on") } else { boolean(is_server_sandbox) }, true)),
                ("bypassAllowed", expected(is_enabled, if is_enabled { s("on") } else { boolean(!is_server_sandbox) }, is_enabled)),
                ("removed", expected(false, if is_enabled { s("off") } else { boolean(!is_server_sandbox) }, false)),
            ]),
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// makes managed allowed domains <hosts> read-only and restores editing after removal

#[test]
fn makes_managed_allowed_domains_read_only_and_restores_editing_after_removal() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = suite_setup();

    for allowed_hosts in [Vec::<&'static str>::new(), vec!["managed.example", "*.managed.example"]] {
        let key = AgentNetworkDomainSettingId::AllowedNetworkDomains.as_str();
        let local_hosts = ["local.example"];
        let configuration = TestSettingsConfiguration::new(&store, Some(obj(vec![(key, dyn_strs(&local_hosts)), (APPLY_ALL_PROFILES_SETTING, Dynamic::array([]))])));
        let managed = Rc::new(ManagedStore::default());
        let (services, _) = create_services(&store, &configuration, false, Rc::new(StubManagedSettingsService::new(managed.clone())));
        let model = store.add_rc(SettingsTreeModel::new(view_state(SettingsTarget::UserLocal), true, services.clone()));
        let setting = ISetting {
            key: key.to_owned(),
            r#type: Some(SettingSchemaType::Single("array".to_owned())),
            array_item_type: Some("string".to_owned()),
            description: Vec::new(),
            scope: Some(ConfigurationScope::Application),
            ..ISetting::default()
        };
        model.update(&toc_entry("test", "Test", vec![Rc::new(setting)]));
        let element = first_element(&model, key);

        let ui = Ui::new();
        let renderers = store.add_rc(SettingTreeRenderers::new(renderer_services(&ui, &services, Rc::new(RecordingContextMenu::default()))));
        let changes = record_changes(&store, &renderers);
        let row = RenderedRow::new(&store, &ui, &renderers, "settings.array.template", &element);

        let render = || {
            row.render();
            let rows = with_class(&ui, row.container, "setting-list-row");
            let tree_attribute = |node: NodeId| ui.tree().attribute(node, "draggable") == Some("true");
            obj(vec![
                ("values", Dynamic::array(with_class(&ui, row.container, "setting-list-value").into_iter().map(|value| s(&text_content(&ui, value))))),
                ("addHidden", boolean(!with_class(&ui, row.container, "setting-list-hide-add-button").is_empty())),
                ("actions", number(with_label(&ui, row.container, "Edit Item").len() + with_label(&ui, row.container, "Remove Item").len())),
                ("draggable", Dynamic::array(rows.into_iter().map(|row| boolean(tree_attribute(row))))),
                ("indicator", boolean(text_content(&ui, row.container).contains("Managed by organization"))),
            ])
        };
        let has_input = || {
            let tree = ui.tree();
            tree.subtree(row.container).into_iter().any(|node| tree.role(node) == Role::TextInput)
        };

        let initial = render();
        let first_row = with_class(&ui, row.container, "setting-list-row").first().copied().expect("a linha local existe");
        double_click(&ui, first_row);
        assert!(has_input(), "The local list should be editable before policy arrives");
        managed.set(COPILOT_SANDBOX_ALLOWED_HOSTS_KEY, Some(PolicyValue::String(json_stringify(&dyn_strs(&allowed_hosts)).expect("JSON.stringify"))));
        let locked = render();
        if let Some(locked_row) = with_class(&ui, row.container, "setting-list-row").first().copied() {
            double_click(&ui, locked_row);
        }
        let editing = has_input();
        managed.delete(COPILOT_SANDBOX_ALLOWED_HOSTS_KEY);
        let removed = render();

        assert_deep_strict_equal(
            &obj(vec![
                ("initial", initial.clone()),
                ("locked", locked),
                ("editing", boolean(editing)),
                ("removed", removed),
                ("changes", changes_of(&changes)),
                ("stored", configuration.get_value(key)),
            ]),
            &obj(vec![
                (
                    "initial",
                    obj(vec![
                        ("values", dyn_strs(&local_hosts)),
                        ("addHidden", boolean(false)),
                        ("actions", number(2)),
                        ("draggable", Dynamic::array([boolean(true)])),
                        ("indicator", boolean(false)),
                    ]),
                ),
                (
                    "locked",
                    obj(vec![
                        ("values", dyn_strs(&allowed_hosts)),
                        ("addHidden", boolean(true)),
                        ("actions", number(0)),
                        ("draggable", Dynamic::array(allowed_hosts.iter().map(|_| boolean(false)))),
                        ("indicator", boolean(true)),
                    ]),
                ),
                ("editing", boolean(false)),
                ("removed", initial),
                ("changes", Dynamic::array([])),
                ("stored", dyn_strs(&local_hosts)),
            ]),
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// o AbstractSettingRenderer sozinho (TestSettingRenderer)

/// O `AbstractSettingRenderer` e o template comum do `TestSettingRenderer` (`renderTemplate` é `renderCommonTemplate`).
struct CommonRenderer {
    ui: Ui,
    renderer: Rc<AbstractSettingRenderer>,
    container: NodeId,
    template: ISettingItemTemplate,
    tree_services: SettingsTreeServices,
}

impl CommonRenderer {
    fn new(store: &LeakGuard) -> Self {
        let configuration = TestSettingsConfiguration::new(store, None);
        let (tree_services, _) = create_services(store, &configuration, false, null_managed_settings_service());
        let ui = Ui::new();
        let services = renderer_services(&ui, &tree_services, Rc::new(RecordingContextMenu::default()));
        let renderer = AbstractSettingRenderer::new(services.renderer, Vec::new(), Rc::new(|_: &ISetting, _: &SettingsTarget| Vec::new()));
        let root = ui.root();
        let container = create_el(&mut ui.tree_mut(), root, Role::Generic, "");
        let template = renderer.render_common_template(container, "test");
        Self { ui, renderer, container, template, tree_services }
    }

    /// `createSettingElement(severity)`.
    fn create_setting_element(&self, deprecation_message: &str, severity: &str) -> Rc<SettingsTreeSettingElement> {
        let group = SettingsTreeGroupElement::new("test", None, "Test", 0, false);
        let setting = ISetting {
            key: "test.setting".to_owned(),
            r#type: Some(SettingSchemaType::Single("string".to_owned())),
            description: Vec::new(),
            deprecation_message: Some(deprecation_message.to_owned()),
            deprecation_message_severity: Some(severity.to_owned()),
            deprecation_message_show_in_settings: Some(true),
            ..ISetting::default()
        };
        SettingsTreeSettingElement::new(Rc::new(setting), &group, SettingsTarget::UserLocal, true, None, self.tree_services.clone(), false)
    }

    /// `renderer.renderElement({ element }, 0, template)`: o `renderValue` do `TestSettingRenderer` não faz nada.
    fn render_element(&self, element: &Rc<SettingsTreeSettingElement>) {
        self.renderer.render_setting_element(element, &self.template, &|_on_change| {});
    }

    fn has_toolbar(&self) -> bool {
        !with_class(&self.ui, self.container, "monaco-toolbar").is_empty()
    }

    /// O primeiro filho do elemento de depreciação (`firstElementChild`): o ícone.
    fn deprecation_icon(&self) -> Option<NodeId> {
        self.ui.tree().children(self.template.deprecation_warning_element).first().copied()
    }
}

#[test]
fn disposes_the_setting_toolbar_with_its_template() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = suite_setup();
    let rendered = CommonRenderer::new(&store);

    assert!(rendered.has_toolbar());
    rendered.renderer.dispose_template(&rendered.template);
    assert!(!rendered.has_toolbar());

    rendered.renderer.dispose();
}

#[test]
fn renders_a_deprecated_sandbox_setting_warning_separately_from_the_description() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = suite_setup();
    let rendered = CommonRenderer::new(&store);
    store.add_rc(rendered.renderer.clone());
    let element = rendered.create_setting_element("This setting will be deprecated soon. It does not apply to the Copilot Agent Host sandbox.", "warning");
    store.add_rc(element.parent().expect("o elemento tem grupo"));
    store.add_rc(element.clone());

    rendered.render_element(&element);
    let icon = rendered.deprecation_icon();
    let (icon_classes, icon_role, icon_aria_label) = {
        let tree = rendered.ui.tree();
        (
            icon.map(|icon| tree.classes(icon).join(" ")),
            icon.map(|icon| if tree.role(icon) == Role::Image { "img" } else { "other" }),
            icon.and_then(|icon| tree.label(icon).map(str::to_owned)),
        )
    };
    let is_info = rendered.ui.tree().has_class(rendered.template.container_element, "is-deprecated-info");
    let text = text_content(&rendered.ui, rendered.template.deprecation_warning_element);
    rendered.renderer.dispose_template(&rendered.template);

    assert_deep_strict_equal(
        &obj(vec![
            ("text", s(&text)),
            ("iconClasses", icon_classes.map_or(Dynamic::Undefined, |classes| s(&classes))),
            ("iconRole", icon_role.map_or(Dynamic::Undefined, s)),
            ("iconAriaLabel", icon_aria_label.map_or(Dynamic::Undefined, |label| s(&label))),
            ("isInfo", boolean(is_info)),
        ]),
        &obj(vec![
            ("text", s("This setting will be deprecated soon. It does not apply to the Copilot Agent Host sandbox.")),
            ("iconClasses", s("codicon codicon-error")),
            ("iconRole", s("img")),
            ("iconAriaLabel", s("Warning")),
            ("isInfo", boolean(false)),
        ]),
    );
}

#[test]
fn renders_informational_deprecation_severity() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let _registration = suite_setup();
    let rendered = CommonRenderer::new(&store);
    store.add_rc(rendered.renderer.clone());
    let element = rendered.create_setting_element("Deprecated setting", "info");
    store.add_rc(element.parent().expect("o elemento tem grupo"));
    store.add_rc(element.clone());

    rendered.render_element(&element);
    let icon = rendered.deprecation_icon();
    let (is_deprecated, is_deprecated_info, icon_classes, icon_role, icon_aria_label) = {
        let tree = rendered.ui.tree();
        (
            tree.has_class(rendered.template.container_element, "is-deprecated"),
            tree.has_class(rendered.template.container_element, "is-deprecated-info"),
            icon.map(|icon| tree.classes(icon).join(" ")),
            icon.map(|icon| if tree.role(icon) == Role::Image { "img" } else { "other" }),
            icon.and_then(|icon| tree.label(icon).map(str::to_owned)),
        )
    };

    assert_deep_strict_equal(
        &obj(vec![
            ("isDeprecated", boolean(is_deprecated)),
            ("isDeprecatedInfo", boolean(is_deprecated_info)),
            ("iconClasses", icon_classes.map_or(Dynamic::Undefined, |classes| s(&classes))),
            ("iconRole", icon_role.map_or(Dynamic::Undefined, s)),
            ("iconAriaLabel", icon_aria_label.map_or(Dynamic::Undefined, |label| s(&label))),
        ]),
        &obj(vec![
            ("isDeprecated", boolean(true)),
            ("isDeprecatedInfo", boolean(true)),
            ("iconClasses", s("codicon codicon-info")),
            ("iconRole", s("img")),
            ("iconAriaLabel", s("Info")),
        ]),
    );

    rendered.renderer.dispose_template(&rendered.template);
}
