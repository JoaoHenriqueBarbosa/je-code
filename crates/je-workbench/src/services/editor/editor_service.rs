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
//! - **`findEditors`, `save` e `saveAll`** são lidos pelo `MainThreadWorkspace`. `findEditors` entra só na sobrecarga com
//!   `URI` (a de `IResourceEditorInputIdentifier` entra junto do primeiro consumidor). As interfaces
//!   `ISaveEditorsOptions`, `ISaveAllEditorsOptions` (com `IBaseSaveRevertAllEditorOptions`), `ISaveEditorsResult` e
//!   `IFindEditorOptions` são [`SaveEditorsOptions`], [`SaveAllEditorsOptions`], [`SaveEditorsResult`] e
//!   [`FindEditorOptions`]; `includeUntitled?: { includeScratchpad } | boolean` é [`IncludeUntitled`].

use std::rc::Rc;

use je_base::JsString;
use je_base::uri::URI;
use je_base::promise::Promise;
use je_editor::selection::Selection;
use je_instantiation::service_id;

use crate::common::editor::{EditorIdentifier, EditorInputOrUntyped, EditorsOrder, IEditorPane, SaveOptions, SideBySideEditor, UntypedEditorInput};

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

/// `ISaveEditorsOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveEditorsOptions {
    /// O `ISaveOptions` de que herda.
    pub base: SaveOptions,
    /// Save the editors to a different location (`saveAs`).
    pub save_as: Option<bool>,
}

/// O `{ includeScratchpad: boolean } | boolean` de `includeUntitled`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncludeUntitled {
    Bool(bool),
    /// `{ includeScratchpad }`.
    Options { include_scratchpad: bool },
}

/// `ISaveAllEditorsOptions` (`ISaveEditorsOptions` mais `IBaseSaveRevertAllEditorOptions`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveAllEditorsOptions {
    pub save: SaveEditorsOptions,
    /// Whether to include untitled editors as well.
    pub include_untitled: Option<IncludeUntitled>,
    /// Whether to exclude sticky editors.
    pub exclude_sticky: Option<bool>,
}

/// `ISaveEditorsResult`.
#[derive(Clone)]
pub struct SaveEditorsResult {
    /// Whether the save operation was successful.
    pub success: bool,
    /// Resulting editors after the save operation.
    pub editors: Vec<EditorInputOrUntyped>,
}

/// `IFindEditorOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindEditorOptions {
    /// Whether to consider any or both side by side editor as matching (`Primary`, `Secondary` ou `Any`).
    pub support_side_by_side: Option<SideBySideEditor>,
    /// The order in which to consider editors for finding.
    pub order: Option<EditorsOrder>,
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

    /// The currently active text editor language id or `None` (o `activeTextEditorLanguageId`, que o `EditorService`
    /// lê do modelo do controle de texto ativo).
    fn active_text_editor_language_id(&self) -> Option<String>;

    /// `openEditor(editor: IUntypedEditorInput, group?: PreferredGroup)` sem o grupo preferido (o grupo ativo): opens an
    /// editor in a group and resolves with the editor pane that is showing it, or `None` when it was not opened.
    fn open_editor(&self, editor: UntypedEditorInput) -> Promise<Option<Rc<dyn IEditorPane>>>;

    /// `findEditors(resource, options)`: finds all editors that match the resource.
    fn find_editors(&self, resource: &URI, options: Option<FindEditorOptions>) -> Vec<EditorIdentifier>;

    /// `save(editors, options)`: saves the editors and resolves with the result of the operation.
    fn save(&self, editors: Vec<EditorIdentifier>, options: Option<SaveEditorsOptions>) -> Promise<SaveEditorsResult>;

    /// `saveAll(options)`: saves all editors that are dirty.
    fn save_all(&self, options: Option<SaveAllEditorsOptions>) -> Promise<SaveEditorsResult>;
}

service_id!(pub I_EDITOR_SERVICE: dyn IEditorService = "editorService");
