//! Porte de `src/vs/base/common/worker`: o RPC de worker do VS Code (`webWorker.ts` e `webWorkerBootstrap.ts`).
//!
//! O protocolo de mensagens (`Request`, `Reply`, `SubscribeEvent`, `Event`, `UnsubscribeEvent`), o cliente do
//! lado da thread principal ([`WebWorkerClient`]) e o servidor do lado do worker ([`WebWorkerServer`]) são os do
//! original. O transporte é um [`IWebWorker`]: no nativo, um worker é uma thread do SO ([`ThreadWebWorker`]),
//! que troca as mensagens como bytes (a codificação do IPC, ver [`Message::to_bytes`]) por canais `mpsc`; no
//! alvo web, o mesmo trait é implementado sobre os Web Workers do navegador (`Worker.postMessage`), que não
//! entram no `je-base` por dependerem de `web-sys`.
//!
//! Decisões de tradução:
//! - O `Proxy` de `createProxyToRemoteChannel` (que cria funções a partir do nome, `$método`, `onEvento` e
//!   `onDynamicEvento`) é um [`RemoteChannel`] com `call`, `event` e `dynamic_event`. Os canais locais e o
//!   `requestHandler` são [`IWorkerChannelHandler`] (métodos e eventos por nome, no lugar de `handler[name]`).
//! - Os valores trocados são [`Marshalled`]. `ArrayBuffer` e a lista `transfer` não existem: uma mensagem
//!   atravessa a thread copiada em bytes.
//! - O erro de uma chamada viaja como o `SerializedError` do original (`$isError`, `name`, `message`, `stack`).

pub mod thread_worker;
pub mod web_worker;

pub use web_worker::*;
