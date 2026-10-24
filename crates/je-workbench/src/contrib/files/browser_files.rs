//! Porte parcial de `src/vs/workbench/contrib/files/browser/files.ts`: o `IExplorerService` e o `IExplorerView`.
//!
//! As funções livres do módulo: `getResourceForCommand` (e o `getFocus` dela) está aqui; `getMultiSelectedResources` e
//! `getOpenEditorsViewMultiSelection` entram com os comandos que as usam.
//!
//! `getResourceForCommand`: o `commandArg: unknown` é um [`Dynamic`] e `URI.isUri(commandArg)` é a leitura do `URI` na
//! forma de valor da API ([`ApiValue`], `$$class` `Uri`); o `focus instanceof ExplorerItem` e `instanceof OpenEditor`
//! são `downcast_ref` sobre o elemento que o [`IListWidget`](je_platform::list::list_service::IListWidget) devolve.
//!
//! O `IEditableData` é de `src/vs/workbench/common/views.ts` e vive aqui até esse módulo ser portado.
//!
//! Divergências:
//! - **Membros novos do `IExplorerService`.** `sortOrderConfiguration`, `hasViewFocus`, `setEditable`,
//!   `getEditableData`, `isEditable`, `findClosestRoot`, `refresh`, `isCut`, `registerView` e `getViewId` entram no trait
//!   com um corpo padrão que lança `TypeError: <membro> is not a function`: é o que o objeto do original (um dublê
//!   `{ getContext, getEditable, ... }` registrado com `stub`) faz ao ser chamado em um membro que não define, e o que
//!   mantém os dublês já escritos compilando. O `ExplorerService` sobrescreve todos.
//! - **`refresh(): Promise<void>`.** O `IExplorerService.refresh()` não tem parâmetro; o `reveal = true` é do
//!   `ExplorerService.refresh(reveal)`, que é o método próprio da struct.
//! - **`IExplorerView.isItemCollapsed`.** O `isCollapsed` da árvore lança para um nó que ainda não foi resolvido; o
//!   método devolve `Result` e o `getContext` do serviço trata o `Err` como o `try/catch` do original.
//! - **`IExplorerView.autoReveal`.** A propriedade de leitura e escrita é o par `auto_reveal` e `set_auto_reveal`.

use std::any::Any;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::promise::Promise;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceFileEdit;
use je_instantiation::service_id;

use je_base::dynamic::Dynamic;
use je_platform::list::list_service::IListService;

use super::common_files::{OpenEditor, SortOrderConfiguration};
use super::explorer_model::ExplorerItem;
use crate::api::common::ext_host_values::ApiValue;
use crate::common::editor::{EditorResourceAccessor, EditorResourceAccessorOptions, SideBySideEditor};
use crate::services::editor::editor_service::IEditorService;
use crate::services::textfile::collaborators::ProgressLocation;

/// O `{ content: string; severity: Severity }` de `IEditableData.validationMessage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationMessage {
    pub content: String,
    pub severity: Severity,
}

/// `IEditableData`.
#[derive(Clone)]
pub struct EditableData {
    pub validation_message: Rc<dyn Fn(&str) -> Option<ValidationMessage>>,
    /// `placeholder?: string | null`.
    pub placeholder: Option<String>,
    /// `startingValue?: string | null`.
    pub starting_value: Option<String>,
    pub on_finish: Rc<dyn Fn(String, bool) -> Promise<()>>,
}

/// O `{ stat: ExplorerItem; data: IEditableData }` de `getEditable()`.
#[derive(Clone)]
pub struct EditableItem {
    pub stat: Rc<ExplorerItem>,
    pub data: EditableData,
}

/// O `options` de `applyBulkEdit`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExplorerBulkEditOptions {
    pub undo_label: String,
    pub progress_label: String,
    pub confirm_before_undo: Option<bool>,
    /// `ProgressLocation.Explorer | ProgressLocation.Window`.
    pub progress_location: Option<ProgressLocation>,
}

/// `reveal?: boolean | string` de `select`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExplorerReveal {
    Flag(bool),
    Mode(String),
}

/// O membro que o objeto do original não define: chamá-lo é um `TypeError`.
fn not_a_function(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `IExplorerService` (ver a nota do módulo).
pub trait IExplorerService {
    /// `roots`.
    fn roots(&self) -> Vec<Rc<ExplorerItem>>;

    /// `sortOrderConfiguration`.
    fn sort_order_configuration(&self) -> SortOrderConfiguration {
        not_a_function("sortOrderConfiguration")
    }

    /// `getContext(respectMultiSelection, ignoreNestedChildren?)`.
    fn get_context(&self, respect_multi_selection: bool, ignore_nested_children: Option<bool>) -> Vec<Rc<ExplorerItem>>;

    /// `hasViewFocus()`.
    fn has_view_focus(&self) -> bool {
        not_a_function("hasViewFocus")
    }

    /// `setEditable(stat, data)`: `data` é `null` para terminar a edição.
    fn set_editable(&self, _stat: Rc<ExplorerItem>, _data: Option<EditableData>) -> Promise<()> {
        not_a_function("setEditable")
    }

    /// `getEditable()`.
    fn get_editable(&self) -> Option<EditableItem>;

    /// `getEditableData(stat)`.
    fn get_editable_data(&self, _stat: &Rc<ExplorerItem>) -> Option<EditableData> {
        not_a_function("getEditableData")
    }

    /// `isEditable(stat)`: if `None` is passed checks if any element is currently being edited.
    fn is_editable(&self, _stat: Option<&Rc<ExplorerItem>>) -> bool {
        not_a_function("isEditable")
    }

    /// `findClosest(resource)`.
    fn find_closest(&self, resource: &URI) -> Option<Rc<ExplorerItem>>;

    /// `findClosestRoot(resource)`.
    fn find_closest_root(&self, _resource: &URI) -> Option<Rc<ExplorerItem>> {
        not_a_function("findClosestRoot")
    }

    /// `refresh()`.
    fn refresh(&self) -> Promise<()> {
        not_a_function("refresh")
    }

    /// `setToCopy(stats, cut)`.
    fn set_to_copy(&self, stats: Vec<Rc<ExplorerItem>>, cut: bool) -> Promise<()>;

    /// `isCut(stat)`.
    fn is_cut(&self, _stat: &Rc<ExplorerItem>) -> bool {
        not_a_function("isCut")
    }

    /// `applyBulkEdit(edit, options)`.
    fn apply_bulk_edit(&self, edit: Vec<ResourceFileEdit>, options: ExplorerBulkEditOptions) -> Promise<()>;

    /// Selects and reveal the file element provided by the given resource if its found in the explorer. Will try to
    /// resolve the path in case the explorer is not yet expanded to the file yet.
    fn select(&self, resource: &URI, reveal: Option<ExplorerReveal>) -> Promise<()>;

    /// `registerView(contextAndRefreshProvider)`.
    fn register_view(&self, _context_and_refresh_provider: Rc<dyn IExplorerView>) {
        not_a_function("registerView")
    }

    /// `getViewId()`: the explorer view id for this application, available before the view is instantiated.
    fn get_view_id(&self) -> String {
        not_a_function("getViewId")
    }
}

service_id!(pub I_EXPLORER_SERVICE: dyn IExplorerService = "explorerService");

/// `IExplorerView`.
pub trait IExplorerView {
    /// `id`.
    fn id(&self) -> String;

    /// `autoReveal`: `boolean | 'force' | 'focusNoScroll'`.
    fn auto_reveal(&self) -> ExplorerReveal;

    /// `autoReveal = value`.
    fn set_auto_reveal(&self, value: ExplorerReveal);

    /// `getContext(respectMultiSelection)`.
    fn get_context(&self, respect_multi_selection: bool) -> Vec<Rc<ExplorerItem>>;

    /// `refresh(recursive, item?, cancelEditing?)`.
    fn refresh(&self, recursive: bool, item: Option<&Rc<ExplorerItem>>, cancel_editing: Option<bool>) -> Promise<()>;

    /// `selectResource(resource, reveal?, retry?)`.
    fn select_resource(&self, resource: Option<&URI>, reveal: Option<ExplorerReveal>, retry: Option<usize>) -> Promise<()>;

    /// `setTreeInput()`.
    fn set_tree_input(&self) -> Promise<()>;

    /// `itemsCopied(stats, cut, previousCut)`.
    fn items_copied(&self, stats: &[Rc<ExplorerItem>], cut: bool, previous_cut: Option<&[Rc<ExplorerItem>]>);

    /// `setEditable(stat, isEditing)`.
    fn set_editable(&self, stat: &Rc<ExplorerItem>, is_editing: bool) -> Promise<()>;

    /// `isItemVisible(item)`.
    fn is_item_visible(&self, item: &Rc<ExplorerItem>) -> bool;

    /// `isItemCollapsed(item)`: `Err` quando o nó não está resolvido (o `isCollapsed` da árvore lança).
    fn is_item_collapsed(&self, item: &Rc<ExplorerItem>) -> Result<bool, Error>;

    /// `hasFocus()`.
    fn has_focus(&self) -> bool;

    /// `getFocus()`.
    fn get_focus(&self) -> Vec<Rc<ExplorerItem>>;

    /// `focusNext()`.
    fn focus_next(&self);

    /// `focusLast()`.
    fn focus_last(&self);

    /// `hasPhantomElements()`.
    fn has_phantom_elements(&self) -> bool;
}

/// `getFocus(listService)`: o elemento em foco da última lista focada, se a lista tem o foco do documento.
fn get_focus(list_service: &dyn IListService) -> Option<Rc<dyn Any>> {
    let list = list_service.last_focused_list()?;
    if list.is_active_element() { list.get_focused_element() } else { None }
}

/// `getResourceForCommand(commandArg, editorService, listService)`: commands can get executed from a command palette, from
/// a context menu or from some list using a keybinding. To cover all these cases we need to properly compute the resource
/// on which the command is being executed.
pub fn get_resource_for_command(command_arg: Option<&Dynamic>, editor_service: &dyn IEditorService, list_service: &dyn IListService) -> Option<URI> {
    if let Some(resource) = command_arg.and_then(|arg| <URI as ApiValue>::from_dynamic(arg).ok()) {
        return Some(resource);
    }

    let focus = get_focus(list_service);
    if let Some(focus) = focus.as_deref() {
        if let Some(item) = focus.downcast_ref::<ExplorerItem>() {
            return Some(item.resource());
        } else if let Some(open_editor) = focus.downcast_ref::<OpenEditor>() {
            return open_editor.get_resource();
        }
    }

    EditorResourceAccessor::get_original_uri(
        editor_service.active_editor().as_ref(),
        Some(&EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Primary), filter_by_scheme: None }),
    )
}
