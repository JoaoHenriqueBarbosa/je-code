//! Porte parcial de `src/vs/workbench/services/editor/common/editorService.ts`: o contrato `IEditorService` reduzido ao que
//! os editores de dados portados leem dele.
//!
//! Divergências:
//! - **`visibleEditorPanes`** é o único membro lido (por `findViewStateForEditor`, que o `SideBySideEditorInput` usa para
//!   preservar o estado de visão). Devolve o [`IEditorPane`] reduzido. O restante do serviço (abrir, fechar, procurar
//!   editores, os eventos) entra com o porte do `EditorService` e do `IEditorGroupsService`.

use std::rc::Rc;

use je_instantiation::service_id;

use crate::common::editor::IEditorPane;

/// `IEditorService`.
pub trait IEditorService {
    /// The currently visible editor panes in the editor part, one per visible group.
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>>;
}

service_id!(pub I_EDITOR_SERVICE: dyn IEditorService = "editorService");
