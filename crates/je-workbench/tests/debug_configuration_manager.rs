//! Porte de `src/vs/workbench/contrib/debug/test/browser/debugConfigurationManager.test.ts` (suíte `debugConfigurationManager`), caso a caso e na mesma ordem.
//!
//! NENHUM caso foi portado (sem `#[test]`): os quatro (`resolves configuration based on type`, `resolves configuration from
//! second provider if type changes`, `uses remote target OS when computing visible configurations` e `ignores null entries
//! in launch configurations`) instanciam o `ConfigurationManager` (`browser/debugConfigurationManager.ts`, 802 linhas:
//! `launch.json`, `Launch`/`WorkspaceLaunch`/`UserLaunch`, seleção de configuração no armazenamento, provedores dinâmicos,
//! `IRemoteAgentService`, `IPreferencesService`, `IWorkspaceContextService`, quick pick), que NÃO foi portado neste lote.
//! O contrato que ele fornece ao serviço (`registerDebugConfigurationProvider`, `getLaunch`) existe só como interface em
//! `debug.rs`; um caso sobre o gerenciador real não seria fiel sem a implementação.
