//! Porte de `src/vs/workbench/services` (e do `platform/userDataSync/common`) para a sincronização de dados do usuário
//! que depende dos serviços de gerência de extensões: o `ExtensionsSynchroniser` e o
//! `IIgnoredExtensionsManagementService`. Moram aqui porque `extensions_merge` e `SyncExtension` estão no
//! `je-extensions` e os serviços de gerência de extensões no `je-workbench`, e os dois dependem do `je-platform`.

pub mod extensions_sync;
pub mod ignored_extensions;
