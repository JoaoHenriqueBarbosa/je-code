//! Porte parcial de `src/vs/workbench/services/agentHost/browser/codexAccountService.ts`: só o contrato
//! [`ICodexAccountService`].
//!
//! O serviço pertence à camada de IA (agent host, chat e Copilot), que o produto recortou. Entra só o contrato, para a
//! adição do serviço não ficar impossível: [`ICodexAccountInfo`] carrega o `status` e o `email`, e não há implementação
//! do serviço nem consumidor na produção (o menu "Sign in to ChatGPT" da barra global de atividades, a abertura da URL de
//! login, a imagem de perfil e as requisições ao agent host ficam com a camada de IA).

use je_base::event::Event;

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

/// `ICodexAccountViewInfo`, no recorte `status` e `email`.
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
