//! Porte de `src/vs/workbench/services/editor`: o contrato `IEditorService` (reduzido) e o `IEditorGroupsService`
//! (reduzido); `common/*.ts` vira módulos diretos desta pasta.

pub mod custom_editor_label_service;
pub mod editor_groups_service;
pub mod editor_service;

// Resolvedor de editores (`editorResolverService.ts` comum e do browser), `editorGroupColumn.ts`, `editorGroupFinder.ts` e os
// contratos reduzidos de grupos de editores e de extensões que eles leem.
pub mod browser_editor_resolver_service;
pub mod editor_group_column;
pub mod editor_group_contracts;
pub mod editor_group_finder;
pub mod editor_resolver_service;
pub mod extension_service_contract;
