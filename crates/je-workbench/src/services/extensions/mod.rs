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
// `abstractExtensionService.ts` e o que ele lê: `extensionHostManagers.ts`, `extensionHostProtocol.ts` (só o delta),
// `extensionHostProxy.ts` (só o resultado de `resolveAuthority`), `extensionDevOptions.ts`, `extensionsProposedApi.ts` (com
// `extensionsApiProposals.ts`) e `implicitActivationEvents.ts`.
pub mod abstract_extension_service;
pub mod extension_dev_options;
pub mod extension_host_managers;
pub mod extension_host_protocol;
pub mod extension_host_proxy;
pub mod extensions_api_proposals;
pub mod extensions_proposed_api;
pub mod implicit_activation_events;
// `extHostCustomers.ts`, `extensionHostManager.ts` e `lazyCreateExtensionHostManager.ts`: o gerenciador do host de extensões (cria o
// `RPCProtocol` sobre o protocolo do host e instancia os clientes `MainThread*` do registro).
pub mod ext_host_customers;
pub mod extension_host_manager;
pub mod lazy_create_extension_host_manager;
// `extensionStorageMigration.ts`: a migração do armazenamento de uma extensão para outro identificador.
pub mod extension_storage_migration;
