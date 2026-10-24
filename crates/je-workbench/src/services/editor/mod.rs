//! Porte de `src/vs/workbench/services/editor`: o contrato `IEditorService` (reduzido), o `IEditorGroupsService`
//! (reduzido) e o resolvedor de editores; `common/*.ts` vira módulos diretos desta pasta.

pub mod custom_editor_label_service;
pub mod editor_groups_service;
pub mod editor_service;

// O `EditorService` (`editorService.ts` do browser) e o container de grupos que ele e o observador de editores recebem.
pub mod editor_groups_container;
pub mod editor_pane_service;
pub mod editor_service_impl;

// Resolvedor de editores (`editorResolverService.ts` comum e do browser), `editorGroupColumn.ts` e `editorGroupFinder.ts`.
pub mod browser_editor_resolver_service;
pub mod editor_group_column;
pub mod editor_group_finder;
pub mod editor_resolver_service;
