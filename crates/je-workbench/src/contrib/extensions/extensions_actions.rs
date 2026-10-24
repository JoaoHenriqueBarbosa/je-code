//! Porte de `src/vs/workbench/contrib/extensions/browser/extensionsActions.ts`: a lógica das ações de extensões
//! (`enabled`, `label`, `class`, `tooltip` e `run`), sem a renderização (a UI é nativa, não há DOM).
//!
//! Notas de tradução:
//! - `ExtensionAction extends Action` é o [`ExtensionAction`], que guarda o estado de uma [`Action`](je_base::actions::Action)
//!   (com o `onDidChange` próprio, que também emite `hidden` e `menuActions`) e a extensão corrente. Cada ação concreta
//!   guarda um `ExtensionAction` em `base` e o expõe por `Deref`; `IAction`, `IExtensionContainer` e
//!   [`IExtensionAction`] saem das macros `impl_extension_action!` e `impl_extension_container!`.
//! - Os serviços que o construtor do original recebe por decorador são o [`ExtensionActionServices`].
//! - O `update()` das ações que usam `Throttler` roda a primeira chamada de forma síncrona e as seguintes depois da
//!   anterior terminar, como o original; o corpo `async` roda já até o primeiro `await`.
//!
//! Recortes (cada um entra inteiro quando o serviço que ele usa for portado):
//! - **`run()` do [`InstallAction`] e do [`UpdateAction`]** (e a `MigrateDeprecatedExtensionAction`): precisam de
//!   `IExtensionsWorkbenchService.open` (o editor de extensão), do `PromptExtensionInstallFailureAction`, do
//!   `IWorkbenchThemeService` e do `IPreferencesService`, que não existem no porte. Instalar sem os diálogos de
//!   assinatura e de descontinuação seria outra ação, então `run` fica sem efeito e só `update` está portado.
//! - **`run()` do [`ManageExtensionAction`]** (`getActionGroups`/`showMenu`) depende do menu de contexto
//!   (`getContextMenuActionsGroups`, `toActions`, `IMenuService`) e do `DropDownExtensionActionViewItem`.
//! - **`alert(...)`** (anúncio de acessibilidade em região viva do DOM) não tem equivalente nativo e fica fora de
//!   [`UninstallAction`] e do `run` do [`InstallInOtherServerAction`]; o `open` do editor de extensão idem, no
//!   `run` do [`InstallInOtherServerAction`].
//! - **As ações de tema** (`SetColorThemeAction`...), as de recomendação, as de `ExtensionStatusAction` e
//!   `ExtensionStatusLabelAction`, as de instalação em lote e as de auto-atualização: nenhum caso do teste portado as
//!   exercita e dependem de serviços de tema, de recomendação e de quickinput que não existem no porte.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IAction, Separator};
use je_base::async_::{Throttler, async_fn};
use je_base::dynamic::Dynamic;
use je_base::error_message::to_error_message_of;
use je_base::errors::is_cancellation_error;
use je_base::event::{Emitter, Event, any};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_extensions::allowed::{AllowedCandidate, AllowedExtensionsService};
use je_extensions::management::InstallOptions;
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionManifest, ExtensionType, is_language_pack_extension};
use je_extensions::util::are_same_extensions;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationTarget};
use je_platform::dialogs::dialogs::IDialogService;
use je_platform::label::label::ILabelService;
use je_platform::product::IProductService;
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::update::update::UpdateService;
use je_platform::user_data_profile::user_data_profile::IUserDataProfilesService;
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState};

use super::extensions::{ExtensionRuntimeActionType, ExtensionState, IExtension, IExtensionContainer};
use super::extensions_service::ExtensionsWorkbenchService;
use super::extensions_workbench_service::Extension;
use crate::services::extension_management::extension_enablement_service::CHAT_AI_DISABLED_SETTING_ID;
use crate::services::extension_management::extension_install_service::{IExtensionInstallService, IWorkbenchExtensionInstallService};
use crate::services::extension_management::extension_management::{
    EnablementState, IExtensionManagementServer, IExtensionManagementServerService, IWorkbenchExtensionEnablementService,
};
use crate::services::extensions::extension_manifest_properties_service::IExtensionManifestPropertiesService;
use crate::services::extensions::extensions::IExtensionService;
use crate::services::host::host::IHostService;

// ---------------------------------------------------------------------------------------------------------------------
// Serviços e classes de estilo.
// ---------------------------------------------------------------------------------------------------------------------

/// Os serviços que as ações recebem por decorador no original.
#[derive(Clone)]
pub struct ExtensionActionServices {
    pub extensions_workbench_service: Rc<ExtensionsWorkbenchService>,
    pub extension_management_service: Rc<dyn IWorkbenchExtensionInstallService>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub extension_enablement_service: Rc<dyn IWorkbenchExtensionEnablementService>,
    pub extension_management_server_service: Rc<dyn IExtensionManagementServerService>,
    pub extension_manifest_properties_service: Rc<dyn IExtensionManifestPropertiesService>,
    pub product_service: Rc<dyn IProductService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub workspace_context_service: Rc<dyn IWorkspaceContextService>,
    pub host_service: Rc<dyn IHostService>,
    pub update_service: Rc<dyn UpdateService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub allowed_extensions_service: Rc<AllowedExtensionsService>,
    pub label_service: Rc<dyn ILabelService>,
    pub dialog_service: Rc<dyn IDialogService>,
    pub user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
}

/// `ExtensionAction.EXTENSION_ACTION_CLASS`.
pub const EXTENSION_ACTION_CLASS: &str = "extension-action";
/// `ExtensionAction.TEXT_ACTION_CLASS`.
pub const TEXT_ACTION_CLASS: &str = "extension-action text";
/// `ExtensionAction.LABEL_ACTION_CLASS`.
pub const LABEL_ACTION_CLASS: &str = "extension-action label";
/// `ExtensionAction.ICON_ACTION_CLASS`.
pub const ICON_ACTION_CLASS: &str = "extension-action icon";

fn undefined() -> Promise<Dynamic> {
    Promise::resolved(Dynamic::Undefined)
}

/// `ExtensionIdentifier.equals(extension.identifier.id, productService.defaultChatAgent?.chatExtensionId)`.
fn is_chat_extension(product_service: &dyn IProductService, extension_id: &str) -> bool {
    product_service.default_chat_agent().is_some_and(|agent| agent.chat_extension_id.to_lowercase() == extension_id.to_lowercase())
}

/// `{ id: e.identifier.value, uuid: e.uuid }` de uma extensão em execução.
fn running_identifier(running: &je_extensions::scanner::ExtensionDescription) -> ExtensionIdentifierInfo {
    ExtensionIdentifierInfo { id: running.identifier.value.clone(), uuid: running.uuid.clone() }
}

fn same_server(a: &Option<Rc<IExtensionManagementServer>>, b: &Option<Rc<IExtensionManagementServer>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// ExtensionAction.
// ---------------------------------------------------------------------------------------------------------------------

/// `IExtensionActionChangeEvent`: o que mudou (os campos mudados valem `Some`).
#[derive(Clone, Default)]
pub struct IExtensionActionChangeEvent {
    pub label: Option<String>,
    pub tooltip: Option<String>,
    pub class: Option<String>,
    pub enabled: Option<bool>,
    pub checked: Option<bool>,
    pub hidden: Option<bool>,
    pub menu_actions: Option<Vec<ActionRef>>,
}

/// `ExtensionAction`: o estado de uma ação de extensão.
pub struct ExtensionAction {
    disposable: Rc<Disposable>,
    on_did_change: Rc<Emitter<IExtensionActionChangeEvent>>,
    id: String,
    label: RefCell<String>,
    tooltip: RefCell<String>,
    class: RefCell<String>,
    enabled: Cell<bool>,
    checked: Cell<Option<bool>>,
    extension: RefCell<Option<Rc<Extension>>>,
    hidden: Cell<bool>,
    hide_on_disabled: Cell<bool>,
    update_when_counter_extension_changes: Cell<bool>,
}

impl ExtensionAction {
    /// `super(id, label, cssClass, enabled)` de `Action`.
    pub fn new(id: &str, label: &str, class: &str, enabled: bool) -> Self {
        let disposable = Disposable::new();
        let on_did_change = Emitter::new();
        disposable.register(on_did_change.clone());
        Self {
            disposable,
            on_did_change,
            id: id.to_owned(),
            label: RefCell::new(label.to_owned()),
            tooltip: RefCell::new(String::new()),
            class: RefCell::new(class.to_owned()),
            enabled: Cell::new(enabled),
            checked: Cell::new(None),
            extension: RefCell::new(None),
            hidden: Cell::new(false),
            hide_on_disabled: Cell::new(true),
            update_when_counter_extension_changes: Cell::new(false),
        }
    }

    /// `_register(o)`.
    pub fn register(&self, disposable: DisposableRef) -> DisposableRef {
        self.disposable.register(disposable)
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<IExtensionActionChangeEvent> {
        self.on_did_change.event()
    }

    /// `get extension()`.
    pub fn extension(&self) -> Option<Rc<Extension>> {
        self.extension.borrow().clone()
    }

    /// O que o `set extension` guarda; quem o chama faz o `this.update()` em seguida.
    pub fn store_extension(&self, extension: Option<Rc<Extension>>) {
        *self.extension.borrow_mut() = extension;
    }

    /// `get hidden()`.
    pub fn hidden(&self) -> bool {
        self.hidden.get()
    }

    /// `set hidden(hidden)`.
    pub fn set_hidden(&self, hidden: bool) {
        if self.hidden.get() != hidden {
            self.hidden.set(hidden);
            self.fire(IExtensionActionChangeEvent { hidden: Some(hidden), ..Default::default() });
        }
    }

    /// `hideOnDisabled = value`.
    pub fn set_hide_on_disabled(&self, hide_on_disabled: bool) {
        self.hide_on_disabled.set(hide_on_disabled);
    }

    /// `updateWhenCounterExtensionChanges`.
    pub fn update_when_counter_extension_changes(&self) -> bool {
        self.update_when_counter_extension_changes.get()
    }

    pub fn set_update_when_counter_extension_changes(&self, value: bool) {
        self.update_when_counter_extension_changes.set(value);
    }

    /// `this.class = value`.
    pub fn set_class_name(&self, class: &str) {
        IAction::set_class(self, Some(class.to_owned()));
    }

    fn fire(&self, event: IExtensionActionChangeEvent) {
        self.on_did_change.fire(event);
    }
}

impl IAction for ExtensionAction {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn label(&self) -> String {
        self.label.borrow().clone()
    }

    fn set_label(&self, value: String) {
        if *self.label.borrow() != value {
            *self.label.borrow_mut() = value.clone();
            self.fire(IExtensionActionChangeEvent { label: Some(value), ..Default::default() });
        }
    }

    fn tooltip(&self) -> String {
        self.tooltip.borrow().clone()
    }

    fn set_tooltip(&self, value: String) {
        if *self.tooltip.borrow() != value {
            *self.tooltip.borrow_mut() = value.clone();
            self.fire(IExtensionActionChangeEvent { tooltip: Some(value), ..Default::default() });
        }
    }

    fn class(&self) -> Option<String> {
        Some(self.class.borrow().clone())
    }

    fn set_class(&self, value: Option<String>) {
        let value = value.unwrap_or_default();
        if *self.class.borrow() != value {
            *self.class.borrow_mut() = value.clone();
            self.fire(IExtensionActionChangeEvent { class: Some(value), ..Default::default() });
        }
    }

    fn enabled(&self) -> bool {
        self.enabled.get()
    }

    /// `_setEnabled(value)`: depois de guardar, a ação some quando `hideOnDisabled`.
    fn set_enabled(&self, value: bool) {
        if self.enabled.get() != value {
            self.enabled.set(value);
            self.fire(IExtensionActionChangeEvent { enabled: Some(value), ..Default::default() });
        }
        if self.hide_on_disabled.get() {
            self.set_hidden(!value);
        }
    }

    fn checked(&self) -> Option<bool> {
        self.checked.get()
    }

    fn set_checked(&self, value: Option<bool>) {
        if self.checked.get() != value {
            self.checked.set(value);
            self.fire(IExtensionActionChangeEvent { checked: value, ..Default::default() });
        }
    }

    fn run(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        undefined()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl IDisposable for ExtensionAction {
    fn dispose(&self) {
        self.disposable.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ExtensionAction"
    }
}

/// Uma [`IAction`] que também é contêiner de extensão, com o `hidden` e o `onDidChange` de [`ExtensionAction`]: o que
/// [`ButtonWithDropDownExtensionAction`] precisa das ações que agrupa (`ExtensionAction[][]`).
pub trait IExtensionAction: IAction + IExtensionContainer {
    fn hidden(&self) -> bool;
    fn on_did_change_extension_action(&self) -> Event<IExtensionActionChangeEvent>;
}

/// Implementa `IAction`, `IDisposable`, `Deref<Target = ExtensionAction>` e [`IExtensionAction`] de uma ação concreta que
/// guarda o `ExtensionAction` em `base`. O `run` é o método `$run` (`fn(&self, &[Dynamic]) -> Promise<Dynamic>`) ou, sem
/// ele, o do `ExtensionAction` (sem efeito).
macro_rules! impl_extension_action {
    (@common $ty:ty, $run:expr) => {
        impl IAction for $ty {
            fn id(&self) -> String {
                IAction::id(&self.base)
            }
            fn label(&self) -> String {
                IAction::label(&self.base)
            }
            fn set_label(&self, value: String) {
                IAction::set_label(&self.base, value)
            }
            fn tooltip(&self) -> String {
                IAction::tooltip(&self.base)
            }
            fn set_tooltip(&self, value: String) {
                IAction::set_tooltip(&self.base, value)
            }
            fn class(&self) -> Option<String> {
                IAction::class(&self.base)
            }
            fn set_class(&self, value: Option<String>) {
                IAction::set_class(&self.base, value)
            }
            fn enabled(&self) -> bool {
                IAction::enabled(&self.base)
            }
            fn set_enabled(&self, value: bool) {
                IAction::set_enabled(&self.base, value)
            }
            fn checked(&self) -> Option<bool> {
                IAction::checked(&self.base)
            }
            fn set_checked(&self, value: Option<bool>) {
                IAction::set_checked(&self.base, value)
            }
            fn run(&self, args: &[Dynamic]) -> Promise<Dynamic> {
                ($run)(self, args)
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        impl IDisposable for $ty {
            fn dispose(&self) {
                IDisposable::dispose(&self.base);
            }
            fn constructor_name(&self) -> &'static str {
                stringify!($ty)
            }
        }

        impl Deref for $ty {
            type Target = ExtensionAction;
            fn deref(&self) -> &ExtensionAction {
                &self.base
            }
        }

        impl IExtensionAction for $ty {
            fn hidden(&self) -> bool {
                self.base.hidden()
            }
            fn on_did_change_extension_action(&self) -> Event<IExtensionActionChangeEvent> {
                self.base.on_did_change()
            }
        }
    };
    ($ty:ty) => {
        impl_extension_action!(@common $ty, |this: &$ty, args: &[Dynamic]| IAction::run(&this.base, args));
    };
    ($ty:ty, run = $run:ident) => {
        impl_extension_action!(@common $ty, |this: &$ty, args: &[Dynamic]| this.$run(args));
    };
}

/// Implementa [`IExtensionContainer`] de uma ação concreta: `set extension` guarda e chama `$update`.
macro_rules! impl_extension_container {
    ($ty:ty, $update:ident) => {
        impl IExtensionContainer for $ty {
            fn extension(&self) -> Option<Rc<Extension>> {
                self.base.extension()
            }
            fn set_extension(&self, extension: Option<Rc<Extension>>) {
                self.base.store_extension(extension);
                self.$update();
            }
            fn update_when_counter_extension_changes(&self) -> bool {
                self.base.update_when_counter_extension_changes()
            }
            fn update(&self) {
                self.$update();
            }
        }
    };
}

/// Registra no `base` o ouvinte de um evento que chama `$update` (`this._register(event(() => this.update()))`).
macro_rules! register_update_listener {
    ($this:expr, $event:expr, $update:ident) => {{
        let weak = Rc::downgrade(&$this);
        $this.base.register($event.listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.$update();
            }
        }));
    }};
}

// ---------------------------------------------------------------------------------------------------------------------
// ButtonWithDropDownExtensionAction.
// ---------------------------------------------------------------------------------------------------------------------

/// `ButtonWithDropDownExtensionAction`: o botão cuja ação primária é a primeira visível de um grupo de ações.
pub struct ButtonWithDropDownExtensionAction {
    base: ExtensionAction,
    actions_groups: Vec<Vec<Rc<dyn IExtensionAction>>>,
    extension_actions: Vec<Rc<dyn IExtensionAction>>,
    primary_action: RefCell<Option<ActionRef>>,
    menu_actions: RefCell<Vec<ActionRef>>,
    /// `getLabel(action)`: o texto do botão a partir da ação primária (padrão: o `label` dela).
    label_of: Box<dyn Fn(&ActionRef) -> String>,
    pub menu_action_class_names: Vec<String>,
}

impl ButtonWithDropDownExtensionAction {
    /// `constructor(id, clazz, actionsGroups)`.
    pub fn new(id: &str, class: &str, actions_groups: Vec<Vec<Rc<dyn IExtensionAction>>>) -> Rc<Self> {
        Self::with_label(id, class, actions_groups, Box::new(|action| action.label()))
    }

    fn with_label(id: &str, class: &str, actions_groups: Vec<Vec<Rc<dyn IExtensionAction>>>, label_of: Box<dyn Fn(&ActionRef) -> String>) -> Rc<Self> {
        let class = format!("{class} action-dropdown");
        let extension_actions: Vec<Rc<dyn IExtensionAction>> = actions_groups.iter().flatten().cloned().collect();
        let this = Rc::new(Self {
            base: ExtensionAction::new(id, "", &class, true),
            actions_groups,
            extension_actions,
            primary_action: RefCell::new(None),
            menu_actions: RefCell::new(Vec::new()),
            label_of,
            menu_action_class_names: class.split(' ').map(str::to_owned).collect(),
        });
        this.base.set_hide_on_disabled(false);
        this.update_with(false);
        let weak = Rc::downgrade(&this);
        let changes = any(this.extension_actions.iter().map(|action| action.on_did_change_extension_action()).collect());
        this.base.register(changes.listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_with(true);
            }
        }));
        for action in &this.extension_actions {
            let disposable: DisposableRef = action.clone();
            this.base.register(disposable);
        }
        this
    }

    /// `get menuActions()`.
    pub fn menu_actions(&self) -> Vec<ActionRef> {
        self.menu_actions.borrow().clone()
    }

    /// `update(donotUpdateActions)`.
    pub fn update_with(&self, donot_update_actions: bool) {
        if !donot_update_actions {
            for action in &self.extension_actions {
                action.update();
            }
        }

        let mut actions: Vec<ActionRef> = Vec::new();
        for group in &self.actions_groups {
            let visible: Vec<&Rc<dyn IExtensionAction>> = group.iter().filter(|action| !action.hidden()).collect();
            if !visible.is_empty() {
                for action in visible {
                    let action: ActionRef = action.clone();
                    actions.push(action);
                }
                let separator: ActionRef = Separator::new();
                actions.push(separator);
            }
        }
        if !actions.is_empty() {
            actions.pop();
        }

        let primary_action = actions.first().cloned();
        *self.primary_action.borrow_mut() = primary_action.clone();
        let menu_actions = if actions.len() > 1 { actions } else { Vec::new() };
        *self.menu_actions.borrow_mut() = menu_actions.clone();
        self.base.fire(IExtensionActionChangeEvent { menu_actions: Some(menu_actions), ..Default::default() });

        match primary_action {
            Some(primary_action) => {
                self.base.set_hidden(false);
                self.base.set_enabled(primary_action.enabled());
                self.base.set_label((self.label_of)(&primary_action));
                self.base.set_tooltip(primary_action.tooltip());
            }
            None => {
                self.base.set_hidden(true);
                self.base.set_enabled(false);
            }
        }
    }

    fn run_primary(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        let primary = self.primary_action.borrow().clone();
        if self.base.enabled()
            && let Some(primary) = primary
        {
            return primary.run(&[]);
        }
        undefined()
    }
}

/// `this.extensionActions.forEach(a => this._register(a))` descarta a ação filha junto com o botão (o `IExtensionContainer`
/// é `IDisposable`, então a ação filha vira `DisposableRef`).
impl_extension_action!(ButtonWithDropDownExtensionAction, run = run_primary);

impl IExtensionContainer for ButtonWithDropDownExtensionAction {
    fn extension(&self) -> Option<Rc<Extension>> {
        self.base.extension()
    }

    /// `set extension`: as ações filhas primeiro, depois a própria.
    fn set_extension(&self, extension: Option<Rc<Extension>>) {
        for action in &self.extension_actions {
            action.set_extension(extension.clone());
        }
        self.base.store_extension(extension);
        self.update_with(false);
    }

    fn update_when_counter_extension_changes(&self) -> bool {
        self.base.update_when_counter_extension_changes()
    }

    fn update(&self) {
        self.update_with(false);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// InstallAction.
// ---------------------------------------------------------------------------------------------------------------------

/// `getLocale(gallery)` e o `isWeb` de `canSetLanguage` (`extensionsWorkbenchService.ts`).
fn can_set_language(extension: &Rc<Extension>) -> bool {
    if !Platform::detect().is_web {
        return false;
    }
    let Some(gallery) = extension.gallery() else { return false };
    gallery.tags.iter().find(|tag| tag.starts_with("lp-")).and_then(|tag| tag.split("lp-").nth(1)).is_some_and(|locale| !locale.is_empty())
}

/// `InstallAction`.
pub struct InstallAction {
    this: Weak<InstallAction>,
    base: ExtensionAction,
    services: ExtensionActionServices,
    manifest: RefCell<Option<ExtensionManifest>>,
    update_throttler: Rc<Throttler<()>>,
    pub options: InstallOptions,
}

impl InstallAction {
    pub const CLASS: &'static str = "extension-action label prominent install";
    const HIDE: &'static str = "extension-action label prominent install hide";

    /// `constructor(options, ...services)`.
    pub fn new(options: InstallOptions, services: &ExtensionActionServices) -> Rc<Self> {
        let mut options = options;
        if options.is_machine_scoped.is_none() {
            options.is_machine_scoped = Some(false);
        }
        let this = Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            base: ExtensionAction::new("extensions.install", &localize("install", "Install", &[]), Self::CLASS, false),
            services: services.clone(),
            manifest: RefCell::new(None),
            update_throttler: Rc::new(Throttler::new()),
            options,
        });
        this.base.register(this.update_throttler.clone());
        this.base.set_hide_on_disabled(false);
        this.update_state();
        register_update_listener!(this, services.allowed_extensions_service.on_did_change_allowed_extensions_config_value(), update_state);
        register_update_listener!(this, services.label_service.on_did_change_formatters(), update_label);
        this
    }

    /// `set manifest(manifest)`.
    pub fn set_manifest(&self, manifest: Option<ExtensionManifest>) {
        *self.manifest.borrow_mut() = manifest;
        self.update_label();
    }

    fn update_state(&self) {
        let Some(this) = self.this.upgrade() else { return };
        self.update_throttler.queue(move |_| this.compute_and_update_enablement());
    }

    /// `computeAndUpdateEnablement()`.
    fn compute_and_update_enablement(self: Rc<Self>) -> Promise<()> {
        async_fn(async move {
            self.base.set_enabled(false);
            self.base.set_class_name(Self::HIDE);
            self.base.set_hidden(true);
            let Some(extension) = self.base.extension() else { return Ok(()) };
            if extension.is_builtin() {
                return Ok(());
            }
            if can_set_language(&extension) {
                return Ok(());
            }
            if extension.state() != ExtensionState::Uninstalled {
                return Ok(());
            }
            if self.options.install_pre_release_version {
                let publisher_display_name = extension.publisher_display_name();
                let id = extension.identifier().id;
                let allowed = self.services.allowed_extensions_service.is_allowed(&AllowedCandidate::Fields {
                    id: &id,
                    publisher_display_name: Some(publisher_display_name.as_str()),
                    version: None,
                    prerelease: Some(true),
                    target_platform: None,
                });
                if !extension.has_pre_release_version() || allowed.is_err() {
                    return Ok(());
                }
            }
            if !self.options.install_pre_release_version && !extension.has_release_version() {
                return Ok(());
            }
            self.base.set_hidden(false);
            self.base.set_class_name(Self::CLASS);
            if self.services.extensions_workbench_service.can_install(&extension).await?.is_ok() {
                self.base.set_enabled(true);
                self.update_label();
            }
            Ok(())
        })
    }

    fn update_label(&self) {
        self.base.set_label(self.get_label(false));
    }

    /// `getLabel(primary?)`.
    pub fn get_label(&self, primary: bool) -> String {
        let extension = self.base.extension();
        if let Some(extension) = &extension
            && extension.is_workspace_scoped()
            && extension.resource_extension().is_some_and(|resource| self.services.workspace_context_service.is_inside_workspace(&resource.location))
        {
            return localize("install workspace version", "Install Workspace Extension", &[]);
        }
        let has_pre_release_version = extension.as_ref().is_some_and(|extension| extension.has_pre_release_version());
        // install pre-release version
        if self.options.install_pre_release_version && has_pre_release_version {
            return if primary {
                localize("install pre-release", "Install Pre-Release", &[])
            } else {
                localize("install pre-release version", "Install Pre-Release Version", &[])
            };
        }
        // install released version that has a pre release version
        if has_pre_release_version {
            return if primary { localize("install", "Install", &[]) } else { localize("install release version", "Install Release Version", &[]) };
        }
        localize("install", "Install", &[])
    }
}

impl_extension_action!(InstallAction);
impl_extension_container!(InstallAction, update_state);

/// `InstallDropdownAction`.
pub struct InstallDropdownAction;

impl InstallDropdownAction {
    /// `constructor(instantiationService, extensionManagementService)`.
    pub fn new(services: &ExtensionActionServices) -> Rc<ButtonWithDropDownExtensionAction> {
        let prefer_pre_releases = services.extension_management_service.prefer_pre_releases();
        let primary: Rc<dyn IExtensionAction> = InstallAction::new(InstallOptions { install_pre_release_version: prefer_pre_releases, ..InstallOptions::default() }, services);
        let secondary: Rc<dyn IExtensionAction> = InstallAction::new(InstallOptions { install_pre_release_version: !prefer_pre_releases, ..InstallOptions::default() }, services);
        ButtonWithDropDownExtensionAction::with_label(
            "extensions.installActions",
            InstallAction::CLASS,
            vec![vec![primary, secondary]],
            Box::new(|action| match action.as_any().downcast_ref::<InstallAction>() {
                Some(install) => install.get_label(true),
                None => action.label(),
            }),
        )
    }

    /// `set manifest(manifest)`.
    pub fn set_manifest(button: &ButtonWithDropDownExtensionAction, manifest: Option<ExtensionManifest>) {
        for action in &button.extension_actions {
            if let Some(install) = action.as_any().downcast_ref::<InstallAction>() {
                install.set_manifest(manifest.clone());
            }
        }
        button.update_with(false);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// InstallingLabelAction.
// ---------------------------------------------------------------------------------------------------------------------

/// `InstallingLabelAction`.
pub struct InstallingLabelAction {
    base: ExtensionAction,
}

impl InstallingLabelAction {
    const CLASS: &'static str = "extension-action label install installing";

    pub fn new() -> Rc<Self> {
        Rc::new(Self { base: ExtensionAction::new("extension.installing", &localize("installing", "Installing", &[]), Self::CLASS, false) })
    }

    fn update_state(&self) {
        let installing = self.base.extension().is_some_and(|extension| extension.state() == ExtensionState::Installing);
        self.base.set_class_name(&format!("{}{}", Self::CLASS, if installing { "" } else { " hide" }));
    }
}

impl_extension_action!(InstallingLabelAction);
impl_extension_container!(InstallingLabelAction, update_state);

// ---------------------------------------------------------------------------------------------------------------------
// InstallInOtherServerAction.
// ---------------------------------------------------------------------------------------------------------------------

/// O texto de instalar de cada subclasse de `InstallInOtherServerAction` (`getInstallLabel()`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum InstallLabel {
    Remote,
    Local,
    Web,
}

/// `InstallInOtherServerAction` (a classe abstrata e as três concretas: [`RemoteInstallAction`], [`LocalInstallAction`] e
/// [`WebInstallAction`]).
pub struct InstallInOtherServerAction {
    base: ExtensionAction,
    services: ExtensionActionServices,
    server: Option<Rc<IExtensionManagementServer>>,
    can_install_anywhere: bool,
    install_label: InstallLabel,
}

impl InstallInOtherServerAction {
    const CLASS: &'static str = "extension-action label prominent install-other-server";
    const INSTALLING_CLASS: &'static str = "extension-action label install-other-server installing";

    fn create(id: &str, server: Option<Rc<IExtensionManagementServer>>, can_install_anywhere: bool, install_label: InstallLabel, services: &ExtensionActionServices) -> Rc<Self> {
        let this = Rc::new(Self {
            base: ExtensionAction::new(id, &localize("install", "Install", &[]), Self::CLASS, false),
            services: services.clone(),
            server,
            can_install_anywhere,
            install_label,
        });
        this.base.set_update_when_counter_extension_changes(true);
        this.update_state();
        this
    }

    fn get_install_label(&self) -> String {
        match self.install_label {
            InstallLabel::Remote => match self.services.extension_management_server_service.remote_extension_management_server() {
                Some(server) => localize("install in remote", "Install in {0}", &[server.label.as_str()]),
                None => localize("install", "Install", &[]),
            },
            InstallLabel::Local => localize("install locally", "Install Locally", &[]),
            InstallLabel::Web => localize("install browser", "Install in Browser", &[]),
        }
    }

    fn update_state(&self) {
        self.base.set_enabled(false);
        self.base.set_class_name(Self::CLASS);

        if self.can_install() {
            let extension = self.base.extension().expect("canInstall exige a extensão");
            let extension_in_other_server = self
                .services
                .extensions_workbench_service
                .installed()
                .into_iter()
                .find(|e| are_same_extensions(&e.identifier(), &extension.identifier()) && same_server(&e.server(), &self.server));
            match extension_in_other_server {
                Some(extension_in_other_server) => {
                    // Getting installed in other server
                    if extension_in_other_server.state() == ExtensionState::Installing && extension_in_other_server.local().is_none() {
                        self.base.set_enabled(true);
                        self.base.set_label(localize("installing", "Installing", &[]));
                        self.base.set_class_name(Self::INSTALLING_CLASS);
                    }
                }
                None => {
                    // Not installed in other server
                    self.base.set_enabled(true);
                    self.base.set_label(self.get_install_label());
                }
            }
        }
    }

    fn can_install(&self) -> bool {
        let services = &self.services;
        // Disable if extension is not installed or not an user extension
        let Some(extension) = self.base.extension() else { return false };
        let Some(server) = &self.server else { return false };
        let Some(local) = extension.local() else { return false };
        if extension.state() != ExtensionState::Installed
            || extension.extension_type() != ExtensionType::User
            || matches!(
                extension.enablement_state(),
                EnablementState::DisabledByEnvironment | EnablementState::DisabledByTrustRequirement | EnablementState::DisabledByVirtualWorkspace
            )
        {
            return false;
        }

        if is_language_pack_extension(&local.manifest) {
            return true;
        }

        let is_server = |other: Option<Rc<IExtensionManagementServer>>| other.is_some_and(|other| Rc::ptr_eq(server, &other));
        let local_server = services.extension_management_server_service.local_extension_management_server();
        let remote_server = services.extension_management_server_service.remote_extension_management_server();
        let web_server = services.extension_management_server_service.web_extension_management_server();
        let manifest_properties = &services.extension_manifest_properties_service;

        // Prefers to run on UI
        if is_server(local_server.clone()) && manifest_properties.prefers_execute_on_ui(&local.manifest) {
            return true;
        }

        // Prefers to run on Workspace
        if is_server(remote_server.clone()) && manifest_properties.prefers_execute_on_workspace(&local.manifest) {
            return true;
        }

        // Prefers to run on Web
        if is_server(web_server) && manifest_properties.prefers_execute_on_web(&local.manifest) {
            return true;
        }

        if self.can_install_anywhere {
            // Can run on UI
            if is_server(local_server) && manifest_properties.can_execute_on_ui(&local.manifest) {
                return true;
            }

            // Can run on Workspace
            if is_server(remote_server) && manifest_properties.can_execute_on_workspace(&local.manifest) {
                return true;
            }
        }

        false
    }

    fn run_action(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        let Some(extension) = self.base.extension() else { return undefined() };
        if extension.local().is_none() || extension.server().is_none() {
            return undefined();
        }
        let Some(server) = self.server.clone() else { return undefined() };
        self.services.extensions_workbench_service.install_in_server(extension, server, None).then(|()| Dynamic::Undefined)
    }
}

impl_extension_action!(InstallInOtherServerAction, run = run_action);
impl_extension_container!(InstallInOtherServerAction, update_state);

/// `RemoteInstallAction`.
pub struct RemoteInstallAction;

impl RemoteInstallAction {
    /// `constructor(canInstallAnyWhere, ...services)`.
    pub fn new(can_install_anywhere: bool, services: &ExtensionActionServices) -> Rc<InstallInOtherServerAction> {
        InstallInOtherServerAction::create(
            "extensions.remoteinstall",
            services.extension_management_server_service.remote_extension_management_server(),
            can_install_anywhere,
            InstallLabel::Remote,
            services,
        )
    }
}

/// `LocalInstallAction`.
pub struct LocalInstallAction;

impl LocalInstallAction {
    pub fn new(services: &ExtensionActionServices) -> Rc<InstallInOtherServerAction> {
        InstallInOtherServerAction::create("extensions.localinstall", services.extension_management_server_service.local_extension_management_server(), false, InstallLabel::Local, services)
    }
}

/// `WebInstallAction`.
pub struct WebInstallAction;

impl WebInstallAction {
    pub fn new(services: &ExtensionActionServices) -> Rc<InstallInOtherServerAction> {
        InstallInOtherServerAction::create("extensions.webInstall", services.extension_management_server_service.web_extension_management_server(), false, InstallLabel::Web, services)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// UninstallAction.
// ---------------------------------------------------------------------------------------------------------------------

/// `UninstallAction`.
pub struct UninstallAction {
    base: ExtensionAction,
    services: ExtensionActionServices,
}

impl UninstallAction {
    pub const UNINSTALL_CLASS: &'static str = "extension-action label uninstall";
    const UNINSTALLING_CLASS: &'static str = "extension-action label uninstall uninstalling";

    fn uninstall_label() -> String {
        localize("uninstallAction", "Uninstall", &[])
    }

    pub fn new(services: &ExtensionActionServices) -> Rc<Self> {
        let this = Rc::new(Self { base: ExtensionAction::new("extensions.uninstall", &Self::uninstall_label(), Self::UNINSTALL_CLASS, false), services: services.clone() });
        this.update_state();
        this
    }

    fn update_state(&self) {
        let Some(extension) = self.base.extension() else {
            self.base.set_enabled(false);
            return;
        };

        let state = extension.state();

        if state == ExtensionState::Uninstalling {
            self.base.set_label(localize("Uninstalling", "Uninstalling", &[]));
            self.base.set_class_name(Self::UNINSTALLING_CLASS);
            self.base.set_enabled(false);
            return;
        }

        let application_scoped_in_many_profiles =
            extension.local().is_some_and(|local| local.is_application_scoped) && self.services.user_data_profiles_service.profiles().len() > 1;
        self.base.set_label(if application_scoped_in_many_profiles { localize("uninstallAll", "Uninstall (All Profiles)", &[]) } else { Self::uninstall_label() });
        self.base.set_class_name(Self::UNINSTALL_CLASS);
        self.base.set_tooltip(Self::uninstall_label());

        if state != ExtensionState::Installed {
            self.base.set_enabled(false);
            return;
        }

        if extension.is_builtin() {
            self.base.set_enabled(false);
            return;
        }

        self.base.set_enabled(true);
    }

    fn run_action(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        let Some(extension) = self.base.extension() else { return undefined() };
        let workbench_service = self.services.extensions_workbench_service.clone();
        let dialog_service = self.services.dialog_service.clone();
        async_fn(async move {
            if let Err(error) = workbench_service.uninstall(&extension).await
                && !is_cancellation_error(&error)
            {
                dialog_service.error(&to_error_message_of(&error, false), None);
            }
            Ok(Dynamic::Undefined)
        })
    }
}

impl_extension_action!(UninstallAction, run = run_action);
impl_extension_container!(UninstallAction, update_state);

// ---------------------------------------------------------------------------------------------------------------------
// UpdateAction.
// ---------------------------------------------------------------------------------------------------------------------

/// `UpdateAction`.
pub struct UpdateAction {
    this: Weak<UpdateAction>,
    base: ExtensionAction,
    services: ExtensionActionServices,
    verbose: bool,
    update_throttler: Rc<Throttler<()>>,
}

impl UpdateAction {
    const ENABLED_CLASS: &'static str = "extension-action label update";
    const DISABLED_CLASS: &'static str = "extension-action label update disabled";

    /// `constructor(verbose, ...services)`.
    pub fn new(verbose: bool, services: &ExtensionActionServices) -> Rc<Self> {
        let this = Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            base: ExtensionAction::new("extensions.update", &localize("update", "Update", &[]), Self::DISABLED_CLASS, false),
            services: services.clone(),
            verbose,
            update_throttler: Rc::new(Throttler::new()),
        });
        this.base.register(this.update_throttler.clone());
        this.update_state();
        this
    }

    fn update_state(&self) {
        if let Some(this) = self.this.upgrade() {
            self.update_throttler.queue(move |_| this.compute_and_update_enablement());
        }
        if let Some(extension) = self.base.extension() {
            self.base.set_label(if self.verbose {
                localize("update to", "Update to v{0}", &[extension.latest_version().as_str()])
            } else {
                localize("update", "Update", &[])
            });
        }
    }

    /// `computeAndUpdateEnablement()`.
    fn compute_and_update_enablement(self: Rc<Self>) -> Promise<()> {
        async_fn(async move {
            self.base.set_enabled(false);
            self.base.set_class_name(Self::DISABLED_CLASS);

            let Some(extension) = self.base.extension() else { return Ok(()) };

            if extension.deprecation_info().is_some() {
                return Ok(());
            }

            let can_install = self.services.extensions_workbench_service.can_install(&extension).await?;
            let is_installed = extension.state() == ExtensionState::Installed;

            self.base.set_enabled(can_install.is_ok() && is_installed && extension.outdated());
            self.base.set_class_name(if self.base.enabled() { Self::ENABLED_CLASS } else { Self::DISABLED_CLASS });
            Ok(())
        })
    }
}

impl_extension_action!(UpdateAction);
impl_extension_container!(UpdateAction, update_state);

// ---------------------------------------------------------------------------------------------------------------------
// ManageExtensionAction.
// ---------------------------------------------------------------------------------------------------------------------

/// `ManageExtensionAction`.
pub struct ManageExtensionAction {
    base: ExtensionAction,
}

impl ManageExtensionAction {
    pub const ID: &'static str = "extensions.manage";
    const CLASS: &'static str = "extension-action icon manage codicon codicon-extensions-manage";
    const HIDE_MANAGE_EXTENSION_CLASS: &'static str = "extension-action icon manage codicon codicon-extensions-manage hide";

    pub fn new() -> Rc<Self> {
        let this = Rc::new(Self { base: ExtensionAction::new(Self::ID, "", "", true) });
        this.base.set_tooltip(localize("manage", "Manage", &[]));
        this.update_state();
        this
    }

    fn update_state(&self) {
        self.base.set_class_name(Self::HIDE_MANAGE_EXTENSION_CLASS);
        self.base.set_enabled(false);
        if let Some(extension) = self.base.extension() {
            let state = extension.state();
            self.base.set_enabled(state == ExtensionState::Installed);
            self.base.set_class_name(if self.base.enabled() || state == ExtensionState::Uninstalling { Self::CLASS } else { Self::HIDE_MANAGE_EXTENSION_CLASS });
        }
    }
}

impl_extension_action!(ManageExtensionAction);
impl_extension_container!(ManageExtensionAction, update_state);

// ---------------------------------------------------------------------------------------------------------------------
// Habilitar e desabilitar.
// ---------------------------------------------------------------------------------------------------------------------

/// `EnableForWorkspaceAction`.
pub struct EnableForWorkspaceAction {
    base: ExtensionAction,
    services: ExtensionActionServices,
}

impl EnableForWorkspaceAction {
    pub const ID: &'static str = "extensions.enableForWorkspace";

    pub fn new(services: &ExtensionActionServices) -> Rc<Self> {
        let this = Rc::new(Self {
            base: ExtensionAction::new(Self::ID, &localize("enableForWorkspaceAction", "Enable (Workspace)", &[]), LABEL_ACTION_CLASS, true),
            services: services.clone(),
        });
        this.base.set_tooltip(localize("enableForWorkspaceActionToolTip", "Enable this extension only in this workspace", &[]));
        this.update_state();
        this
    }

    fn update_state(&self) {
        self.base.set_enabled(false);
        if let Some(extension) = self.base.extension()
            && let Some(local) = extension.local()
            && !extension.is_workspace_scoped()
        {
            if is_chat_extension(&*self.services.product_service, &extension.identifier().id) {
                return;
            }
            let enablement = &self.services.extension_enablement_service;
            self.base.set_enabled(
                extension.state() == ExtensionState::Installed && !enablement.is_enabled(&local) && enablement.can_change_workspace_enablement(&local),
            );
        }
    }

    fn run_action(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        let Some(extension) = self.base.extension() else { return undefined() };
        self.services.extensions_workbench_service.set_enablement(vec![extension], EnablementState::EnabledWorkspace).then(|()| Dynamic::Undefined)
    }
}

impl_extension_action!(EnableForWorkspaceAction, run = run_action);
impl_extension_container!(EnableForWorkspaceAction, update_state);

/// `EnableGloballyAction`.
pub struct EnableGloballyAction {
    base: ExtensionAction,
    services: ExtensionActionServices,
}

impl EnableGloballyAction {
    pub const ID: &'static str = "extensions.enableGlobally";

    pub fn new(services: &ExtensionActionServices) -> Rc<Self> {
        let this = Rc::new(Self {
            base: ExtensionAction::new(Self::ID, &localize("enableGloballyAction", "Enable", &[]), LABEL_ACTION_CLASS, true),
            services: services.clone(),
        });
        this.base.set_tooltip(localize("enableGloballyActionToolTip", "Enable this extension", &[]));
        this.update_state();
        this
    }

    fn update_state(&self) {
        self.base.set_enabled(false);
        if let Some(extension) = self.base.extension()
            && let Some(local) = extension.local()
            && !extension.is_workspace_scoped()
        {
            if is_chat_extension(&*self.services.product_service, &extension.identifier().id) {
                return;
            }
            let enablement = &self.services.extension_enablement_service;
            self.base.set_enabled(
                extension.state() == ExtensionState::Installed && enablement.is_disabled_globally(&local) && enablement.can_change_enablement(&local),
            );
        }
    }

    fn run_action(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        let Some(extension) = self.base.extension() else { return undefined() };
        self.services.extensions_workbench_service.set_enablement(vec![extension], EnablementState::EnabledGlobally).then(|()| Dynamic::Undefined)
    }
}

impl_extension_action!(EnableGloballyAction, run = run_action);
impl_extension_container!(EnableGloballyAction, update_state);

/// `DisableForWorkspaceAction`.
pub struct DisableForWorkspaceAction {
    base: ExtensionAction,
    services: ExtensionActionServices,
}

impl DisableForWorkspaceAction {
    pub const ID: &'static str = "extensions.disableForWorkspace";

    pub fn new(services: &ExtensionActionServices) -> Rc<Self> {
        let this = Rc::new(Self {
            base: ExtensionAction::new(Self::ID, &localize("disableForWorkspaceAction", "Disable (Workspace)", &[]), LABEL_ACTION_CLASS, true),
            services: services.clone(),
        });
        this.base.set_tooltip(localize("disableForWorkspaceActionToolTip", "Disable this extension only in this workspace", &[]));
        this.update_state();
        register_update_listener!(this, services.extension_service.on_did_change_extensions(), update_state);
        this
    }

    fn update_state(&self) {
        self.base.set_enabled(false);
        if let Some(extension) = self.base.extension()
            && let Some(local) = extension.local()
            && !extension.is_workspace_scoped()
            && self
                .services
                .extension_service
                .extensions()
                .iter()
                .any(|e| are_same_extensions(&running_identifier(e), &extension.identifier()) && self.services.workspace_context_service.get_workbench_state() != WorkbenchState::Empty)
        {
            if is_chat_extension(&*self.services.product_service, &extension.identifier().id) {
                return;
            }
            let enablement_state = extension.enablement_state();
            self.base.set_enabled(
                extension.state() == ExtensionState::Installed
                    && matches!(enablement_state, EnablementState::EnabledGlobally | EnablementState::EnabledWorkspace)
                    && self.services.extension_enablement_service.can_change_workspace_enablement(&local),
            );
        }
    }

    fn run_action(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        let Some(extension) = self.base.extension() else { return undefined() };
        self.services.extensions_workbench_service.set_enablement(vec![extension], EnablementState::DisabledWorkspace).then(|()| Dynamic::Undefined)
    }
}

impl_extension_action!(DisableForWorkspaceAction, run = run_action);
impl_extension_container!(DisableForWorkspaceAction, update_state);

/// `DisableGloballyAction`.
pub struct DisableGloballyAction {
    base: ExtensionAction,
    services: ExtensionActionServices,
}

impl DisableGloballyAction {
    pub const ID: &'static str = "extensions.disableGlobally";

    pub fn new(services: &ExtensionActionServices) -> Rc<Self> {
        let this = Rc::new(Self {
            base: ExtensionAction::new(Self::ID, &localize("disableGloballyAction", "Disable", &[]), LABEL_ACTION_CLASS, true),
            services: services.clone(),
        });
        this.base.set_tooltip(localize("disableGloballyActionToolTip", "Disable this extension", &[]));
        this.update_state();
        register_update_listener!(this, services.extension_service.on_did_change_extensions(), update_state);
        this
    }

    fn update_state(&self) {
        self.base.set_enabled(false);
        if let Some(extension) = self.base.extension()
            && let Some(local) = extension.local()
            && !extension.is_workspace_scoped()
            && self.services.extension_service.extensions().iter().any(|e| are_same_extensions(&running_identifier(e), &extension.identifier()))
        {
            if is_chat_extension(&*self.services.product_service, &extension.identifier().id) {
                return;
            }
            let enablement_state = extension.enablement_state();
            self.base.set_enabled(
                extension.state() == ExtensionState::Installed
                    && matches!(enablement_state, EnablementState::EnabledGlobally | EnablementState::EnabledWorkspace)
                    && self.services.extension_enablement_service.can_change_enablement(&local),
            );
        }
    }

    fn run_action(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        let Some(extension) = self.base.extension() else { return undefined() };
        self.services.extensions_workbench_service.set_enablement(vec![extension], EnablementState::DisabledGlobally).then(|()| Dynamic::Undefined)
    }
}

impl_extension_action!(DisableGloballyAction, run = run_action);
impl_extension_container!(DisableGloballyAction, update_state);

/// `inspect(ChatAIDisabledSettingId)`.
fn inspect_ai_disabled(configuration_service: &dyn IConfigurationService) -> Rc<dyn je_platform::configuration::ConfigurationValue> {
    configuration_service.inspect(CHAT_AI_DISABLED_SETTING_ID, &ConfigurationOverrides::default())
}

/// `configurationService.getValue<boolean>(ChatAIDisabledSettingId) === true`.
fn is_ai_disabled(configuration_service: &dyn IConfigurationService) -> bool {
    matches!(configuration_service.get_value(Some(CHAT_AI_DISABLED_SETTING_ID), None), Dynamic::Bool(true))
}

/// Chama `$update` quando a configuração `ChatAIDisabledSettingId` muda.
macro_rules! register_ai_setting_listener {
    ($this:expr, $services:expr) => {{
        let weak = Rc::downgrade(&$this);
        $this.base.register($services.configuration_service.on_did_change_configuration().listen(move |event| {
            if event.affects_configuration(CHAT_AI_DISABLED_SETTING_ID, None)
                && let Some(this) = weak.upgrade()
            {
                this.update_state();
            }
        }));
    }};
}

/// `EnableAIFeaturesGloballyAction`.
pub struct EnableAIFeaturesGloballyAction {
    base: ExtensionAction,
    services: ExtensionActionServices,
}

impl EnableAIFeaturesGloballyAction {
    pub const ID: &'static str = "extensions.enableAIGlobally";

    pub fn new(services: &ExtensionActionServices) -> Rc<Self> {
        let this = Rc::new(Self {
            base: ExtensionAction::new(Self::ID, &localize("enableAIGloballyAction", "Enable AI Features", &[]), LABEL_ACTION_CLASS, true),
            services: services.clone(),
        });
        this.base.set_tooltip(localize("enableAIGloballyActionToolTip", "Enable AI features", &[]));
        this.update_state();
        register_ai_setting_listener!(this, services);
        this
    }

    fn update_state(&self) {
        self.base.set_enabled(false);
        let Some(extension) = self.base.extension() else { return };
        if extension.local().is_none() {
            return;
        }
        if !is_chat_extension(&*self.services.product_service, &extension.identifier().id) {
            return;
        }
        if extension.enablement_state() == EnablementState::DisabledWorkspace {
            return;
        }
        if extension.enablement_state() == EnablementState::EnabledWorkspace {
            return;
        }
        let inspect = inspect_ai_disabled(&*self.services.configuration_service);
        if matches!(inspect.workspace_value(), Dynamic::Bool(true)) {
            return;
        }
        self.base.set_enabled(matches!(inspect.value(), Dynamic::Bool(true)));
    }

    fn run_action(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        self.services.configuration_service.update_value(CHAT_AI_DISABLED_SETTING_ID, Dynamic::Bool(false), None, None).then(|()| Dynamic::Undefined)
    }
}

impl_extension_action!(EnableAIFeaturesGloballyAction, run = run_action);
impl_extension_container!(EnableAIFeaturesGloballyAction, update_state);

/// `EnableAIFeaturesInWorkspaceAction`.
pub struct EnableAIFeaturesInWorkspaceAction {
    base: ExtensionAction,
    services: ExtensionActionServices,
}

impl EnableAIFeaturesInWorkspaceAction {
    pub const ID: &'static str = "extensions.enableAIInWorkspace";

    pub fn new(services: &ExtensionActionServices) -> Rc<Self> {
        let this = Rc::new(Self {
            base: ExtensionAction::new(Self::ID, &localize("enableAIInWorkspaceAction", "Enable AI Features (Workspace)", &[]), LABEL_ACTION_CLASS, true),
            services: services.clone(),
        });
        this.base.set_tooltip(localize("enableAIInWorkspaceActionToolTip", "Enable AI features in this workspace", &[]));
        this.update_state();
        register_ai_setting_listener!(this, services);
        this
    }

    fn update_state(&self) {
        self.base.set_enabled(false);
        let Some(extension) = self.base.extension() else { return };
        let Some(local) = extension.local() else { return };
        if !is_chat_extension(&*self.services.product_service, &extension.identifier().id) {
            return;
        }
        if !self.services.extension_enablement_service.can_change_workspace_enablement(&local) {
            return;
        }
        let inspect = inspect_ai_disabled(&*self.services.configuration_service);
        if matches!(inspect.value(), Dynamic::Bool(false)) {
            return;
        }
        if matches!(inspect.workspace_value(), Dynamic::Bool(true)) {
            self.base.set_enabled(true);
            return;
        }
        if extension.enablement_state() == EnablementState::EnabledWorkspace {
            return;
        }
        self.base.set_enabled(true);
    }

    fn run_action(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        let Some(extension) = self.base.extension() else { return undefined() };
        let workbench_service = self.services.extensions_workbench_service.clone();
        let configuration_service = self.services.configuration_service.clone();
        async_fn(async move {
            workbench_service.set_enablement(vec![extension], EnablementState::EnabledWorkspace).await?;
            if is_ai_disabled(&*configuration_service) {
                configuration_service.update_value(CHAT_AI_DISABLED_SETTING_ID, Dynamic::Bool(false), None, Some(ConfigurationTarget::Workspace)).await?;
            }
            Ok(Dynamic::Undefined)
        })
    }
}

impl_extension_action!(EnableAIFeaturesInWorkspaceAction, run = run_action);
impl_extension_container!(EnableAIFeaturesInWorkspaceAction, update_state);

/// `DisableAIFeaturesGloballyAction`.
pub struct DisableAIFeaturesGloballyAction {
    base: ExtensionAction,
    services: ExtensionActionServices,
}

impl DisableAIFeaturesGloballyAction {
    pub const ID: &'static str = "extensions.disableAIGlobally";

    pub fn new(services: &ExtensionActionServices) -> Rc<Self> {
        let this = Rc::new(Self {
            base: ExtensionAction::new(Self::ID, &localize("disableAIGloballyAction", "Disable AI Features", &[]), LABEL_ACTION_CLASS, true),
            services: services.clone(),
        });
        this.base.set_tooltip(localize("disableAIGloballyActionToolTip", "Disable AI features", &[]));
        this.update_state();
        register_ai_setting_listener!(this, services);
        this
    }

    fn update_state(&self) {
        self.base.set_enabled(false);
        if let Some(extension) = self.base.extension()
            && is_chat_extension(&*self.services.product_service, &extension.identifier().id)
        {
            self.base.set_enabled(
                extension.state() == ExtensionState::Installed
                    && !is_ai_disabled(&*self.services.configuration_service)
                    && extension.enablement_state() != EnablementState::DisabledWorkspace,
            );
        }
    }

    fn run_action(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        self.services.configuration_service.update_value(CHAT_AI_DISABLED_SETTING_ID, Dynamic::Bool(true), None, None).then(|()| Dynamic::Undefined)
    }
}

impl_extension_action!(DisableAIFeaturesGloballyAction, run = run_action);
impl_extension_container!(DisableAIFeaturesGloballyAction, update_state);

/// `DisableAIFeaturesInWorkspaceAction`.
pub struct DisableAIFeaturesInWorkspaceAction {
    base: ExtensionAction,
    services: ExtensionActionServices,
}

impl DisableAIFeaturesInWorkspaceAction {
    pub const ID: &'static str = "extensions.disableAIInWorkspace";

    pub fn new(services: &ExtensionActionServices) -> Rc<Self> {
        let this = Rc::new(Self {
            base: ExtensionAction::new(Self::ID, &localize("disableAIInWorkspaceAction", "Disable AI Features (Workspace)", &[]), LABEL_ACTION_CLASS, true),
            services: services.clone(),
        });
        this.base.set_tooltip(localize("disableAIInWorkspaceActionToolTip", "Disable AI features in this workspace", &[]));
        this.update_state();
        register_update_listener!(this, services.extension_service.on_did_change_extensions(), update_state);
        this
    }

    fn update_state(&self) {
        self.base.set_enabled(false);
        if let Some(extension) = self.base.extension()
            && let Some(local) = extension.local()
            && is_chat_extension(&*self.services.product_service, &extension.identifier().id)
        {
            let enablement_state = extension.enablement_state();
            self.base.set_enabled(
                extension.state() == ExtensionState::Installed
                    && matches!(enablement_state, EnablementState::EnabledGlobally | EnablementState::EnabledWorkspace)
                    && self.services.extension_enablement_service.can_change_workspace_enablement(&local),
            );
        }
    }

    fn run_action(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        let Some(extension) = self.base.extension() else { return undefined() };
        let workbench_service = self.services.extensions_workbench_service.clone();
        async_fn(async move {
            workbench_service.set_enablement(vec![extension], EnablementState::DisabledWorkspace).await?;
            workbench_service.update_running_extensions(Some(localize("restartExtensionHost.reason.disable", "Disabling AI features", &[])), false).await?;
            Ok(Dynamic::Undefined)
        })
    }
}

impl_extension_action!(DisableAIFeaturesInWorkspaceAction, run = run_action);
impl_extension_container!(DisableAIFeaturesInWorkspaceAction, update_state);

/// `EnableDropDownAction`.
pub struct EnableDropDownAction;

impl EnableDropDownAction {
    pub fn new(services: &ExtensionActionServices) -> Rc<ButtonWithDropDownExtensionAction> {
        let enable: Vec<Rc<dyn IExtensionAction>> = vec![EnableGloballyAction::new(services), EnableForWorkspaceAction::new(services)];
        let enable_ai: Vec<Rc<dyn IExtensionAction>> = vec![EnableAIFeaturesGloballyAction::new(services), EnableAIFeaturesInWorkspaceAction::new(services)];
        ButtonWithDropDownExtensionAction::new("extensions.enable", LABEL_ACTION_CLASS, vec![enable, enable_ai])
    }
}

/// `DisableDropDownAction`.
pub struct DisableDropDownAction;

impl DisableDropDownAction {
    pub fn new(services: &ExtensionActionServices) -> Rc<ButtonWithDropDownExtensionAction> {
        let disable: Vec<Rc<dyn IExtensionAction>> = vec![DisableGloballyAction::new(services), DisableForWorkspaceAction::new(services)];
        let disable_ai: Vec<Rc<dyn IExtensionAction>> = vec![DisableAIFeaturesGloballyAction::new(services), DisableAIFeaturesInWorkspaceAction::new(services)];
        ButtonWithDropDownExtensionAction::new("extensions.disable", LABEL_ACTION_CLASS, vec![disable, disable_ai])
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// ExtensionRuntimeStateAction.
// ---------------------------------------------------------------------------------------------------------------------

/// `ExtensionRuntimeStateAction`.
pub struct ExtensionRuntimeStateAction {
    base: ExtensionAction,
    services: ExtensionActionServices,
}

impl ExtensionRuntimeStateAction {
    const ENABLED_CLASS: &'static str = "extension-action label prominent reload";
    const DISABLED_CLASS: &'static str = "extension-action label prominent reload disabled";

    pub fn new(services: &ExtensionActionServices) -> Rc<Self> {
        let this = Rc::new(Self { base: ExtensionAction::new("extensions.runtimeState", "", Self::DISABLED_CLASS, false), services: services.clone() });
        this.base.set_update_when_counter_extension_changes(true);
        register_update_listener!(this, services.extension_service.on_did_change_extensions(), update_state);
        this.update_state();
        this
    }

    fn update_state(&self) {
        self.base.set_enabled(false);
        self.base.set_tooltip(String::new());
        self.base.set_class_name(Self::DISABLED_CLASS);

        let Some(extension) = self.base.extension() else { return };

        let state = extension.state();
        if state == ExtensionState::Installing || state == ExtensionState::Uninstalling {
            return;
        }

        if extension.local().is_some_and(|local| is_language_pack_extension(&local.manifest)) {
            return;
        }

        let Some(runtime_state) = extension.runtime_state() else { return };

        self.base.set_enabled(true);
        self.base.set_class_name(Self::ENABLED_CLASS);
        self.base.set_tooltip(runtime_state.reason.clone());
        self.base.set_label(match runtime_state.action {
            ExtensionRuntimeActionType::ReloadWindow => localize("reload window", "Reload Window", &[]),
            ExtensionRuntimeActionType::RestartExtensions => localize("restart extensions", "Restart Extensions", &[]),
            ExtensionRuntimeActionType::QuitAndInstall => localize("restart product", "Restart to Update", &[]),
            ExtensionRuntimeActionType::ApplyUpdate | ExtensionRuntimeActionType::DownloadUpdate => {
                localize("update product", "Update {0}", &[self.services.product_service.name_short()])
            }
        });
    }

    fn run_action(&self, _args: &[Dynamic]) -> Promise<Dynamic> {
        let Some(runtime_state) = self.base.extension().and_then(|extension| extension.runtime_state()) else { return undefined() };

        self.services.telemetry_service.public_log2("extensions:runtimestate:action", Some(Dynamic::object([("action", Dynamic::string(runtime_state.action.as_str()))])));

        match runtime_state.action {
            ExtensionRuntimeActionType::ReloadWindow => self.services.host_service.reload(None).then(|()| Dynamic::Undefined),
            ExtensionRuntimeActionType::RestartExtensions => self.services.extensions_workbench_service.update_running_extensions(None, false).then(|()| Dynamic::Undefined),
            ExtensionRuntimeActionType::DownloadUpdate => self.services.update_service.download_update(true).then(|()| Dynamic::Undefined),
            ExtensionRuntimeActionType::ApplyUpdate => self.services.update_service.apply_update().then(|()| Dynamic::Undefined),
            ExtensionRuntimeActionType::QuitAndInstall => self.services.update_service.quit_and_install().then(|()| Dynamic::Undefined),
        }
    }
}

impl_extension_action!(ExtensionRuntimeStateAction, run = run_action);
impl_extension_container!(ExtensionRuntimeStateAction, update_state);
