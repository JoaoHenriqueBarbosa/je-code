//! Host de extensões do je-code (Fase 5, `wip/notes/exthost-plan.md`): o que liga o motor JavaScript (`zjsc`) à
//! máquina onde as extensões rodam.
//!
//! - [`os_host::OsHost`]: o `Host` do zjsc sobre o sistema operacional real (arquivos, rede de loopback, processos
//!   filhos), em Rust seguro e portável entre Linux, macOS e Windows. No alvo web não existe: lá o `Host` vem do
//!   sistema de arquivos virtual do navegador.
//! - [`runtime`]: o `ModuleFs` em disco, o console do processo e a thread com a pilha do motor, para dirigir o
//!   `zjsc::api::embed_node::NodeRunner`.

#[cfg(not(target_family = "wasm"))]
pub mod os_host;
#[cfg(not(target_family = "wasm"))]
pub mod runtime;
