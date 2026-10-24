//! Porte de `src/vs/workbench/services/extensions`: um módulo por arquivo de `common`, na árvore do original.

pub mod browser;
pub mod extension_manifest_properties_service;
pub mod extensions;
pub mod extensions_registry;
pub mod workspace_contains;
pub mod extension_activation;
pub mod extension_description_registry;
// `lazyPromise.ts` e `rpcProtocol.ts` (o RPC real do host de extensões).
pub mod lazy_promise;
pub mod rpc_protocol;
// `extensionHostKind.ts`, `extensionRunningLocation.ts` e `extensionRunningLocationTracker.ts`.
pub mod extension_host_kind;
pub mod extension_running_location;
pub mod extension_running_location_tracker;
