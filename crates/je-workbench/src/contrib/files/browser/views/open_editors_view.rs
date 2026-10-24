//! Porte parcial de `src/vs/workbench/contrib/files/browser/views/openEditorsView.ts`: a função `findFirstDirtyEditor`.
//!
//! A vista (`OpenEditorsView`) e a lista visual dela entram com os consumidores que as leem.

use std::rc::Rc;

use crate::contrib::files::common_files::OpenEditor;
use crate::services::editor::editor_groups_service::IEditorGroup;

/// `findFirstDirtyEditor`: finds the first editor with unsaved changes in index order, i.e. the first unsaved editor of
/// the first group that has one.
pub fn find_first_dirty_editor(groups: &[Rc<dyn IEditorGroup>]) -> Option<OpenEditor> {
    for group in groups {
        for editor in group.editors() {
            if editor.is_dirty() {
                return Some(OpenEditor::new(editor, group.clone()));
            }
        }
    }

    None
}
