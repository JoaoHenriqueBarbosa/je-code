//! Porte de `src/vs/workbench/browser/parts/editor/editorCommandsContext.ts`: a resolução do contexto de um comando de editor
//! (os grupos e os editores sobre os quais ele age) a partir dos argumentos, da lista com foco e da seleção do grupo.
//!
//! Divergências:
//! - **`commandArgs: unknown[]`.** O original só sabe extrair o contexto de um `IEditorCommandsContext` e de uma `URI`
//!   (`isEditorCommandsContext(arg) || URI.isUri(arg)`) e descarta o resto. O argumento é o [`CommandArg`], com uma variante
//!   para cada forma que o original reconhece e [`CommandArg::Other`] para o que ele descarta.
//! - **`list instanceof List && list.getHTMLElement() === getActiveElement()`.** É [`IListWidget::is_list`] e
//!   [`IListWidget::is_active_element`] da lista focada por último (sem DOM, ver `list_service`).
//! - **`getFocusedElements()` e `getSelectedElements()`.** São [`IListWidget::get_focused_elements`] e
//!   [`IListWidget::get_selected_elements`]. Cada elemento viaja como `Rc<dyn Any>`: um `IEditorIdentifier` é o
//!   [`EditorIdentifier`] e um `IEditorGroup` é o `Rc<dyn IEditorGroup>` (o `isEditorIdentifier` e o `isEditorGroup` do
//!   original são o `downcast_ref` para um deles).
//! - **`editorIndex: -1`.** O `getIndexOfEditor` do original devolve `-1` para um editor que o grupo não tem, e o
//!   `getEditorByIndex(-1)` devolve `undefined`. O índice do contexto é `usize` ([`EditorCommandsContext`]), então o `-1` é
//!   o `usize::MAX`, que também não existe no grupo ([`context_index`]).
//! - **`throw new Error(...)`** é o [`errors::throw`](je_base::errors::throw).

use std::any::Any;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::uri::URI;
use je_platform::list::list_service::IListService;

use crate::common::editor::{EditorCommandsContext, EditorIdentifier, EditorInputRef};
use crate::services::editor::editor_groups_service::{IEditorGroup, IEditorGroupsService, IEditorPart};
use crate::services::editor::editor_service::IEditorService;

/// Um elemento de `commandArgs: unknown[]`.
#[derive(Clone)]
pub enum CommandArg {
    /// `IEditorCommandsContext`.
    Context(EditorCommandsContext),
    /// `URI`.
    Uri(URI),
    /// Qualquer outra forma, que o original descarta.
    Other,
}

impl From<EditorCommandsContext> for CommandArg {
    fn from(context: EditorCommandsContext) -> Self {
        CommandArg::Context(context)
    }
}

impl From<URI> for CommandArg {
    fn from(uri: URI) -> Self {
        CommandArg::Uri(uri)
    }
}

/// `IResolvedEditorCommandsContext.groupedEditors[number]`.
pub struct GroupedEditors {
    pub group: Rc<dyn IEditorGroup>,
    pub editors: Vec<EditorInputRef>,
}

/// `IResolvedEditorCommandsContext`.
pub struct ResolvedEditorCommandsContext {
    pub grouped_editors: Vec<GroupedEditors>,
    pub preserve_focus: bool,
}

/// `resolveCommandsContext(commandArgs, editorService, editorGroupsService, listService)`.
pub fn resolve_commands_context(
    command_args: &[CommandArg],
    editor_service: &dyn IEditorService,
    editor_groups_service: &dyn IEditorGroupsService,
    list_service: &dyn IListService,
) -> ResolvedEditorCommandsContext {
    let command_context = get_commands_context(command_args, editor_service, editor_groups_service, list_service);
    let preserve_focus = command_context.first().is_some_and(|context| context.preserve_focus.unwrap_or(false));
    let mut resolved_context = ResolvedEditorCommandsContext { grouped_editors: Vec::new(), preserve_focus };

    for editor_context in &command_context {
        let Some((group, editor)) = get_editor_and_group_from_context(editor_context, editor_groups_service) else {
            continue;
        };

        // Find group context if already added
        let existing = resolved_context.grouped_editors.iter().position(|target_group_context| target_group_context.group.id() == group.id());

        // Otherwise add new group context
        let index = existing.unwrap_or_else(|| {
            resolved_context.grouped_editors.push(GroupedEditors { group, editors: Vec::new() });
            resolved_context.grouped_editors.len() - 1
        });

        // Add editor to group context
        if let Some(editor) = editor {
            resolved_context.grouped_editors[index].editors.push(editor);
        }
    }

    resolved_context
}

fn get_commands_context(
    command_args: &[CommandArg],
    editor_service: &dyn IEditorService,
    editor_groups_service: &dyn IEditorGroupsService,
    list_service: &dyn IListService,
) -> Vec<EditorCommandsContext> {
    // Figure out if command is executed from a list
    let list = list_service.last_focused_list();
    let mut is_list_action = list.is_some_and(|list| list.is_list() && list.is_active_element());

    // Get editor context for which the command was triggered
    let editor_context = get_editor_context_from_command_args(command_args, is_list_action, editor_service, editor_groups_service, list_service);

    // If the editor context can not be determined use the active editor
    let editor_context = editor_context.unwrap_or_else(|| {
        let active_group = match editor_groups_service.active_modal_editor_part() {
            Some(modal_editor_part) => modal_editor_part.active_group(),
            None => editor_groups_service.active_group(),
        };
        let active_editor = active_group.active_editor();
        is_list_action = false;
        EditorCommandsContext {
            group_id: active_group.id(),
            editor_index: active_editor.map(|active_editor| context_index(active_group.get_index_of_editor(&active_editor))),
            preserve_focus: None,
        }
    });

    let multi_editor_context = get_multi_select_context(&editor_context, is_list_action, editor_service, editor_groups_service, list_service);

    // Make sure the command context is the first one in the list
    move_current_editor_context_to_front(editor_context, multi_editor_context)
}

fn move_current_editor_context_to_front(editor_context: EditorCommandsContext, mut multi_editor_context: Vec<EditorCommandsContext>) -> Vec<EditorCommandsContext> {
    if multi_editor_context.len() <= 1 {
        return multi_editor_context;
    }

    let editor_context_index =
        multi_editor_context.iter().position(|context| context.group_id == editor_context.group_id && context.editor_index == editor_context.editor_index);

    if let Some(editor_context_index) = editor_context_index {
        multi_editor_context.remove(editor_context_index);
        multi_editor_context.insert(0, editor_context);
    } else if editor_context.editor_index.is_none() {
        multi_editor_context.insert(0, editor_context);
    } else {
        errors::throw(Error::new("Editor context not found in multi editor context"));
    }

    multi_editor_context
}

fn get_editor_context_from_command_args(
    command_args: &[CommandArg],
    is_list_action: bool,
    editor_service: &dyn IEditorService,
    editor_groups_service: &dyn IEditorGroupsService,
    list_service: &dyn IListService,
) -> Option<EditorCommandsContext> {
    // We only know how to extraxt the command context from URI and IEditorCommandsContext arguments
    let filtered_args: Vec<&CommandArg> = command_args.iter().filter(|arg| !matches!(arg, CommandArg::Other)).collect();

    // If the command arguments contain an editor context, use it
    for arg in &filtered_args {
        if let CommandArg::Context(context) = arg {
            return Some(*context);
        }
    }

    // Otherwise, try to find the editor group by the URI of the resource
    for arg in &filtered_args {
        let CommandArg::Uri(uri) = arg else { continue };
        let editor_identifiers = editor_service.find_editors(uri, None);
        if let Some(editor_identifier) = editor_identifiers.first() {
            let group = editor_groups_service.get_group(editor_identifier.group_id);
            return Some(EditorCommandsContext {
                group_id: editor_identifier.group_id,
                editor_index: group.map(|group| context_index(group.get_index_of_editor(&editor_identifier.editor))),
                preserve_focus: None,
            });
        }
    }

    // If there is no context in the arguments, try to find the context from the focused list
    // if the action was executed from a list
    if is_list_action && let Some(list) = list_service.last_focused_list() {
        for focused_element in list.get_focused_elements() {
            if let Some(element) = as_group_or_editor(&focused_element) {
                return Some(group_or_editor_to_editor_context(&element, None, editor_groups_service));
            }
        }
    }

    None
}

fn get_multi_select_context(
    editor_context: &EditorCommandsContext,
    is_list_action: bool,
    editor_service: &dyn IEditorService,
    editor_groups_service: &dyn IEditorGroupsService,
    list_service: &dyn IListService,
) -> Vec<EditorCommandsContext> {
    // If the action was executed from a list, return all selected editors
    if is_list_action && let Some(list) = list_service.last_focused_list() {
        let selection: Vec<GroupOrEditor> = list.get_selected_elements().iter().filter_map(as_group_or_editor).collect();

        if selection.len() > 1 {
            return selection.iter().map(|element| group_or_editor_to_editor_context(element, editor_context.preserve_focus, editor_groups_service)).collect();
        }

        if selection.is_empty() {
            // TODO@benibenj workaround for https://github.com/microsoft/vscode/issues/224050
            // Explainer: the `isListAction` flag can be a false positive in certain cases because
            // it will be `true` if the active element is a `List` even if it is part of the editor
            // area. The workaround here is to fallback to `isListAction: false` if the list is not
            // having any editor or group selected.
            return get_multi_select_context(editor_context, false, editor_service, editor_groups_service, list_service);
        }
    }
    // Check editors selected in the group (tabs)
    else {
        let group = editor_groups_service.get_group(editor_context.group_id);
        let editor = group.as_ref().and_then(|group| match editor_context.editor_index {
            Some(editor_index) => group.get_editor_by_index(editor_index),
            None => group.active_editor(),
        });
        // If the editor is selected, return all selected editors otherwise only use the editors context
        if let (Some(group), Some(editor)) = (&group, &editor)
            && group.is_selected(editor)
        {
            return group
                .selected_editors()
                .into_iter()
                .map(|editor| {
                    let element = GroupOrEditor::Editor(EditorIdentifier { editor, group_id: group.id() });
                    group_or_editor_to_editor_context(&element, editor_context.preserve_focus, editor_groups_service)
                })
                .collect();
        }
    }

    // Otherwise go with passed in context
    vec![*editor_context]
}

/// `IEditorIdentifier | IEditorGroup`: o elemento de uma lista de editores ou de grupos.
enum GroupOrEditor {
    Group(Rc<dyn IEditorGroup>),
    Editor(EditorIdentifier),
}

fn group_or_editor_to_editor_context(element: &GroupOrEditor, preserve_focus: Option<bool>, editor_groups_service: &dyn IEditorGroupsService) -> EditorCommandsContext {
    match element {
        GroupOrEditor::Group(group) => EditorCommandsContext { group_id: group.id(), editor_index: None, preserve_focus },
        GroupOrEditor::Editor(element) => {
            let group = editor_groups_service.get_group(element.group_id);
            let editor_index = match group {
                Some(group) => context_index(group.get_index_of_editor(&element.editor)),
                None => context_index(-1),
            };
            EditorCommandsContext { group_id: element.group_id, editor_index: Some(editor_index), preserve_focus }
        }
    }
}

/// `isGroupOrEditor(element)`: `isEditorGroup(element) || isEditorIdentifier(element)`.
fn as_group_or_editor(element: &Rc<dyn Any>) -> Option<GroupOrEditor> {
    if let Some(group) = element.downcast_ref::<Rc<dyn IEditorGroup>>() {
        return Some(GroupOrEditor::Group(group.clone()));
    }

    element.downcast_ref::<EditorIdentifier>().map(|identifier| GroupOrEditor::Editor(identifier.clone()))
}

/// O índice de um editor no grupo como o `editorIndex` do contexto: o `-1` de "não está no grupo" é o `usize::MAX` (ver a nota do
/// módulo).
fn context_index(index: i64) -> usize {
    usize::try_from(index).unwrap_or(usize::MAX)
}

fn get_editor_and_group_from_context(command_context: &EditorCommandsContext, editor_groups_service: &dyn IEditorGroupsService) -> Option<(Rc<dyn IEditorGroup>, Option<EditorInputRef>)> {
    let group = editor_groups_service.get_group(command_context.group_id)?;

    let Some(editor_index) = command_context.editor_index else {
        return Some((group, None));
    };

    let editor = group.get_editor_by_index(editor_index);
    Some((group, editor))
}
