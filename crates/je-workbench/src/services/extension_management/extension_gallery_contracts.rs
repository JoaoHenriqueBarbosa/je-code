//! Recortes dos contratos de serviço que o serviço do manifesto da galeria e o da conta da galeria leem e que o porte
//! ainda não tem por inteiro: `ISharedProcessService` (`platform/ipc/electron-browser/services.ts`), o
//! `getConnection` do `IRemoteAgentService` (`workbench/services/remote/common/remoteAgentService.ts`), o `restart` do
//! `IHostService` (`workbench/services/host/browser/host.ts`), o `IAuthenticationService`
//! (`workbench/services/authentication/common/authentication.ts`) e o `IDefaultAccountService`
//! (`platform/defaultAccount/common/defaultAccount.ts`).
//!
//! Por que traits à parte e não mais métodos nos recortes que já existem
//! ([`crate::services::remote::remote_agent_service::IRemoteAgentService`],
//! [`super::service_contracts::IHostService`], [`crate::services::authentication::authentication::IAuthenticationService`]
//! e [`super::service_contracts::IDefaultAccountService`]): cada recorte existente é implementado por dublês de outros
//! testes, que só têm os membros que esses testes usam; acrescentar membro obrigatório quebraria todos. Cada trait daqui
//! traz só o que a galeria chama, com o nome do contrato original e o prefixo do que ele recorta, e se funde ao recorte
//! existente quando o serviço dono do contrato for portado por inteiro.

use std::rc::Rc;

use je_base::default_account::IDefaultAccount;
use je_base::event::Event;
use je_base::parts::ipc::IChannel;
use je_base::promise::Promise;
use je_instantiation::service_id;

use crate::services::authentication::authentication::AuthenticationSessionAccount;

/// `ISharedProcessService`, reduzido a `getChannel`.
pub trait ISharedProcessService {
    /// `getChannel(channelName)`: o canal de nome dado, no processo compartilhado.
    fn get_channel(&self, channel_name: &str) -> Rc<dyn IChannel>;
}

service_id!(pub I_SHARED_PROCESS_SERVICE: dyn ISharedProcessService = "sharedProcessService");

/// `IRemoteAgentConnection`, reduzido a `getChannel`.
pub trait IRemoteAgentConnection {
    /// `getChannel(channelName)`: o canal de nome dado, no servidor remoto.
    fn get_channel(&self, channel_name: &str) -> Rc<dyn IChannel>;
}

/// O `getConnection` do `IRemoteAgentService`: a conexão com o servidor remoto, ou `None` (o `null` do original) sem
/// janela remota.
pub trait IRemoteAgentConnectionProvider {
    fn get_connection(&self) -> Option<Rc<dyn IRemoteAgentConnection>>;
}

service_id!(pub I_REMOTE_AGENT_CONNECTION_PROVIDER: dyn IRemoteAgentConnectionProvider = "remoteAgentConnectionProvider");

/// O `restart` do `IHostService`: reinicia a aplicação.
pub trait IHostRestartService {
    fn restart(&self) -> Promise<()>;
}

service_id!(pub I_HOST_RESTART_SERVICE: dyn IHostRestartService = "hostRestartService");

/// `AuthenticationSession`, sem `expiresAfter` e `authorizationServer`, que a galeria não lê.
#[derive(Clone)]
pub struct AuthenticationSession {
    pub id: String,
    pub access_token: String,
    pub account: AuthenticationSessionAccount,
    pub scopes: Vec<String>,
    /// `idToken?`.
    pub id_token: Option<String>,
}

/// `AuthenticationSessionsChangeEvent`: `added`, `removed` e `changed` são `ReadonlyArray | undefined`.
#[derive(Clone, Default)]
pub struct AuthenticationSessionsChangeEvent {
    pub added: Option<Vec<AuthenticationSession>>,
    pub removed: Option<Vec<AuthenticationSession>>,
    pub changed: Option<Vec<AuthenticationSession>>,
}

/// O `{ providerId, label, event }` de `IAuthenticationService.onDidChangeSessions`.
#[derive(Clone)]
pub struct AuthenticationSessionsChange {
    pub provider_id: String,
    pub label: String,
    pub event: AuthenticationSessionsChangeEvent,
}

/// `IAuthenticationService`, reduzido ao que a conta da galeria chama. `scopeListOrRequest` é a lista de escopos (a forma
/// `IAuthenticationWwwAuthenticateRequest` não é usada), `getSessions` não recebe `options` nem `activateImmediate`, e
/// `IAuthenticationCreateSessionOptions` é só o `{ account }`, dado como `account`.
pub trait IGalleryAuthenticationService {
    fn on_did_change_sessions(&self) -> Event<AuthenticationSessionsChange>;
    fn get_accounts(&self, id: &str) -> Promise<Vec<AuthenticationSessionAccount>>;
    fn get_sessions(&self, id: &str, scopes: &[String]) -> Promise<Vec<AuthenticationSession>>;
    fn create_session(&self, provider_id: &str, scopes: &[String], account: Option<&AuthenticationSessionAccount>) -> Promise<AuthenticationSession>;
}

service_id!(pub I_GALLERY_AUTHENTICATION_SERVICE: dyn IGalleryAuthenticationService = "galleryAuthenticationService");

/// `IDefaultAccountService`, reduzido ao que a conta da galeria chama. `signIn(options?)` é chamado sem opções.
pub trait IGalleryDefaultAccountService {
    fn on_did_change_default_account(&self) -> Event<Option<IDefaultAccount>>;
    fn get_default_account(&self) -> Promise<Option<IDefaultAccount>>;
    fn sign_in(&self) -> Promise<Option<IDefaultAccount>>;
}

service_id!(pub I_GALLERY_DEFAULT_ACCOUNT_SERVICE: dyn IGalleryDefaultAccountService = "galleryDefaultAccountService");
