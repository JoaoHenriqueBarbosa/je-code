//! Porte de `src/vs/workbench/browser/parts/editor/editorTypePicker.ts`: os editores disponíveis para o recurso do editor
//! ativo ([`get_available_editor_types`]), o rótulo de cada um ([`editor_type_display_label`]) e as ações que reabrem o
//! editor com outro tipo ([`create_editor_type_actions`]).
//!
//! Notas de tradução:
//! - **`activeEditor?.editorId ?? DEFAULT_EDITOR_ASSOCIATION.id`** é `editor_id().unwrap_or(DEFAULT_EDITOR_ASSOCIATION_ID)`.
//! - **`isDiffEditorInput(activeEditor)` e `isEditorInputWithDiffResources`** leem `diff_editors()` e `diff_resources()` da
//!   entrada (o `(modified, original)` do primeiro vira `{ original, modified }`).
//! - **`toAction({ ..., run })`** é o [`to_action`] de `je-base`; o `run` devolve um `MaybePromise`, e o `executeCommand` do
//!   comando de reabrir é a promessa dele. O `new Error('The source editor cannot be opened to the side.')` é um
//!   [`errors::throw`], como o `throw` dentro do `run` do original.
//! - **`localize({ key, comment }, ...)`** das duas mensagens com marcadores é o `localize` de `je-base` com os argumentos.

use std::rc::Rc;

use je_base::actions::{ActionRef, Separator, SubmenuAction, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_base::uri::URI;
use je_platform::commands::ICommandService;

use crate::browser::parts::editor::editor::{IEditorGroupView, fill_active_editor_selection};
use crate::common::editor::{
    DEFAULT_EDITOR_ASSOCIATION_ID, EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, EditorResourceAccessor, EditorResourceAccessorOptions, SideBySideEditor,
    UntypedEditorInput,
};
use crate::services::editor::editor_resolver_service::{
    EditorMatches, EditorResolverServiceGetEditorMatchesOptions, IEditorResolverService, RegisteredEditorInfo, is_unconfigured_universal_optional_editor_match,
};
use crate::services::editor::editor_service::IEditorService;
use crate::services::editor::editor_groups_service::{IEditorGroup, PreferredGroup};
use je_platform::opener::editor_types::EditorOverride;

/// `REOPEN_ACTIVE_EDITOR_WITH_COMMAND_ID` (`editorCommands.ts`).
pub const REOPEN_ACTIVE_EDITOR_WITH_COMMAND_ID: &str = "reopenActiveEditorWith";

/// `IAvailableEditorTypes`: the editors available for the active editor's resource, i.e. the different editor types
/// (e.g. "Text Editor" vs. "Markdown Preview") that the resource can be reopened with.
#[derive(Clone)]
pub struct IAvailableEditorTypes {
    pub resource: URI,
    pub is_diff_editor: bool,
    pub original_resource: Option<URI>,
    pub modified_resource: Option<URI>,
    pub current_id: String,
    pub editor_matches: EditorMatches,
    pub editors: Vec<RegisteredEditorInfo>,
}

/// `getAvailableEditorTypes(activeEditor, editorResolverService, hiddenEditorIds?)`: determines the editors available for
/// the given active editor's resource. Returns `None` when there is nothing meaningful to switch between: no resource, only
/// the default text editor, or an exclusive editor (e.g. the hex editor).
pub fn get_available_editor_types(active_editor: Option<&EditorInputRef>, editor_resolver_service: &dyn IEditorResolverService, hidden_editor_ids: Option<&[String]>) -> Option<IAvailableEditorTypes> {
    let standard_diff_resources = active_editor.and_then(|editor| editor.diff_editors()).map(|(modified, original)| (original.resource(), modified.resource()));
    let diff_resources = match standard_diff_resources {
        Some((original, modified)) => Some((original, modified)),
        None => active_editor.and_then(|editor| editor.diff_resources()).map(|(original, modified)| (Some(original), Some(modified))),
    };
    let typed = active_editor.cloned().map(EditorInputOrUntyped::Typed);
    let options = EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Primary), filter_by_scheme: None };
    let resource = diff_resources.as_ref().and_then(|(_, modified)| modified.clone()).or_else(|| EditorResourceAccessor::get_original_uri(typed.as_ref(), Some(&options)))?;
    let current_id = active_editor.and_then(|editor| editor.editor_id()).unwrap_or_else(|| DEFAULT_EDITOR_ASSOCIATION_ID.to_owned());
    let hidden_editor_id_set: std::collections::HashSet<&String> = hidden_editor_ids.unwrap_or_default().iter().collect();
    let editor_matches = editor_resolver_service.get_editor_matches(&resource, Some(&EditorResolverServiceGetEditorMatchesOptions { is_diff_editor: Some(diff_resources.is_some()) }));
    let editors: Vec<RegisteredEditorInfo> = editor_matches
        .matches()
        .iter()
        .filter(|rule| !is_unconfigured_universal_optional_editor_match(rule) || rule.editor.id == current_id)
        .map(|rule| rule.editor.clone())
        .filter(|editor| editor.id == current_id || !hidden_editor_id_set.contains(&editor.id))
        .collect();
    if editor_matches.has_exclusive_match() || editors.len() <= 1 {
        return None;
    }
    let (original_resource, modified_resource) = match diff_resources {
        Some((original, modified)) => (original, modified),
        None => (None, None),
    };
    Some(IAvailableEditorTypes { resource, is_diff_editor: modified_resource.is_some() || original_resource.is_some(), original_resource, modified_resource, current_id, editor_matches, editors })
}

/// `editorTypeDisplayLabel(editor, isDiffEditor)`: the label to show for an editor type. In a diff context the default text
/// editor is presented as "Text Diff Editor" to match how it actually opens.
pub fn editor_type_display_label(editor: &RegisteredEditorInfo, is_diff_editor: bool) -> String {
    if is_diff_editor && editor.id == DEFAULT_EDITOR_ASSOCIATION_ID {
        return localize("textDiffEditor", "Text Diff Editor", &[]);
    }
    editor.label.clone()
}

/// `createEditorTypeActions(available, group, editorResolverService, commandService, editorService)`: builds the actions
/// that let the user switch between the editor types available for a resource: one "reopen with" action per available editor
/// (the current one checked), a "Set Default" submenu for persisting the choice, and, for diffs, actions to open either
/// side as a standalone editor.
pub fn create_editor_type_actions(
    available: &IAvailableEditorTypes,
    group: Rc<dyn IEditorGroupView>,
    editor_resolver_service: Rc<dyn IEditorResolverService>,
    command_service: Rc<dyn ICommandService>,
    editor_service: Rc<dyn IEditorService>,
) -> Vec<ActionRef> {
    let default_rule = available.editor_matches.default_rule().clone();
    let glob = default_rule.association_pattern.clone();
    let source_editor = group.active_editor();
    let source_input = source_editor.as_ref().and_then(|editor| editor.to_untyped(None));
    let reopen: Rc<dyn Fn(&str) -> je_base::promise::Promise<Dynamic>> = {
        let (group, command_service, source_editor) = (group.clone(), command_service.clone(), source_editor.clone());
        Rc::new(move |editor_id: &str| {
            let editor_index = source_editor.as_ref().map_or(-1, |editor| group.get_index_of_editor(editor));
            command_service.execute_command(
                REOPEN_ACTIVE_EDITOR_WITH_COMMAND_ID,
                vec![Dynamic::string(editor_id), Dynamic::object([("groupId", Dynamic::Number(group.id() as f64)), ("editorIndex", Dynamic::Number(editor_index as f64))])],
            )
        })
    };

    // Show the contributing extension in parentheses, but only for extension-provided editors.
    // Built-in providers share this localized label, so their (redundant) source is omitted.
    let builtin_provider_label = localize("builtinProviderDisplayName", "Built-in", &[]);
    let is_diff_editor = available.is_diff_editor;
    let label_with_source = move |editor: &RegisteredEditorInfo| -> String {
        let label = editor_type_display_label(editor, is_diff_editor);
        match &editor.detail {
            Some(detail) if !detail.is_empty() && *detail != builtin_provider_label => localize("editorType.labelWithSource", "{0} - {1}", &[label.as_str(), detail.as_str()]),
            _ => label,
        }
    };

    // Reopen the active editor with the chosen editor type. The currently active type is checked.
    let reopen_actions: Vec<ActionRef> = available
        .editors
        .iter()
        .map(|editor| {
            let is_current = editor.id == available.current_id;
            let alt = if is_current && source_editor.as_ref().is_some_and(|source| source.has_capability(EditorInputCapabilities::SINGLETON)) {
                None
            } else {
                let (group, editor_service, source_editor, source_input, editor_id) = (group.clone(), editor_service.clone(), source_editor.clone(), source_input.clone(), editor.id.clone());
                Some(to_action(ToActionProps {
                    id: format!("editorType.openToSide.{}", editor.id),
                    label: localize("editorType.openToSide", "{0} (Open to the Side)", &[label_with_source(editor).as_str()]),
                    tooltip: None,
                    enabled: Some(source_input.is_some()),
                    checked: None,
                    class: None,
                    alt: None,
                    run: Rc::new(move |_| {
                        let (Some(source_editor), Some(source_input)) = (&source_editor, &source_input) else {
                            errors::throw(Error::new("The source editor cannot be opened to the side."));
                        };
                        let group_view: Rc<dyn IEditorGroupView> = group.clone();
                        group.groups_view().activate_group(&group_view);
                        let mut preset = source_input.options.clone().unwrap_or_default();
                        preset.base.override_ = Some(EditorOverride::Id(editor_id.as_str().into()));
                        preset.base.pinned = Some(true);
                        preset.base.preserve_focus = Some(false);
                        let options = fill_active_editor_selection(&*group, source_editor, preset);
                        let untyped = UntypedEditorInput { options: Some(options), ..source_input.clone() };
                        MaybePromise::Promise(editor_service.open_editor_in(untyped, Some(PreferredGroup::Side)).then_result(|_| Ok(Dynamic::Undefined)))
                    }),
                }))
            };
            let (reopen, editor_id) = (reopen.clone(), editor.id.clone());
            to_action(ToActionProps {
                id: editor.id.clone(),
                label: label_with_source(editor),
                tooltip: None,
                enabled: None,
                checked: Some(is_current),
                class: None,
                alt,
                run: Rc::new(move |_| MaybePromise::Promise(reopen(&editor_id))),
            })
        })
        .collect();

    // Persist the chosen editor as the default for this file type. For diffs this updates the specialized
    // `workbench.diffEditorAssociations` setting instead of the general one. The effective default is checked. Setting a
    // default also reopens the active editor with that type so the change takes effect immediately.
    let set_default_actions: Vec<ActionRef> = available
        .editors
        .iter()
        .map(|editor| {
            let (editor_resolver_service, reopen, editor_id, resource) = (editor_resolver_service.clone(), reopen.clone(), editor.id.clone(), available.resource.clone());
            to_action(ToActionProps {
                id: format!("setDefault.{}", editor.id),
                label: label_with_source(editor),
                tooltip: None,
                enabled: None,
                checked: Some(editor.id == default_rule.editor.id),
                class: None,
                alt: None,
                run: Rc::new(move |_| {
                    editor_resolver_service.set_default_editor(&resource, &editor_id, Some(is_diff_editor));
                    MaybePromise::Promise(reopen(&editor_id))
                }),
            })
        })
        .collect();
    let set_default_submenu_label = if available.is_diff_editor {
        localize("editorType.setDefaultDiff", "Set Default (Diff Only) for '{0}'", &[glob.as_str()])
    } else {
        localize("editorType.setDefault", "Set Default for '{0}'", &[glob.as_str()])
    };
    let set_default_submenu: ActionRef = SubmenuAction::new("editorType.setDefault", &set_default_submenu_label, set_default_actions, None);

    let mut actions: Vec<ActionRef> = reopen_actions;
    actions.push(Separator::new());
    actions.push(set_default_submenu);

    // For diffs, offer to open either side as a standalone editor.
    if available.is_diff_editor {
        actions.push(Separator::new());
        for (id, label, resource) in [
            ("editorType.openOriginal", localize("editorType.openOriginal", "Open Original", &[]), &available.original_resource),
            ("editorType.openModified", localize("editorType.openModified", "Open Modified", &[]), &available.modified_resource),
        ] {
            if let Some(resource) = resource {
                let (editor_service, resource) = (editor_service.clone(), resource.clone());
                actions.push(to_action(ToActionProps {
                    id: id.to_owned(),
                    label,
                    tooltip: None,
                    enabled: None,
                    checked: None,
                    class: None,
                    alt: None,
                    run: Rc::new(move |_| {
                        MaybePromise::Promise(editor_service.open_editor(UntypedEditorInput { resource: Some(resource.clone()), ..UntypedEditorInput::default() }).then_result(|_| Ok(Dynamic::Undefined)))
                    }),
                }));
            }
        }
    }

    actions
}
