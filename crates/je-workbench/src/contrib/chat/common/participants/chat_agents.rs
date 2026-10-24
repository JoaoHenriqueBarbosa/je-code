//! Porte parcial de `src/vs/workbench/contrib/chat/common/participants/chatAgents.ts`: o contrato do serviço de agentes
//! (`IChatAgentService`) restrito ao que os consumidores de fora do chat leem, que é a lista de agentes com os locais em que
//! atuam e o aviso de que a lista mudou. O registro dos agentes e a invocação deles (`ChatAgentService`) estão fora do escopo do
//! porte: o chat em si foi recortado.
//!
//! Divergências:
//! - `IChatAgentData` traz só `id`, `name`, `isDefault`, `locations` e `modes`: o resto (`extensionId`, `extensionVersion`,
//!   `extensionDisplayName`, `extensionPublisherId`, `metadata`, `slashCommands`, `disambiguation`, `when`, `capabilities`...) descreve
//!   a contribuição do agente para a interface de chat e nenhum consumidor portado o lê.
//! - `onDidChangeAgents: Event<IChatAgent | undefined>` é `Event<()>`: o agente que mudou (o `IChatAgent` com a implementação) não
//!   existe no porte, e quem escuta só reage ao aviso.

use je_base::event::Event;
use je_instantiation::service_id;

use crate::contrib::chat::common::constants::{ChatAgentLocation, ChatModeKind};

/// `IChatAgentData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IChatAgentData {
    pub id: String,
    pub name: String,
    /// The agent invoked when no agent is specified.
    pub is_default: bool,
    pub locations: Vec<ChatAgentLocation>,
    /// This is only relevant for isDefault agents. Others should have all modes available.
    pub modes: Vec<ChatModeKind>,
}

/// `IChatAgentService`.
pub trait IChatAgentService {
    fn on_did_change_agents(&self) -> Event<()>;
    fn get_agents(&self) -> Vec<IChatAgentData>;
}

service_id!(pub I_CHAT_AGENT_SERVICE: dyn IChatAgentService = "chatAgentService");
