//! Recorte do contrato de serviço que o `ExtensionEnablementService` lê e que ainda não foi portado:
//! `IChatEntitlementService` (`workbench/services/chat/common/chatEntitlementService.ts`).
//!
//! O trait traz só o que o gerenciador de habilitação chama; o resto entra junto com o serviço de que o trait é
//! recorte (o chat está fora do escopo do porte, então o `IChatEntitlementService` é só o que o gerenciador lê para
//! decidir a migração da extensão de chat embutida).

use je_instantiation::service_id;

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

/// `IChatSentiment`, reduzido aos dois estados que o editor de configurações lê: o chat escondido (`hidden`) e o chat
/// desabilitado (`disabled`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChatSentiment {
    pub hidden: bool,
    pub disabled: bool,
}

/// `IChatEntitlementService`, reduzido ao `context` que o `ChatEntitlementService` concreto expõe (um
/// `Lazy<ChatEntitlementContext> | undefined`: o `Lazy` só é avaliado depois de conferir que existe, então aqui é o
/// valor já avaliado ou `None`), mais o `sentiment` e o `onDidChangeSentiment` que o editor de configurações lê. Os dois
/// têm padrão (o chat visível e habilitado, e nenhuma mudança), o de um serviço que não os publica.
pub trait IChatEntitlementService {
    fn context(&self) -> Option<ChatEntitlementContext>;

    /// `sentiment`.
    fn sentiment(&self) -> ChatSentiment {
        ChatSentiment::default()
    }

    /// `onDidChangeSentiment`.
    fn on_did_change_sentiment(&self) -> je_base::event::Event<()> {
        je_base::event::Event::none()
    }
}

service_id!(pub I_CHAT_ENTITLEMENT_SERVICE: dyn IChatEntitlementService = "chatEntitlementService");
