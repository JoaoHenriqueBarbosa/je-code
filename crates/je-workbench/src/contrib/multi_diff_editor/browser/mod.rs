//! Porte parcial de `src/vs/workbench/contrib/multiDiffEditor/browser`: o ícone do rótulo, o `MultiDiffEditorInput` (com o
//! serializador e a contribuição que registra o editor no resolvedor), o `MultiDiffEditorItem`, o serviço de resolvedores de fonte do
//! editor de diff de vários arquivos e o resolvedor do SCM. O `multi_diff_editor_view_model` é de
//! `editor/browser/widget/multiDiffEditor` (ver o topo dele).

pub mod icons_contribution;
pub mod multi_diff_editor_input;
pub mod multi_diff_editor_view_model;
pub mod multi_diff_source_resolver_service;
pub mod scm_multi_diff_source_resolver;
