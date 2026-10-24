//! Recorte do contrato de serviço que o serviço do manifesto da galeria e o da conta da galeria leem e que o porte
//! ainda não tem por inteiro: `ISharedProcessService` (`platform/ipc/electron-browser/services.ts`). O trait traz só o
//! que a galeria chama, com o nome do contrato original, e se funde ao serviço dono do contrato quando ele for portado
//! por inteiro.

use std::rc::Rc;

use je_base::parts::ipc::IChannel;
use je_instantiation::service_id;

/// `ISharedProcessService`, reduzido a `getChannel`.
pub trait ISharedProcessService {
    /// `getChannel(channelName)`: o canal de nome dado, no processo compartilhado.
    fn get_channel(&self, channel_name: &str) -> Rc<dyn IChannel>;
}

service_id!(pub I_SHARED_PROCESS_SERVICE: dyn ISharedProcessService = "sharedProcessService");
