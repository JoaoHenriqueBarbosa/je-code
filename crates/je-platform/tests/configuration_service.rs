//! Porte de `src/vs/platform/configuration/test/common/configurationService.test.ts` (suíte `ConfigurationService.test.ts`), caso a caso e na mesma ordem.
//!
//! Pendente: nenhum dos 17 casos roda ainda. A suíte exercita o `ConfigurationService`
//! (`configurationService.ts`), que depende do `IFileService` (`vs/platform/files`: `FileService`,
//! `InMemoryFileSystemProvider`), do `UserSettings` (lê e observa o `settings.json` por ele), do
//! `FilePolicyService`/`NullPolicyService` (`vs/platform/policy`), de `jsonEdit`/`jsonFormatter`
//! (`vs/base/common`, usados por `updateValue` para reescrever o arquivo) e do `runWithFakedTimers` sobre o
//! serviço de arquivos; nada disso foi portado. Os casos, na ordem do original:
//!
//! - simple
//! - config gets flattened
//! - error case does not explode
//! - missing file does not explode
//! - trigger configuration change event when file does not exist
//! - trigger configuration change event when file exists
//! - reloadConfiguration
//! - model defaults
//! - lookup
//! - lookup with null
//! - update configuration
//! - update configuration when exist
//! - update configuration to default value should remove
//! - update configuration should remove when undefined is passed
//! - update unknown configuration
//! - update configuration in non user target throws error
//! - update configuration throws error for policy setting
