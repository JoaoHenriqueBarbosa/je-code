//! Host de extensões do je-code (Fase 5, `wip/notes/exthost-plan.md`): o que liga o motor JavaScript (`zjsc`) à
//! máquina onde as extensões rodam.
//!
//! - [`os_host::OsHost`]: o `Host` do zjsc sobre o sistema operacional real (arquivos, rede de loopback, processos
//!   filhos), em Rust seguro e portável entre Linux, macOS e Windows. No alvo web não existe: lá o `Host` vem do
//!   sistema de arquivos virtual do navegador.
//! - [`runtime`]: o `ModuleFs` em disco, o console do processo e a thread com a pilha do motor, para dirigir o
//!   `zjsc::api::embed_node::NodeRunner`.
//! - [`local_extension_host`]: o `IExtensionHost` em processo que o `AbstractExtensionService` do workbench gerencia (pelo
//!   `ExtensionHostManager`), ligado ao `RPCProtocol` por um canal de mensagens em memória; o lado do host é o
//!   [`ext_host_extension_service`] (ativação por evento, registros e carregador CommonJS) sobre o `vscode` montado em Rust.

#[cfg(not(target_family = "wasm"))]
pub mod os_host;
#[cfg(not(target_family = "wasm"))]
pub mod runtime;

pub mod api;
pub mod convert;
pub mod host;
pub mod in_process;
pub mod local_context;
// O host de extensões em processo ligado ao workbench por um protocolo de mensagens em memória (só com o motor e o carregador
// de módulos em disco, que não existem no alvo web). O `ext_host_extension_service` não depende deles (só do `NodeRunner` e do
// `require` publicado em `globalThis.__jeRequire`) e `vscode.extensions` o lê, então fica fora do filtro do alvo web.
pub mod ext_host_extension_service;
pub mod extension_storage_paths;
#[cfg(not(target_family = "wasm"))]
pub mod local_extension_host;
