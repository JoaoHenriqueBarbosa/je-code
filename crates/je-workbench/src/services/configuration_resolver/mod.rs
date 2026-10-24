//! Porte de `src/vs/workbench/services/configurationResolver`: o resolvedor das variáveis `${...}` dos arquivos de
//! configuração (`launch.json`, `tasks.json`, ajustes). `common` tem o contrato, a expressão analisada e o resolvedor
//! abstrato; `browser` tem o serviço que lê o editor ativo, as pastas do workspace e pede entrada ao usuário.
//!
//! Fora do porte: `electron-browser/configurationResolverService.ts`, que só acrescenta `appRoot`, `execPath` e o ambiente
//! do shell (`INativeWorkbenchEnvironmentService` e `IShellEnvironmentService`, ainda sem porte).

pub mod browser;
pub mod common;
