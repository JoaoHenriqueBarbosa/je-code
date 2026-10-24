//! Porte parcial de `src/vs/workbench/services/editor/common/editorService.ts`: o contrato `IEditorService` reduzido ao que
//! os editores de dados portados e o resolvedor de variáveis de configuração leem dele.
//!
//! Divergências:
//! - **`visibleEditorPanes`** é lido por `findViewStateForEditor`, que o `SideBySideEditorInput` usa para preservar o
//!   estado de visão. Devolve o [`IEditorPane`] reduzido. O restante do serviço (abrir, fechar, procurar editores, os
//!   eventos) entra com o porte do `EditorService` e do `IEditorGroupsService`.
//! - **`activeEditor`** é lido pelo `configurationResolverService` (`EditorResourceAccessor.getOriginalUri(activeEditor,
//!   ...)`). O original o tipa como `EditorInput | undefined`, mas o `EditorResourceAccessor` aceita também o editor sem
//!   tipo, e os testes do resolvedor devolvem um literal `{ resource }`: o retorno é [`EditorInputOrUntyped`].
//! - **`activeTextEditorControl`** (`ICodeEditor | IDiffEditor | undefined`) é o [`ActiveTextEditorControl`]. O `ICodeEditor`
//!   do `je-editor` carrega a API inteira do editor; o resolvedor só lê a seleção, o texto dela e o foco, que são os
//!   métodos de [`IActiveCodeEditor`]. `isCodeEditor` e `isDiffEditor` são as variantes do enum.

use std::rc::Rc;

use je_base::JsString;
use je_base::promise::Promise;
use je_editor::selection::Selection;
use je_instantiation::service_id;

use crate::common::editor::{EditorInputOrUntyped, IEditorPane, UntypedEditorInput};

/// O recorte de `ICodeEditor` que o resolvedor de variáveis lê: a seleção, o texto dentro dela e o foco do widget.
pub trait IActiveCodeEditor {
    /// `getSelection()`.
    fn get_selection(&self) -> Option<Selection>;

    /// `getModel()?.getValueInRange(selection)`: `None` quando o editor não tem modelo.
    fn get_value_in_range(&self, selection: &Selection) -> Option<JsString>;

    /// `hasWidgetFocus()`.
    fn has_widget_focus(&self) -> bool;
}

/// `ICodeEditor | IDiffEditor`: o controle do editor de texto ativo. O de diff guarda os dois lados
/// (`getOriginalEditor()` e `getModifiedEditor()`).
#[derive(Clone)]
pub enum ActiveTextEditorControl {
    Code(Rc<dyn IActiveCodeEditor>),
    Diff { original: Rc<dyn IActiveCodeEditor>, modified: Rc<dyn IActiveCodeEditor> },
}

/// `IEditorService`.
pub trait IEditorService {
    /// The currently visible editor panes in the editor part, one per visible group.
    fn visible_editor_panes(&self) -> Vec<Rc<dyn IEditorPane>>;

    /// The currently active editor or `None` if none.
    fn active_editor(&self) -> Option<EditorInputOrUntyped>;

    /// The currently active text editor control or `None`. There can only be one active editor control and only one
    /// editor control can be active.
    fn active_text_editor_control(&self) -> Option<ActiveTextEditorControl>;

    /// `openEditor(editor: IUntypedEditorInput, group?: PreferredGroup)` sem o grupo preferido (o grupo ativo): opens an
    /// editor in a group and resolves with the editor pane that is showing it, or `None` when it was not opened.
    fn open_editor(&self, editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>>;
}

service_id!(pub I_EDITOR_SERVICE: dyn IEditorService = "editorService");
