//! Porte de `src/vs/workbench/contrib/relauncher/browser/relauncher.contribution.ts`: o `SettingsChangeRelauncher`, que
//! observa as configurações que só valem depois de reiniciar a aplicação e pergunta ao usuário se quer reiniciar quando
//! uma delas muda.
//!
//! Recortes:
//! - `WorkspaceChangeExtHostRelauncher` (o reinício do host de extensões quando a primeira pasta do workspace muda) fica
//!   para quando `IExtensionService` e o `IWorkbenchEnvironmentService` completo estiverem portados; o oráculo não o
//!   cobre em `relauncher.test.ts`.
//! - O `registerWorkbenchContribution(SettingsChangeRelauncher, LifecyclePhase.Restored)` do fim do módulo fica com quem
//!   monta o workbench.
//!
//! Divergências:
//! - `isNative`, `isMacintosh` e `isLinux` são constantes de módulo no original; aqui entram pelo [`Platform`] que quem
//!   inicializa passa ([`Platform::detect`]).
//! - `IUserDataSyncService`, `IUserDataSyncEnablementService`, `IUserDataSyncWorkbenchService` e `IDialogService` entram
//!   pelos contratos estreitos [`RelauncherUserDataSyncService`], [`RelauncherUserDataSyncEnablementService`],
//!   [`RelauncherUserDataSyncWorkbenchService`] e [`RelauncherDialogService`] (só o que a contribuição lê). Os serviços
//!   reais (`IUserDataSyncService`, `IUserDataSyncEnablementService` e `IDialogService`) já os implementam; o
//!   `IUserDataSyncWorkbenchService` ainda não foi portado e só o evento `onDidTurnOnSync` é pedido.
//! - `IWindowsConfiguration` e as constantes de `platform/window/common/window.ts` (`TitleBarSetting.TITLE_BAR_STYLE`,
//!   `MenuSettings.MenuStyle`, `TitlebarStyle`) são as constantes deste módulo; a configuração é lida como [`Dynamic`], e
//!   o acesso sem `?.` ao `config.window` lança `TypeError` quando falta, como o JavaScript.
//! - `ChangeObserver<T>` guarda o último valor como [`Dynamic`] e compara com `===` ([`Dynamic::strict_equals`]).
//! - O construtor devolve `Rc<Self>` e os ouvintes seguram a contribuição por referência fraca.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_platform::configuration::ConfigurationTarget;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;
use je_platform::dialogs::dialogs::{BaseDialogOptions, Confirmation, ConfirmationResult, DialogDetail, IDialogService};
use je_platform::product::IProductService;
use je_platform::user_data_sync::user_data_sync::{IUserDataSyncEnablementService, IUserDataSyncService, SyncStatus};

use crate::services::host::host::IHostService;

/// `TitleBarSetting.TITLE_BAR_STYLE`.
const TITLE_BAR_STYLE_SETTING: &str = "window.titleBarStyle";

/// `MenuSettings.MenuStyle`.
const MENU_STYLE_SETTING: &str = "window.menuStyle";

/// `TitlebarStyle.NATIVE`.
const TITLEBAR_STYLE_NATIVE: &str = "native";

/// `TitlebarStyle.CUSTOM`.
const TITLEBAR_STYLE_CUSTOM: &str = "custom";

/// `IUserDataSyncService`, só o `status`.
pub trait RelauncherUserDataSyncService {
    fn status(&self) -> SyncStatus;
}

impl<T: IUserDataSyncService + ?Sized> RelauncherUserDataSyncService for T {
    fn status(&self) -> SyncStatus {
        IUserDataSyncService::status(self)
    }
}

/// `IUserDataSyncEnablementService`, só o `isEnabled`.
pub trait RelauncherUserDataSyncEnablementService {
    fn is_enabled(&self) -> bool;
}

impl<T: IUserDataSyncEnablementService + ?Sized> RelauncherUserDataSyncEnablementService for T {
    fn is_enabled(&self) -> bool {
        IUserDataSyncEnablementService::is_enabled(self)
    }
}

/// `IUserDataSyncWorkbenchService`, só o `onDidTurnOnSync`.
pub trait RelauncherUserDataSyncWorkbenchService {
    fn on_did_turn_on_sync(&self) -> Event<()>;
}

/// `IDialogService`, só o `confirm`.
pub trait RelauncherDialogService {
    fn confirm(&self, confirmation: Confirmation) -> Promise<ConfirmationResult>;
}

impl<T: IDialogService + ?Sized> RelauncherDialogService for T {
    fn confirm(&self, confirmation: Confirmation) -> Promise<ConfirmationResult> {
        IDialogService::confirm(self, confirmation)
    }
}

/// `value?.key`.
fn optional(value: &Dynamic, key: &str) -> Dynamic {
    if value.is_nullish() { Dynamic::Undefined } else { value.get_str(key) }
}

/// `value?.a?.b?.c`.
fn optional_path(value: &Dynamic, keys: &[&str]) -> Dynamic {
    keys.iter().fold(value.clone(), |current, key| optional(&current, key))
}

/// `ChangeObserver<T>`: lembra o último valor do tipo esperado e diz se o novo é diferente.
struct ChangeObserver {
    type_name: &'static str,
    last_value: RefCell<Dynamic>,
}

impl ChangeObserver {
    fn new(type_name: &'static str) -> Self {
        Self { type_name, last_value: RefCell::new(Dynamic::Undefined) }
    }

    /// Returns if there was a change compared to the last value.
    fn handle_change(&self, value: &Dynamic) -> bool {
        if value.type_of() == self.type_name && !value.strict_equals(&self.last_value.borrow()) {
            *self.last_value.borrow_mut() = value.clone();
            return true;
        }

        false
    }
}

/// Os observadores de cada configuração (os campos `ChangeObserver` do original).
struct Observers {
    title_bar_style: ChangeObserver,
    menu_style: ChangeObserver,
    native_tabs: ChangeObserver,
    native_full_screen: ChangeObserver,
    click_through_inactive: ChangeObserver,
    controls_style: ChangeObserver,
    workspace_trust_enabled: ChangeObserver,
    experiments_enabled: ChangeObserver,
    enable_ppe_extensions_gallery: ChangeObserver,
    restrict_unc_access: ChangeObserver,
    accessibility_verbosity_debug: ChangeObserver,
    telemetry_feedback_enabled: ChangeObserver,
    extension_unification_enabled: ChangeObserver,
    agent_sessions_migrate_legacy_copilot_cli: ChangeObserver,
    agent_host_claude_agent_enabled: ChangeObserver,
    editor_codex_prefer_agent_host: ChangeObserver,
    agent_host_otel_enabled: ChangeObserver,
    agent_host_otel_exporter_type: ChangeObserver,
    agent_host_otel_otlp_endpoint: ChangeObserver,
    agent_host_otel_capture_content: ChangeObserver,
    agent_host_otel_outfile: ChangeObserver,
    agent_host_otel_db_span_exporter_enabled: ChangeObserver,
}

impl Observers {
    fn new() -> Self {
        Self {
            title_bar_style: ChangeObserver::new("string"),
            menu_style: ChangeObserver::new("string"),
            native_tabs: ChangeObserver::new("boolean"),
            native_full_screen: ChangeObserver::new("boolean"),
            click_through_inactive: ChangeObserver::new("boolean"),
            controls_style: ChangeObserver::new("string"),
            workspace_trust_enabled: ChangeObserver::new("boolean"),
            experiments_enabled: ChangeObserver::new("boolean"),
            enable_ppe_extensions_gallery: ChangeObserver::new("boolean"),
            restrict_unc_access: ChangeObserver::new("boolean"),
            accessibility_verbosity_debug: ChangeObserver::new("boolean"),
            telemetry_feedback_enabled: ChangeObserver::new("boolean"),
            extension_unification_enabled: ChangeObserver::new("boolean"),
            agent_sessions_migrate_legacy_copilot_cli: ChangeObserver::new("boolean"),
            agent_host_claude_agent_enabled: ChangeObserver::new("boolean"),
            editor_codex_prefer_agent_host: ChangeObserver::new("boolean"),
            agent_host_otel_enabled: ChangeObserver::new("boolean"),
            agent_host_otel_exporter_type: ChangeObserver::new("string"),
            agent_host_otel_otlp_endpoint: ChangeObserver::new("string"),
            agent_host_otel_capture_content: ChangeObserver::new("boolean"),
            agent_host_otel_outfile: ChangeObserver::new("string"),
            agent_host_otel_db_span_exporter_enabled: ChangeObserver::new("boolean"),
        }
    }
}

/// `SettingsChangeRelauncher`.
pub struct SettingsChangeRelauncher {
    base: Rc<Disposable>,
    platform: Platform,
    host_service: Rc<dyn IHostService>,
    configuration_service: Rc<dyn IConfigurationService>,
    user_data_sync_service: Rc<dyn RelauncherUserDataSyncService>,
    user_data_sync_enablement_service: Rc<dyn RelauncherUserDataSyncEnablementService>,
    product_service: Rc<dyn IProductService>,
    dialog_service: Rc<dyn RelauncherDialogService>,
    observers: Observers,
    accessibility_support: RefCell<Option<String>>,
}

impl SettingsChangeRelauncher {
    pub const ID: &'static str = "workbench.contrib.settingsChangeRelauncher";

    const SETTINGS: [&'static str; 23] = [
        TITLE_BAR_STYLE_SETTING,
        MENU_STYLE_SETTING,
        "window.nativeTabs",
        "window.nativeFullScreen",
        "window.clickThroughInactive",
        "window.controlsStyle",
        "editor.accessibilitySupport",
        "security.workspace.trust.enabled",
        "workbench.enableExperiments",
        "_extensionsGallery.enablePPE",
        "security.restrictUNCAccess",
        "accessibility.verbosity.debug",
        "telemetry.feedback.enabled",
        "chat.extensionUnification.enabled",
        "chat.agentSessions.migrateLegacyCopilotCli",
        "chat.agentHost.claudeAgent.enabled",
        "chat.editor.codex.preferAgentHost",
        "chat.agentHost.otel.enabled",
        "chat.agentHost.otel.exporterType",
        "chat.agentHost.otel.otlpEndpoint",
        "chat.agentHost.otel.captureContent",
        "chat.agentHost.otel.outfile",
        "chat.agentHost.otel.dbSpanExporter.enabled",
    ];

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        platform: Platform,
        host_service: Rc<dyn IHostService>,
        configuration_service: Rc<dyn IConfigurationService>,
        user_data_sync_service: Rc<dyn RelauncherUserDataSyncService>,
        user_data_sync_enablement_service: Rc<dyn RelauncherUserDataSyncEnablementService>,
        user_data_sync_workbench_service: Rc<dyn RelauncherUserDataSyncWorkbenchService>,
        product_service: Rc<dyn IProductService>,
        dialog_service: Rc<dyn RelauncherDialogService>,
    ) -> Rc<Self> {
        let this = Rc::new(Self {
            base: Disposable::new(),
            platform,
            host_service,
            configuration_service,
            user_data_sync_service,
            user_data_sync_enablement_service,
            product_service,
            dialog_service,
            observers: Observers::new(),
            accessibility_support: RefCell::new(None),
        });

        this.update(false);

        let weak: Weak<Self> = Rc::downgrade(&this);
        this.base.register(this.configuration_service.on_did_change_configuration().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_configuration_change(e);
            }
        }));
        let weak: Weak<Self> = Rc::downgrade(&this);
        this.base.register(user_data_sync_workbench_service.on_did_turn_on_sync().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update(true);
            }
        }));
        this
    }

    fn on_configuration_change(&self, e: &ConfigurationChangeEvent) {
        if !Self::SETTINGS.iter().any(|key| e.affects_configuration(*key, None)) {
            return;
        }

        // Skip if turning on sync is in progress
        if self.is_turning_on_sync_in_progress() {
            return;
        }

        self.update(e.source.get() != Some(ConfigurationTarget::Default) /* do not ask to relaunch if defaults changed */);
    }

    fn is_turning_on_sync_in_progress(&self) -> bool {
        !self.user_data_sync_enablement_service.is_enabled() && self.user_data_sync_service.status() == SyncStatus::Syncing
    }

    fn update(&self, ask_to_relaunch: bool) {
        let mut changed = false;
        let observers = &self.observers;
        let is_native = self.platform.is_native;
        let is_macintosh = self.platform.is_macintosh;

        let mut process_changed = |did_change: bool| {
            changed = changed || did_change;
        };

        let config = self.configuration_service.get_value(None, None);
        let window = optional(&config, "window");
        if is_native {
            // Titlebar style
            let title_bar_style = config.get_str("window").get_str("titleBarStyle");
            process_changed(
                (title_bar_style.strict_equals(&Dynamic::from(TITLEBAR_STYLE_NATIVE)) || title_bar_style.strict_equals(&Dynamic::from(TITLEBAR_STYLE_CUSTOM)))
                    && observers.title_bar_style.handle_change(&optional(&window, "titleBarStyle")),
            );

            // Windows/Linux: Menu style
            process_changed(!is_macintosh && observers.menu_style.handle_change(&optional(&window, "menuStyle")));

            // macOS: Native tabs
            process_changed(is_macintosh && observers.native_tabs.handle_change(&optional(&window, "nativeTabs")));

            // macOS: Native fullscreen
            process_changed(is_macintosh && observers.native_full_screen.handle_change(&optional(&window, "nativeFullScreen")));

            // macOS: Click through (accept first mouse)
            process_changed(is_macintosh && observers.click_through_inactive.handle_change(&optional(&window, "clickThroughInactive")));

            // Windows/Linux: Window controls style
            process_changed(!is_macintosh && observers.controls_style.handle_change(&optional(&window, "controlsStyle")));

            // On linux turning on accessibility support will also pass this flag to the chrome renderer, thus a restart is required
            if self.platform.is_linux
                && let Dynamic::String(support) = optional_path(&config, &["editor", "accessibilitySupport"])
            {
                let support = support.to_string_lossy();
                if self.accessibility_support.borrow().as_deref() != Some(support.as_str()) {
                    *self.accessibility_support.borrow_mut() = Some(support.clone());
                    if support == "on" {
                        process_changed(true);
                    }
                }
            }

            // Workspace trust
            process_changed(observers.workspace_trust_enabled.handle_change(&optional_path(&config, &["security", "workspace", "trust", "enabled"])));

            // UNC host access restrictions
            process_changed(observers.restrict_unc_access.handle_change(&optional_path(&config, &["security", "restrictUNCAccess"])));

            // Debug accessibility verbosity
            process_changed(observers.accessibility_verbosity_debug.handle_change(&optional_path(&config, &["accessibility", "verbosity", "debug"])));
        }

        // Experiments
        process_changed(observers.experiments_enabled.handle_change(&optional_path(&config, &["workbench", "enableExperiments"])));

        // Profiles
        process_changed(self.product_service.quality() != Some("stable") && observers.enable_ppe_extensions_gallery.handle_change(&optional_path(&config, &["_extensionsGallery", "enablePPE"])));

        // Enable Feedback
        process_changed(observers.telemetry_feedback_enabled.handle_change(&optional_path(&config, &["telemetry", "feedback", "enabled"])));

        // Extension Unification (only when turning on)
        let extension_unification_enabled = optional_path(&config, &["chat", "extensionUnification", "enabled"]);
        process_changed(
            observers.extension_unification_enabled.handle_change(&extension_unification_enabled) && extension_unification_enabled.strict_equals(&Dynamic::Bool(true)),
        );

        // Agent provider registration and implementation preferences are read at spawn.
        process_changed(observers.agent_host_claude_agent_enabled.handle_change(&optional_path(&config, &["chat", "agentHost", "claudeAgent", "enabled"])));
        process_changed(observers.editor_codex_prefer_agent_host.handle_change(&optional_path(&config, &["chat", "editor", "codex", "preferAgentHost"])));

        // The legacy Copilot CLI migration gate is snapshotted at startup by both the
        // renderer and the shared agent-host process, so a change only applies after a restart.
        process_changed(
            observers.agent_sessions_migrate_legacy_copilot_cli.handle_change(&optional_path(&config, &["chat", "agentSessions", "migrateLegacyCopilotCli"])),
        );

        // Agent Host OTel: settings are forwarded as env vars when the agent host
        // child process is spawned (see `electronAgentHostStarter.ts`). The child
        // is owned by the main process and is not respawned on window reload, so
        // changes only take effect after a full app restart.
        process_changed(observers.agent_host_otel_enabled.handle_change(&optional_path(&config, &["chat", "agentHost", "otel", "enabled"])));
        process_changed(observers.agent_host_otel_exporter_type.handle_change(&optional_path(&config, &["chat", "agentHost", "otel", "exporterType"])));
        process_changed(observers.agent_host_otel_otlp_endpoint.handle_change(&optional_path(&config, &["chat", "agentHost", "otel", "otlpEndpoint"])));
        process_changed(observers.agent_host_otel_capture_content.handle_change(&optional_path(&config, &["chat", "agentHost", "otel", "captureContent"])));
        process_changed(observers.agent_host_otel_outfile.handle_change(&optional_path(&config, &["chat", "agentHost", "otel", "outfile"])));
        process_changed(
            observers
                .agent_host_otel_db_span_exporter_enabled
                .handle_change(&optional_path(&config, &["chat", "agentHost", "otel", "dbSpanExporter", "enabled"])),
        );

        if ask_to_relaunch && changed && self.host_service.has_focus() {
            let name_long = self.product_service.name_long();
            let host_service = self.host_service.clone();
            self.do_confirm(
                if is_native {
                    localize("relaunchSettingMessage", "A setting has changed that requires a restart to take effect.", &[])
                } else {
                    localize("relaunchSettingMessageWeb", "A setting has changed that requires a reload to take effect.", &[])
                },
                if is_native {
                    localize("relaunchSettingDetail", "Press the restart button to restart {0} and enable the setting.", &[name_long])
                } else {
                    localize("relaunchSettingDetailWeb", "Press the reload button to reload {0} and enable the setting.", &[name_long])
                },
                if is_native { localize("restart", "&&Restart", &[]) } else { localize("restartWeb", "&&Reload", &[]) },
                move || {
                    let _ = host_service.restart();
                },
            );
        }
    }

    fn do_confirm(&self, message: String, detail: String, primary_button: String, confirmed_fn: impl FnOnce() + 'static) {
        let mut base = BaseDialogOptions::new(message);
        base.detail = Some(DialogDetail::Text(detail));
        let confirmation = Confirmation { base, primary_button: Some(primary_button), cancel_button: None };
        let _ = self.dialog_service.confirm(confirmation).then(move |result| {
            if result.confirmed {
                confirmed_fn();
            }
        });
    }
}

impl IDisposable for SettingsChangeRelauncher {
    fn dispose(&self) {
        self.base.dispose();
    }
}
