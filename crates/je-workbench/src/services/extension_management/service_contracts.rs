//! Recortes dos contratos de serviço que o `ExtensionEnablementService` lê e que ainda não foram portados:
//! `IHostService` (`workbench/services/host/browser/host.ts`) e `IChatEntitlementService`
//! (`workbench/services/chat/common/chatEntitlementService.ts`).
//!
//! Cada trait traz só o que o gerenciador de habilitação chama; o resto entra junto com o serviço de que o trait é
//! recorte (o chat está fora do escopo do porte, então o `IChatEntitlementService` é só o que o gerenciador lê para
//! decidir a migração da extensão de chat embutida).

use je_base::promise::Promise;
use je_instantiation::service_id;

/// O `{ disableExtensions?: boolean }` de `IReloadOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReloadOptions {
    pub disable_extensions: Option<bool>,
}

/// `IHostService`, reduzido a `reload`.
pub trait IHostService {
    /// `reload(options?)`.
    fn reload(&self, options: Option<ReloadOptions>) -> Promise<()>;
}

service_id!(pub I_HOST_SERVICE: dyn IHostService = "hostService");

/// `IChatEntitlementContextState`, reduzido a `completed` (a configuração do chat foi concluída).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChatEntitlementContextState {
    pub completed: bool,
}

/// `ChatEntitlementContext`, reduzido ao `state`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChatEntitlementContext {
    pub state: ChatEntitlementContextState,
}

/// `IChatEntitlementService`, reduzido ao `context` que o `ChatEntitlementService` concreto expõe (um
/// `Lazy<ChatEntitlementContext> | undefined`: o `Lazy` só é avaliado depois de conferir que existe, então aqui é o
/// valor já avaliado ou `None`).
pub trait IChatEntitlementService {
    fn context(&self) -> Option<ChatEntitlementContext>;
}

service_id!(pub I_CHAT_ENTITLEMENT_SERVICE: dyn IChatEntitlementService = "chatEntitlementService");
