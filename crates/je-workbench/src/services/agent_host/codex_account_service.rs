//! Porte parcial de `src/vs/workbench/services/agentHost/browser/codexAccountService.ts`: o contrato [`ICodexAccountService`]
//! e as duas funções que o menu de contas do `AccountsActivityActionViewItem` chama, [`should_show_codex_account`] e
//! [`create_codex_account_menu_actions`].
//!
//! O serviço pertence à camada de IA (agent host, chat e Copilot), que o produto recortou. Entra só o contrato que o
//! código da barra de atividades exige, para a adição do serviço não ficar impossível: [`ICodexAccountInfo`] carrega o
//! `status` e o `email` (os campos que o menu lê), e não há implementação do serviço (a abertura da URL de login, a
//! imagem de perfil e as requisições ao agent host ficam com a camada de IA).

use std::rc::Rc;

use je_base::actions::{Action, ActionRef, SubmenuAction, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_platform::configuration::service::IConfigurationService;

/// `ChatAIDisabledSettingId`.
pub const CHAT_AI_DISABLED_SETTING_ID: &str = "chat.disableAIFeatures";

/// `AgentHostCodexAgentEnabledSettingId`.
pub const AGENT_HOST_CODEX_AGENT_ENABLED_SETTING_ID: &str = "chat.agentHost.codexAgent.enabled";

/// `CodexPreferAgentHostEditorSettingId`.
pub const CODEX_PREFER_AGENT_HOST_EDITOR_SETTING_ID: &str = "chat.editor.codex.preferAgentHost";

/// `CODEX_AGENT_PROVIDER_ID`.
pub const CODEX_AGENT_PROVIDER_ID: &str = "codex";

/// O `status` de `ICodexAccountInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodexAccountStatus {
    Unknown,
    Downloading,
    SignedIn,
    SignedOut,
    Unavailable,
    Error,
}

/// `ICodexAccountViewInfo`, no recorte que o menu lê (`status` e `email`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICodexAccountInfo {
    pub status: CodexAccountStatus,
    pub email: Option<String>,
}

/// `ICodexAccountService`.
pub trait ICodexAccountService {
    /// A conta atual.
    fn account(&self) -> ICodexAccountInfo;

    /// `onDidChangeAccount`.
    fn on_did_change_account(&self) -> Event<ICodexAccountInfo>;

    /// `signIn()`.
    fn sign_in(&self);

    /// `signOut()`.
    fn sign_out(&self);
}

je_instantiation::service_id!(pub I_CODEX_ACCOUNT_SERVICE: dyn ICodexAccountService = "codexAccountService");

/// `shouldShowCodexAccount(configurationService, isSessionsWindow)`.
pub fn should_show_codex_account(configuration_service: &dyn IConfigurationService, is_sessions_window: bool) -> bool {
    let flag = |section: &str| configuration_service.get_value(Some(section), None);
    !matches!(flag(CHAT_AI_DISABLED_SETTING_ID), Dynamic::Bool(true))
        && matches!(flag(AGENT_HOST_CODEX_AGENT_ENABLED_SETTING_ID), Dynamic::Bool(true))
        && (is_sessions_window || matches!(flag(CODEX_PREFER_AGENT_HOST_EDITOR_SETTING_ID), Dynamic::Bool(true)))
}

/// `createCodexAccountMenuActions(service, visible = true)`.
pub fn create_codex_account_menu_actions(service: &Rc<dyn ICodexAccountService>, visible: bool) -> Vec<ActionRef> {
    if !visible {
        return Vec::new();
    }
    let account = service.account();
    match account.status {
        CodexAccountStatus::SignedIn => {
            let signing_out = service.clone();
            let sign_out = to_action(ToActionProps {
                id: "codex.signOutOfChatGPT".to_owned(),
                label: localize("signOutOfChatGPT", "Sign Out", &[]),
                tooltip: None,
                enabled: None,
                checked: None,
                class: None,
                alt: None,
                run: Rc::new(move |_| {
                    signing_out.sign_out();
                    MaybePromise::Value(Dynamic::Undefined)
                }),
            });
            let account_label = match account.email.as_deref() {
                Some(email) if !email.is_empty() => localize("chatGPTAccountWithProvider", "{0} (ChatGPT)", &[email]),
                _ => localize("chatGPTAccount", "ChatGPT", &[]),
            };
            let submenu: ActionRef = SubmenuAction::new("codex.chatgptAccount", &account_label, vec![sign_out], None);
            vec![submenu]
        }
        CodexAccountStatus::Downloading => {
            let downloading: ActionRef = Action::new("codex.downloadingAgent", &localize("downloadingCodexAgent", "Downloading Codex Agent\u{2026}", &[]), "", false, None);
            vec![downloading]
        }
        // Unknown is passive until explicit Codex use resolves the account.
        CodexAccountStatus::SignedOut | CodexAccountStatus::Error => {
            let signing_in = service.clone();
            let sign_in: ActionRef = Action::new(
                "codex.signInToChatGPT",
                &localize("signInToChatGPT", "Sign in to ChatGPT", &[]),
                "",
                true,
                Some(Rc::new(move |_| {
                    signing_in.sign_in();
                    MaybePromise::Value(Dynamic::Undefined)
                })),
            );
            vec![sign_in]
        }
        CodexAccountStatus::Unknown | CodexAccountStatus::Unavailable => Vec::new(),
    }
}
