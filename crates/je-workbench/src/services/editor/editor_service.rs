//! Porte de `src/vs/workbench/services/editor/common/editorService.ts`: o contrato `IEditorService`.
//!
//! O trait começou reduzido ao que os editores de dados portados e o resolvedor de variáveis liam dele; com o porte do
//! `EditorService` ([`editor_service_impl`](super::editor_service_impl)) ele ganhou o restante do contrato. Os membros que o
//! porte já lia continuam obrigatórios; todos os acrescentados têm implementação padrão (os eventos nunca disparam e o resto
//! lança `Method not implemented.`, o erro do dublê do original), para não quebrar os dublês existentes.
//!
//! Divergências:
//! - **Propriedades** (`activeEditor`, `activeEditorPane`, `editors`, `count`, `visibleEditors`...) viram métodos.
//! - **`visibleEditorPanes`** é lido por `findViewStateForEditor`, que o `SideBySideEditorInput` usa para preservar o
//!   estado de visão. Devolve o [`IEditorPane`] reduzido (o `IVisibleEditorPane` do original é o mesmo painel, com o grupo
//!   garantido).
//! - **`activeEditor`** é lido pelo `configurationResolverService` (`EditorResourceAccessor.getOriginalUri(activeEditor,
//!   ...)`). O original o tipa como `EditorInput | undefined`, mas o `EditorResourceAccessor` aceita também o editor sem
//!   tipo, e os testes do resolvedor devolvem um literal `{ resource }`: o retorno é [`EditorInputOrUntyped`].
//! - **`activeTextEditorControl`** (`ICodeEditor | IDiffEditor | undefined`) é o [`ActiveTextEditorControl`]. O `ICodeEditor`
//!   do `je-editor` carrega a API inteira do editor; o resolvedor só lê a seleção, o texto dela e o foco, que são os
//!   métodos de [`IActiveCodeEditor`] (mais o `getModel()?.getLanguageId()` do `activeTextEditorLanguageId`). `isCodeEditor`
//!   e `isDiffEditor` são as variantes do enum. O `IEditorControl` que o painel devolve em `getControl()` é o
//!   [`EditorControl`], e a checagem `editorPane instanceof SideBySideEditorPane` é [`IEditorPane::side_by_side_panes`].
//! - **Sobrecargas.** O original sobrecarrega `openEditor`, `openEditors`, `replaceEditors`, `findEditors`, `save`, `revert`
//!   e `closeEditor`; o porte tem um método por forma: `open_editor` (sem tipo, grupo ativo), `open_editor_in` (sem tipo,
//!   com grupo), `open_editor_typed` (`EditorInput` com opções e grupo), `open_editors`/`open_editors_in`,
//!   `find_editors`/`find_editors_by_identifier`/`find_editors_in_group`/`find_editor_in_group`. As formas que aceitam
//!   `IEditorIdentifier | IEditorIdentifier[]` recebem sempre a lista.
//! - **`getEditors(order, options)`** com `{ excludeSticky }` é `get_editors_with_options`; o `get_editors(order)` existente é
//!   a forma sem opções.
//! - **`IUntypedEditorReplacement`** e `IEditorReplacement` formam o enum [`EditorReplacementInput`] (o parâmetro
//!   `Array<IEditorReplacement | IUntypedEditorReplacement>` do `replaceEditors`).
//! - **`findEditors`, `save` e `saveAll`** são lidos pelo `MainThreadWorkspace`. As interfaces `ISaveEditorsOptions`,
//!   `ISaveAllEditorsOptions` (com `IBaseSaveRevertAllEditorOptions`), `ISaveEditorsResult`, `IRevertAllEditorsOptions` e
//!   `IFindEditorOptions` são [`SaveEditorsOptions`], [`SaveAllEditorsOptions`], [`SaveEditorsResult`],
//!   [`RevertAllEditorsOptions`] e [`FindEditorOptions`] (este último é o de [`common::editor`](crate::common::editor));
//!   `includeUntitled?: { includeScratchpad } | boolean` é [`IncludeUntitled`].
//! - **`createScoped(editorGroupsContainer, disposables)`** recebe o [`EditorGroupsContainer`] (o container do original não é
//!   um trait aqui).

use std::rc::Rc;

use je_base::JsString;
use je_base::event::Event;
use je_base::lifecycle::DisposableStore;
use je_base::uri::URI;
use je_base::promise::Promise;
use je_editor::code_editor_widget::CodeEditorWidget;
use je_editor::selection::Selection;
use je_instantiation::service_id;

use crate::common::editor::editor_group_model::{GetEditorsOptions, GroupModelChangeEvent};
use crate::common::editor::{
    EditorIdentifier, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, EditorCloseEvent, EditorWillOpenEvent, EditorsOrder, GroupIdentifier, IEditorPane,
    RevertOptions, SaveOptions, UntypedEditorInput,
};
use crate::services::editor::editor_group_finder::FindGroupEditor;
use crate::services::editor::editor_groups_container::EditorGroupsContainer;
use crate::services::editor::editor_groups_service::{CloseEditorOptions, EditorReplacement, GroupRef, PreferredGroup, not_implemented};

pub use crate::common::editor::FindEditorOptions;

/// O recorte de `ICodeEditor` que o resolvedor de variáveis lê: a seleção, o texto dentro dela e o foco do widget.
pub trait IActiveCodeEditor {
    /// `getSelection()`.
    fn get_selection(&self) -> Option<Selection>;

    /// `getModel()?.getValueInRange(selection)`: `None` quando o editor não tem modelo.
    fn get_value_in_range(&self, selection: &Selection) -> Option<JsString>;

    /// `hasWidgetFocus()`.
    fn has_widget_focus(&self) -> bool;

    /// `getModel()?.getLanguageId()`: `None` quando o editor não tem modelo. Tem implementação padrão (sem modelo) para
    /// não quebrar quem implementa só o recorte acima.
    fn get_language_id(&self) -> Option<String> {
        None
    }

    /// O `ICodeEditor` completo por trás do recorte (o `BulkEditService` o passa ao `BulkTextEdits`). Tem implementação
    /// padrão (sem editor completo) para não quebrar quem implementa só o recorte acima.
    fn code_editor_widget(&self) -> Option<Rc<CodeEditorWidget>> {
        None
    }
}

/// `ICodeEditor | IDiffEditor`: o controle do editor de texto ativo. O de diff guarda os dois lados
/// (`getOriginalEditor()` e `getModifiedEditor()`).
#[derive(Clone)]
pub enum ActiveTextEditorControl {
    Code(Rc<dyn IActiveCodeEditor>),
    Diff { original: Rc<dyn IActiveCodeEditor>, modified: Rc<dyn IActiveCodeEditor> },
}

/// `IEditorControl` (`getControl()` de um painel de editor): um controle de texto (`ICodeEditor | IDiffEditor`), um editor
/// composto (`ICompositeCodeEditor`, que tem o `activeCodeEditor`) ou qualquer outro controle.
#[derive(Clone)]
pub enum EditorControl {
    /// `isCodeEditor(control) || isDiffEditor(control)`.
    Text(ActiveTextEditorControl),
    /// `isCompositeEditor(control)`: o `activeCodeEditor` é o editor de código ativo dele, se houver.
    Composite { active_code_editor: Option<Rc<dyn IActiveCodeEditor>> },
    /// Um controle que não é de texto.
    Other,
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

/// `IBaseSaveRevertAllEditorOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BaseSaveRevertAllEditorOptions {
    /// Whether to include untitled editors as well.
    pub include_untitled: Option<IncludeUntitled>,
    /// Whether to exclude sticky editors.
    pub exclude_sticky: Option<bool>,
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

impl SaveAllEditorsOptions {
    /// A parte `IBaseSaveRevertAllEditorOptions`.
    pub fn base(&self) -> BaseSaveRevertAllEditorOptions {
        BaseSaveRevertAllEditorOptions { include_untitled: self.include_untitled, exclude_sticky: self.exclude_sticky }
    }
}

/// `IRevertAllEditorsOptions` (`IRevertOptions` mais `IBaseSaveRevertAllEditorOptions`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RevertAllEditorsOptions {
    pub revert: RevertOptions,
    pub base: BaseSaveRevertAllEditorOptions,
}

/// `ISaveEditorsResult`.
#[derive(Clone)]
pub struct SaveEditorsResult {
    /// Whether the save operation was successful.
    pub success: bool,
    /// Resulting editors after the save operation.
    pub editors: Vec<EditorInputOrUntyped>,
}

/// `IResourceEditorInputIdentifier`: a resource URI, type and editor identifier of an editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceEditorInputIdentifier {
    /// The type of the editor.
    pub type_id: String,
    /// The identifier of the editor if provided.
    pub editor_id: Option<String>,
    /// The resource URI of the editor.
    pub resource: URI,
}

/// `IUntypedEditorReplacement`.
#[derive(Clone)]
pub struct UntypedEditorReplacement {
    /// The editor to replace.
    pub editor: EditorInputRef,
    /// The replacement for the editor.
    pub replacement: UntypedEditorInput,
    /// Skips asking the user for confirmation and doesn't save the document. Only use this if you really need to!
    pub force_replace_dirty: Option<bool>,
}

/// `IEditorReplacement | IUntypedEditorReplacement`.
#[derive(Clone)]
pub enum EditorReplacementInput {
    Typed(EditorReplacement),
    Untyped(UntypedEditorReplacement),
}

/// `IOpenEditorsOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenEditorsOptions {
    /// Whether to validate trust when opening editors. If code is running in an untrusted workspace, the user will be
    /// prompted to trust the workspace.
    pub validate_trust: Option<bool>,
}

/// `IEditorsChangeEvent`.
#[derive(Clone)]
pub struct EditorsChangeEvent {
    pub group_id: GroupIdentifier,
    pub event: GroupModelChangeEvent,
}

/// `IVisibleEditorsChangeEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleEditorsChangeEvent {
    /// Whether the visible editors changed as a result of an explicit user action (`true`) or happened automatically as a
    /// side effect.
    pub is_explicit: bool,
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

    /// `getEditors(order, options?)`: all editors that are opened, in the given order, across all groups (a sobrecarga
    /// sem opções; a com `{ excludeSticky }` é [`IEditorService::get_editors_with_options`]).
    fn get_editors(&self, order: EditorsOrder) -> Vec<EditorIdentifier>;

    /// `openEditors(editors, group?)` sem o grupo preferido e só com a forma `IEditorInputWithOptions[]`: opens the
    /// editors in a group and resolves with the editor panes that are showing them.
    fn open_editors(&self, editors: Vec<EditorInputWithOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>>;

    /// `isVisible(editor)`: determines if the editor is visible in any group (it is the active editor of the group).
    fn is_visible(&self, editor: &EditorInputRef) -> bool;

    // Os membros abaixo completam o `IEditorService`. Todos têm implementação padrão para não quebrar quem implementa só o
    // recorte acima: os eventos nunca disparam, e o resto lança o erro do dublê.

    /// `onDidActiveEditorChange`: fired when the currently active editor changes.
    fn on_did_active_editor_change(&self) -> Event<()> {
        Event::none()
    }

    /// `onDidVisibleEditorsChange`: fired when the set of visible editors changes.
    fn on_did_visible_editors_change(&self) -> Event<VisibleEditorsChangeEvent> {
        Event::none()
    }

    /// `onDidEditorsChange`: an aggregated event for any change to any editor across all groups.
    fn on_did_editors_change(&self) -> Event<EditorsChangeEvent> {
        Event::none()
    }

    /// `onWillOpenEditor`: an event that is fired before an editor is about to open in any group.
    fn on_will_open_editor(&self) -> Event<EditorWillOpenEvent> {
        Event::none()
    }

    /// `onDidCloseEditor`: an event that is fired when an editor is closed in any group.
    fn on_did_close_editor(&self) -> Event<EditorCloseEvent> {
        Event::none()
    }

    /// `onDidOpenEditorFail` (do `EditorServiceImpl`): an event that is fired when opening an editor failed.
    fn on_did_open_editor_fail(&self) -> Event<EditorIdentifier> {
        Event::none()
    }

    /// `onDidMostRecentlyActiveEditorsChange` (do `EditorServiceImpl`): fired when the most recently active editors change.
    fn on_did_most_recently_active_editors_change(&self) -> Event<()> {
        Event::none()
    }

    /// `activeEditorPane`: the currently active editor pane or `None` if none.
    fn active_editor_pane(&self) -> Option<Rc<dyn IEditorPane>> {
        not_implemented()
    }

    /// `visibleEditors`: the currently visible editors, one per visible group.
    fn visible_editors(&self) -> Vec<EditorInputRef> {
        not_implemented()
    }

    /// `visibleTextEditorControls`: the currently visible text editor controls.
    fn visible_text_editor_controls(&self) -> Vec<ActiveTextEditorControl> {
        not_implemented()
    }

    /// `getVisibleTextEditorControls(order)`: the visible text editor controls in the given order of groups.
    fn get_visible_text_editor_controls(&self, _order: EditorsOrder) -> Vec<ActiveTextEditorControl> {
        not_implemented()
    }

    /// `editors`: all editors that are opened across all groups, in sequential order.
    fn editors(&self) -> Vec<EditorInputRef> {
        not_implemented()
    }

    /// `count`: the total number of opened editors.
    fn count(&self) -> usize {
        not_implemented()
    }

    /// `getEditors(order, { excludeSticky })`.
    fn get_editors_with_options(&self, _order: EditorsOrder, _options: Option<GetEditorsOptions>) -> Vec<EditorIdentifier> {
        not_implemented()
    }

    /// `openEditor(editor: IUntypedEditorInput, group)`: a forma sem tipo com o grupo preferido.
    fn open_editor_in(&self, _editor: UntypedEditorInput, _group: Option<PreferredGroup>) -> Promise<Option<Rc<dyn IEditorPane>>> {
        not_implemented()
    }

    /// `openEditor(editor: EditorInput, options?, group?)`: a forma tipada. Os `IEditorOptions` são as
    /// [`TextEditorOptions`](crate::common::editor::TextEditorOptions) (que os incluem).
    fn open_editor_typed(
        &self,
        _editor: &EditorInputRef,
        _options: Option<crate::common::editor::TextEditorOptions>,
        _group: Option<PreferredGroup>,
    ) -> Promise<Option<Rc<dyn IEditorPane>>> {
        not_implemented()
    }

    /// `openEditors(editors, group?, options?)` com as duas formas de editor (`EditorInputWithOptions` ou
    /// `IUntypedEditorInput`, o [`FindGroupEditor`]).
    fn open_editors_in(&self, _editors: Vec<FindGroupEditor>, _group: Option<PreferredGroup>, _options: Option<OpenEditorsOptions>) -> Promise<Vec<Rc<dyn IEditorPane>>> {
        not_implemented()
    }

    /// `replaceEditors(replacements, group)`: replaces the editor in the group with the replacement.
    fn replace_editors(&self, _replacements: Vec<EditorReplacementInput>, _group: GroupRef) -> Promise<()> {
        not_implemented()
    }

    /// `isOpened(editor)`: determines if the editor is opened in any group, by resource, type id and editor id.
    fn is_opened(&self, _editor: &ResourceEditorInputIdentifier) -> bool {
        not_implemented()
    }

    /// `closeEditor(editor, options?)`: closes an editor in its group.
    fn close_editor(&self, _editor: EditorIdentifier, _options: Option<CloseEditorOptions>) -> Promise<()> {
        not_implemented()
    }

    /// `closeEditors(editors, options?)`: closes the editors in their groups.
    fn close_editors(&self, _editors: Vec<EditorIdentifier>, _options: Option<CloseEditorOptions>) -> Promise<()> {
        not_implemented()
    }

    /// `findEditors(editor: IResourceEditorInputIdentifier, options?)`: finds all editors that match the identifier.
    fn find_editors_by_identifier(&self, _editor: &ResourceEditorInputIdentifier, _options: Option<FindEditorOptions>) -> Vec<EditorIdentifier> {
        not_implemented()
    }

    /// `findEditors(resource, options, group)`: finds all editors of a group that match the resource.
    fn find_editors_in_group(&self, _resource: &URI, _options: Option<FindEditorOptions>, _group: GroupRef) -> Vec<EditorInputRef> {
        not_implemented()
    }

    /// `findEditors(editor: IResourceEditorInputIdentifier, options, group)`: finds the editor of a group that matches the
    /// identifier.
    fn find_editor_in_group(&self, _editor: &ResourceEditorInputIdentifier, _options: Option<FindEditorOptions>, _group: GroupRef) -> Option<EditorInputRef> {
        not_implemented()
    }

    /// `revert(editors, options?)`: reverts the editors. Resolves to `true` when no editor is dirty anymore.
    fn revert(&self, _editors: Vec<EditorIdentifier>, _options: Option<RevertOptions>) -> Promise<bool> {
        not_implemented()
    }

    /// `revertAll(options?)`: reverts all editors that are dirty.
    fn revert_all(&self, _options: Option<RevertAllEditorsOptions>) -> Promise<bool> {
        not_implemented()
    }

    /// `createScoped(editorGroupsContainer, disposables)`: creates an editor service scoped to the container.
    fn create_scoped(&self, _editor_groups_container: EditorGroupsContainer, _disposables: &DisposableStore) -> Rc<dyn IEditorService> {
        not_implemented()
    }
}

service_id!(pub I_EDITOR_SERVICE: dyn IEditorService = "editorService");
