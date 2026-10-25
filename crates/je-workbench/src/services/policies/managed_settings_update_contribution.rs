//! Porte de `src/vs/workbench/services/policies/browser/managedSettingsUpdate.contribution.ts`: publica o banner (e a chave
//! de contexto) do bloqueio por versão mínima do cliente e a ajuda de acessibilidade que o explica.
//!
//! Divergências:
//! - **Registro.** `registerWorkbenchContribution2(..., AfterRestored)` e `AccessibleViewRegistry.register(...)` do corpo do
//!   módulo não têm equivalente no carregamento: quem monta o workbench cria a contribuição com
//!   [`ManagedSettingsUpdateContribution::new`] e registra a ajuda com [`register_accessible_view`].
//! - **Foco anterior.** `getActiveElement()` e `isHTMLElement(previousFocus) && previousFocus.isConnected` são o nó com foco
//!   do [`Ui`] e a checagem de que ele continua vivo e ligado à raiz; por isso a ajuda guarda o `Ui` em que a janela desenha.
//! - **`status(info.message)` e `lastMessage`** (`base/browser/ui/aria/aria.ts`) escrevem numa região viva do DOM, que não
//!   existe na renderização nativa: o anúncio ao leitor de tela passa pelo `IAccessibilityService`, que este construtor não
//!   recebe, então o anúncio e o `lastMessage` que o deduplica não existem aqui. O texto da mudança continua no banner
//!   (`ariaLabel`).
//! - **`Codicon.info`** é o id `info`.

use std::cell::Cell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::observable::base::Observable;
use je_base::observable::autorun;
use je_base::themables::ThemeIcon;
use je_instantiation::ServicesAccessor;
use je_platform::accessibility::accessible_view::{
    AccessibleContentProvider, AccessibleContentProviderArgs, AccessibleViewContentProvider, AccessibleViewProviderId, AccessibleViewType, IAccessibleViewOptions,
};
use je_platform::accessibility::accessible_view_registry::{self, IAccessibleViewImplementation};
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression, IContextKeyService};
use je_platform::observable::platform_observable_utils::observable_config_value;
use je_ui::Ui;

use super::managed_settings_update::{
    I_MANAGED_SETTINGS_UPDATE_SERVICE, IManagedSettingsUpdateService, MANAGED_SETTINGS_UPDATE_VIEW_ID, managed_settings_update_required_context,
};
use crate::common::contextkeys::{focused_view_context, is_sessions_window_context};
use crate::services::banner::banner_service::{BannerIcon, BannerMessage, IBannerItem, IBannerService, ILinkDescriptor};
use crate::services::environment::environment_service::{I_WORKBENCH_ENVIRONMENT_SERVICE, IWorkbenchEnvironmentService};

/// `ChatAIDisabledSettingId` (`platform/chat/common/chatSettings.ts`).
const CHAT_AI_DISABLED_SETTING_ID: &str = "chat.disableAIFeatures";

/// `ManagedSettingsUpdateContribution`.
pub struct ManagedSettingsUpdateContribution {
    base: Rc<Disposable>,
}

impl ManagedSettingsUpdateContribution {
    pub const ID: &'static str = "workbench.contrib.managedSettingsUpdate";

    pub fn new(
        update_service: Rc<dyn IManagedSettingsUpdateService>,
        configuration_service: Rc<dyn IConfigurationService>,
        context_key_service: Rc<dyn IContextKeyService>,
        banner_service: Rc<dyn IBannerService>,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let context = managed_settings_update_required_context().bind_to(&*context_key_service);
        let hidden = observable_config_value::<bool>(CHAT_AI_DISABLED_SETTING_ID, false, configuration_service, |value| match value {
            Dynamic::Bool(hidden) => Some(*hidden),
            _ => None,
        });
        let update_info = update_service.update_info();
        let dismissed = Rc::new(Cell::new(false));

        {
            let (context, banner_service, environment_service) = (context.clone(), banner_service.clone(), environment_service.clone());
            base.register(to_disposable(move || {
                context.reset();
                if !environment_service.is_sessions_window() {
                    banner_service.hide(Self::ID);
                }
            }));
        }

        base.register(autorun(move |reader| {
            let info = update_info.read(reader);
            let visible_info = info.filter(|_| !hidden.read(reader));
            context.set(Dynamic::Bool(visible_info.is_some()));
            if environment_service.is_sessions_window() {
                return;
            }
            let Some(info) = visible_info else {
                dismissed.set(false);
                banner_service.hide(Self::ID);
                return;
            };
            if !dismissed.get() {
                let message = match &info.update_status {
                    Some(update_status) => localize("managedSettingsUpdate.bannerWithStatus", "{0} {1}", &[info.message.as_str(), update_status.as_str()]),
                    None => info.message.clone(),
                };
                let aria_label = [Some(info.title.as_str()), Some(info.message.as_str()), info.detail.as_deref(), info.update_status.as_deref()]
                    .into_iter()
                    .flatten()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                let dismissed = dismissed.clone();
                banner_service.show(IBannerItem {
                    id: Self::ID.to_owned(),
                    icon: Some(BannerIcon::Theme(ThemeIcon { id: "info".to_owned(), color: None })),
                    message: BannerMessage::Text(message),
                    aria_label: Some(aria_label),
                    actions: Some(info.action.iter().map(|action| ILinkDescriptor { label: action.label.clone(), href: action.href.clone(), ..ILinkDescriptor::default() }).collect()),
                    priority: Some(-1),
                    neutral: Some(true),
                    on_close: Some(Rc::new(move || dismissed.set(true))),
                    close_label: None,
                });
            }
        }));

        Rc::new(Self { base })
    }
}

impl IDisposable for ManagedSettingsUpdateContribution {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O `AccessibleViewRegistry.register({ priority: 125, name: 'managedSettingsUpdate', type: Help, ... })` do módulo.
pub struct ManagedSettingsUpdateAccessibleView {
    ui: Ui,
}

impl ManagedSettingsUpdateAccessibleView {
    pub const PRIORITY: i32 = 125;
    pub const NAME: &'static str = "managedSettingsUpdate";

    pub fn new(ui: Ui) -> Self {
        Self { ui }
    }

    /// `getProvider(accessor)`.
    pub fn get_content_provider(&self, accessor: &ServicesAccessor) -> Option<Rc<AccessibleContentProvider>> {
        let info = accessor.get(I_MANAGED_SETTINGS_UPDATE_SERVICE).update_info().get()?;
        let previous_focus = self.ui.focused();
        let is_sessions_window = accessor.get(I_WORKBENCH_ENVIRONMENT_SERVICE).is_sessions_window();

        let provide_content: Rc<dyn Fn() -> String> = Rc::new(move || {
            let help = if is_sessions_window {
                localize(
                    "managedSettingsUpdate.agentsHelp",
                    "The Agents window is blocked by your organization's minimum-version requirement. The overlay explains the required update.",
                    &[],
                )
            } else {
                localize("managedSettingsUpdate.help", "Chat is read-only while this requirement is active.", &[])
            };
            let lines = [
                Some(info.title.clone()),
                Some(info.message.clone()),
                info.detail.clone(),
                info.update_status.clone(),
                Some(help),
                Some(localize(
                    "managedSettingsUpdate.availableActionsHelp",
                    "Use Tab or Shift+Tab to move between available actions, then press Enter or Space to activate one.",
                    &[],
                )),
                info.action.as_ref().map(|action| localize("managedSettingsUpdate.actionHelp", "The available update action is {0}.", &[action.label.as_str()])),
                is_sessions_window.then(|| {
                    localize(
                        "managedSettingsUpdate.editorWindowHelp",
                        "Use Tab or Shift+Tab to reach Open Editor Window, then press Enter or Space. This opens a new editor window.",
                        &[],
                    )
                }),
                (!is_sessions_window).then(|| {
                    localize(
                        "managedSettingsUpdate.bannerHelp",
                        "The window banner is also available with the Focus Banner command. In the banner, use the arrow keys to reach its actions. Closing the banner does not dismiss the explanation in Chat or change your organization's requirement.",
                        &[],
                    )
                }),
            ];
            lines.into_iter().flatten().filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n")
        });

        let ui = self.ui.clone();
        let on_close: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(node) = previous_focus {
                let connected = {
                    let tree = ui.tree();
                    tree.is_alive(node) && tree.is_ancestor(tree.root(), node)
                };
                if connected {
                    ui.focus(node);
                }
            }
        });

        Some(AccessibleContentProvider::new(AccessibleContentProviderArgs::new(
            AccessibleViewProviderId::PanelChat,
            IAccessibleViewOptions::new(AccessibleViewType::Help),
            provide_content,
            on_close,
            "accessibility.verbosity.chat",
        )))
    }
}

impl IAccessibleViewImplementation for ManagedSettingsUpdateAccessibleView {
    fn view_type(&self) -> AccessibleViewType {
        AccessibleViewType::Help
    }

    fn priority(&self) -> i32 {
        Self::PRIORITY
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn get_provider(&self, accessor: &ServicesAccessor) -> Option<AccessibleViewContentProvider> {
        self.get_content_provider(accessor).map(AccessibleViewContentProvider::Content)
    }

    fn when(&self) -> Option<ContextKeyExpression> {
        ContextKeyExpr::and([
            Some(managed_settings_update_required_context().expr()),
            ContextKeyExpr::or([
                Some(focused_view_context().is_equal_to(MANAGED_SETTINGS_UPDATE_VIEW_ID)),
                Some(is_sessions_window_context().expr()),
            ]),
        ])
    }
}

/// `AccessibleViewRegistry.register(...)` da ajuda do bloqueio, para o `Ui` em que a janela desenha.
pub fn register_accessible_view(ui: Ui) -> DisposableRef {
    accessible_view_registry::register(Rc::new(ManagedSettingsUpdateAccessibleView::new(ui)))
}
