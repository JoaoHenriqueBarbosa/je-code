//! Porte de `src/vs/platform/terminal/common/localPtyChannel.ts`: o canal do `ILocalPtyService`, um serviço exposto como
//! canal pelo `ProxyChannel.fromService`.
//!
//! Representação: o `satisfies (keyof ILocalPtyService)[]` da lista de eventos sem buffer é a lista de nomes (o
//! compilador do original confere os nomes; aqui o `ILocalPtyService` é um [`IProxyService`] por nome, e um nome
//! errado seria um evento ausente, que `fromService` ignora).

use std::rc::Rc;

use je_base::lifecycle::DisposableStore;
use je_base::parts::ipc::IServerChannel;
use je_base::parts::ipc::ProxyChannel::{ICreateServiceChannelOptions, from_service};

use super::pty_service::ILocalPtyService;

pub fn create_local_pty_channel(service: Rc<dyn ILocalPtyService>, disposables: &Rc<DisposableStore>) -> Rc<dyn IServerChannel> {
    from_service(
        service,
        disposables,
        Some(ICreateServiceChannelOptions {
            // Process events use the direct pty host connection; keep management-event buffering unchanged on this channel.
            unbuffered_events: ["onProcessData", "onProcessReady", "onProcessExit", "onProcessReplay", "onDidChangeProperty", "onProcessOrphanQuestion", "onDidRequestDetach"]
                .map(str::to_owned)
                .to_vec(),
            ..Default::default()
        }),
    )
}
