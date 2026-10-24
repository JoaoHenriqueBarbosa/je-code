//! Porte de `src/vs/workbench/services/editor/common/editorGroupFinder.ts`: achar o grupo de editores de destino que
//! melhor serve a um editor e respeita o grupo preferido quando possível (`findGroup`).
//!
//! Divergências:
//! - **Sobrecargas.** As entradas `EditorInputWithOptions | IUntypedEditorInput` são o enum [`FindGroupEditor`], e o
//!   retorno `Promise<[IEditorGroup, EditorActivation | undefined]> | [IEditorGroup, EditorActivation | undefined]` é
//!   [`FindGroupResult`] (`MaybePromise` de uma tupla). O `accessor` é o [`ServicesAccessor`]. As sobrecargas que só
//!   diferem no tipo estático do grupo preferido (`AUX_WINDOW_GROUP`, `MODAL_GROUP` sempre assíncronas) colapsam numa
//!   função: o `MaybePromise` carrega a diferença.
//! - **Grupos e serviço de grupos** vêm de [`editor_group_contracts`](super::editor_group_contracts), o recorte que o
//!   porte tem do `IEditorGroupsService`.
//! - **Comparações de grupo** (`editorGroupService.activeGroup !== group`) comparam o `id` do grupo.
//! - **`handleModalEditorPart`** é `async`: o `await modalEditorPart.close()` (um `boolean`) custa uma rodada de
//!   microtarefas, que o porte reproduz com uma promessa já resolvida.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::Truthy;
use je_base::promise::{MaybePromise, Promise};
use je_instantiation::ServicesAccessor;
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::opener::editor_types::EditorActivation;

use crate::common::editor::{EditorInputCapabilities, EditorInputOrUntyped, EditorInputRef, EditorInputWithOptions, TextEditorOptions, UntypedEditorInput};
use crate::services::editor::editor_group_contracts::{
    AUX_WINDOW_GROUP, FindGroupScope, GroupRef, GroupsOrder, I_EDITOR_GROUPS_SERVICE, IEditorGroup, IEditorGroupsService, IEditorPart, IModalEditorPart, MODAL_GROUP,
    PreferredGroup, SIDE_GROUP, UseModalEditorMode, preferred_side_by_side_group_direction, use_modal_editor_mode,
};

/// `EditorInputWithOptions | IUntypedEditorInput`: o editor que procura grupo.
#[derive(Clone)]
pub enum FindGroupEditor {
    WithOptions(EditorInputWithOptions),
    Untyped(UntypedEditorInput),
}

impl From<EditorInputWithOptions> for FindGroupEditor {
    fn from(editor: EditorInputWithOptions) -> Self {
        FindGroupEditor::WithOptions(editor)
    }
}

impl From<UntypedEditorInput> for FindGroupEditor {
    fn from(editor: UntypedEditorInput) -> Self {
        FindGroupEditor::Untyped(editor)
    }
}

impl FindGroupEditor {
    /// `editor.options`.
    fn options(&self) -> Option<&TextEditorOptions> {
        match self {
            FindGroupEditor::WithOptions(editor) => editor.options.as_ref(),
            FindGroupEditor::Untyped(editor) => editor.options.as_ref(),
        }
    }

    /// `isEditorInputWithOptions(input) ? input.editor : input`.
    fn editor(&self) -> EditorInputOrUntyped {
        match self {
            FindGroupEditor::WithOptions(editor) => EditorInputOrUntyped::Typed(editor.editor.clone()),
            FindGroupEditor::Untyped(editor) => EditorInputOrUntyped::Untyped(editor.clone()),
        }
    }

    /// `isEditorInputWithOptions(editor) ? editor.editor : isEditorInput(editor) ? editor : undefined`: a entrada tipada,
    /// quando há.
    fn editor_input(&self) -> Option<EditorInputRef> {
        match self {
            FindGroupEditor::WithOptions(editor) => Some(editor.editor.clone()),
            FindGroupEditor::Untyped(_) => None,
        }
    }
}

/// `FindGroupResult`.
pub type FindGroupResult = MaybePromise<(Rc<dyn IEditorGroup>, Option<EditorActivation>)>;

/// `findGroup`: finds the target `IEditorGroup` given the instructions provided that is best for the editor and matches
/// the preferred group if possible.
pub fn find_group(accessor: &ServicesAccessor, editor: &FindGroupEditor, preferred_group: Option<&PreferredGroup>) -> FindGroupResult {
    let editor_group_service = accessor.get(I_EDITOR_GROUPS_SERVICE);
    let configuration_service = accessor.get(I_CONFIGURATION_SERVICE);

    let group = do_find_group(editor, preferred_group, &editor_group_service, &*configuration_service);
    match group {
        MaybePromise::Promise(group) => {
            let (editor, preferred_group) = (editor.clone(), preferred_group.cloned());
            MaybePromise::Promise(
                group.then_maybe(move |group| handle_group_result(group, &editor, preferred_group.as_ref(), &editor_group_service, &*configuration_service)),
            )
        }
        MaybePromise::Value(group) => handle_group_result(group, editor, preferred_group, &editor_group_service, &*configuration_service),
    }
}

fn handle_group_result(
    group: Rc<dyn IEditorGroup>,
    editor: &FindGroupEditor,
    preferred_group: Option<&PreferredGroup>,
    editor_group_service: &Rc<dyn IEditorGroupsService>,
    configuration_service: &dyn IConfigurationService,
) -> FindGroupResult {
    let modal_editor_part = editor_group_service.active_modal_editor_part();
    let modal_editor_mode = use_modal_editor_mode(configuration_service);
    let editor_input = editor.editor_input();
    // The `RequiresModal` capability is honored unless the user has explicitly
    // disabled modal editors via `workbench.editor.useModal: 'off'`, in which
    // case the user preference wins.
    let requires_modal = editor_input.is_some_and(|editor_input| editor_input.has_capability(EditorInputCapabilities::REQUIRES_MODAL))
        && modal_editor_mode != Some(UseModalEditorMode::Off);
    if let Some(modal_editor_part) = modal_editor_part
        && !matches!(preferred_group, Some(PreferredGroup::Modal))
        && modal_editor_mode != Some(UseModalEditorMode::All)
        && !requires_modal
    {
        // Only allow to open in modal group if MODAL_GROUP is explicitly requested
        // or when the setting is configured to open all editors modal or when the
        // editor has the RequiresModal capability.
        return MaybePromise::Promise(handle_modal_editor_part(group, editor, modal_editor_part, editor_group_service, preferred_group));
    }

    MaybePromise::Value(handle_group_activation(group, editor, preferred_group, &**editor_group_service))
}

fn handle_modal_editor_part(
    group: Rc<dyn IEditorGroup>,
    editor: &FindGroupEditor,
    modal_editor_part: Rc<dyn IModalEditorPart>,
    editor_group_service: &Rc<dyn IEditorGroupsService>,
    preferred_group: Option<&PreferredGroup>,
) -> Promise<(Rc<dyn IEditorGroup>, Option<EditorActivation>)> {
    let (editor, editor_group_service, preferred_group) = (editor.clone(), editor_group_service.clone(), preferred_group.cloned());
    async_fn(async move {
        let preserve_focus = editor.options().is_some_and(|options| options.preserve_focus == Some(true));
        let mut group = group;

        // If the resolved group is part of the modal, redirect
        // to the main window active group instead
        if modal_editor_part.groups().iter().any(|modal_group| modal_group.id() == group.id()) {
            group = editor_group_service.main_part().active_group();
        }

        // Try to close the modal editor part unless preserveFocus is set
        if !preserve_focus {
            Promise::resolved(modal_editor_part.close()).await?;
        }

        Ok(handle_group_activation(group, &editor, preferred_group.as_ref(), &*editor_group_service))
    })
}

fn handle_group_activation(
    group: Rc<dyn IEditorGroup>,
    editor: &FindGroupEditor,
    preferred_group: Option<&PreferredGroup>,
    editor_group_service: &dyn IEditorGroupsService,
) -> (Rc<dyn IEditorGroup>, Option<EditorActivation>) {
    // Resolve editor activation strategy
    let mut activation = None;
    if editor_group_service.active_group().id() != group.id() // only if target group is not already active
        && editor.options().is_some_and(|options| options.inactive != Some(true) // never for inactive editors
            && options.preserve_focus == Some(true) // only if preserveFocus
            && options.activation.is_none()) // only if activation is not already defined (either true or false)
        && !matches!(preferred_group, Some(PreferredGroup::Side)) // never for the SIDE_GROUP
    {
        // If the resolved group is not the active one, we typically
        // want the group to become active. There are a few cases
        // where we stay away from encorcing this, e.g. if the caller
        // is already providing `activation`.
        //
        // Specifically for historic reasons we do not activate a
        // group is it is opened as `SIDE_GROUP` with `preserveFocus:true`.
        // repeated Alt-clicking of files in the explorer always open
        // into the same side group and not cause a group to be created each time.
        activation = Some(EditorActivation::Activate);
    }

    (group, activation)
}

/// `createModalEditorPart(options?.modal).then(part => part.activeGroup)`.
fn create_modal_group(editor_group_service: &Rc<dyn IEditorGroupsService>, options: Option<&TextEditorOptions>) -> MaybePromise<Rc<dyn IEditorGroup>> {
    MaybePromise::Promise(editor_group_service.create_modal_editor_part(options.and_then(|options| options.modal.clone())).then(|part| part.active_group()))
}

fn do_find_group(
    input: &FindGroupEditor,
    preferred_group: Option<&PreferredGroup>,
    editor_group_service: &Rc<dyn IEditorGroupsService>,
    configuration_service: &dyn IConfigurationService,
) -> MaybePromise<Rc<dyn IEditorGroup>> {
    let mut group: Option<MaybePromise<Rc<dyn IEditorGroup>>> = None;
    let editor = input.editor();
    let editor_input = input.editor_input();
    let options = input.options();
    let modal_editor_mode = use_modal_editor_mode(configuration_service);
    let preferred_number = preferred_group.and_then(PreferredGroup::as_number);

    // Group: Force modal if the editor has the RequiresModal capability,
    // but respect `workbench.editor.useModal: 'off'` as an explicit opt-out.
    if editor_input.as_ref().is_some_and(|editor_input| editor_input.has_capability(EditorInputCapabilities::REQUIRES_MODAL))
        && modal_editor_mode != Some(UseModalEditorMode::Off)
    {
        group = Some(create_modal_group(editor_group_service, options));
    }
    // Group: Instance of Group
    else if let Some(PreferredGroup::Group(preferred)) = preferred_group {
        group = Some(MaybePromise::Value(preferred.clone()));
    }
    // Group: Specific Group
    else if let Some(identifier) = preferred_number.filter(|identifier| *identifier >= 0) {
        group = editor_group_service.get_group(identifier).map(MaybePromise::Value);
    }
    // Group: Side by Side
    else if preferred_number == Some(SIDE_GROUP) {
        let direction = preferred_side_by_side_group_direction(configuration_service);

        let mut candidate_group = editor_group_service.find_group(&FindGroupScope { direction: Some(direction), location: None });
        if candidate_group.as_ref().is_none_or(|candidate_group| is_group_locked_for_editor(candidate_group, &editor)) {
            // Create new group either when the candidate group
            // is locked or was not found in the direction
            candidate_group = Some(editor_group_service.add_group(Some(GroupRef::Group(editor_group_service.active_group())), direction));
        }

        group = candidate_group.map(MaybePromise::Value);
    }
    // Group: Aux Window
    else if preferred_number == Some(AUX_WINDOW_GROUP) {
        group = Some(MaybePromise::Promise(
            editor_group_service.create_auxiliary_editor_part(options.and_then(|options| options.auxiliary)).then(|part| part.active_group()),
        ));
    }
    // Group: Modal (gated behind a setting)
    else if preferred_number == Some(MODAL_GROUP) && modal_editor_mode != Some(UseModalEditorMode::Off) {
        group = Some(create_modal_group(editor_group_service, options));
    }
    // Group: Unspecified without a specific index to open
    else if options.is_none_or(|options| options.index.is_none()) {
        let groups_by_last_active = editor_group_service.get_groups(GroupsOrder::MostRecentlyActive);

        // Respect option to reveal an editor if it is already visible in any group
        if options.is_some_and(|options| options.reveal_if_visible == Some(true)) {
            for last_active_group in &groups_by_last_active {
                if is_active(last_active_group, &editor) {
                    group = Some(MaybePromise::Value(last_active_group.clone()));
                    break;
                }
            }
        }

        // Respect option to reveal an editor if it is open (not necessarily visible)
        // Still prefer to reveal an editor in a group where the editor is active though.
        // We also try to reveal an editor if it has the `ForceReveal` or `Singleton`
        // capability which indicates that editor prefers to be revealed.
        if group.is_none()
            && (options.is_some_and(|options| options.reveal_if_opened == Some(true))
                || configuration_service.get_value(Some("workbench.editor.revealIfOpen"), None).is_truthy()
                || editor_input.as_ref().is_some_and(|editor_input| {
                    editor_input.has_capability(EditorInputCapabilities::FORCE_REVEAL) || editor_input.has_capability(EditorInputCapabilities::SINGLETON)
                }))
        {
            let mut group_with_input_active: Option<Rc<dyn IEditorGroup>> = None;
            let mut group_with_input_opened: Option<Rc<dyn IEditorGroup>> = None;

            for group in &groups_by_last_active {
                if is_opened(group, &editor) {
                    if group_with_input_opened.is_none() {
                        group_with_input_opened = Some(group.clone());
                    }

                    if group_with_input_active.is_none() && group.is_active(&editor) {
                        group_with_input_active = Some(group.clone());
                    }
                }

                if group_with_input_opened.is_some() && group_with_input_active.is_some() {
                    break; // we found all groups we wanted
                }
            }

            // Prefer a target group where the input is visible
            group = group_with_input_active.or(group_with_input_opened).map(MaybePromise::Value);
        }
    }

    // Force modal editor part: redirect to the modal group when setting is 'on'
    if group.is_none() && modal_editor_mode == Some(UseModalEditorMode::All) {
        group = Some(create_modal_group(editor_group_service, options));
    }

    // Fallback to active group if target not valid but avoid
    // locked editor groups unless editor is already opened there
    group.unwrap_or_else(|| {
        let mut candidate_group = editor_group_service.active_group();

        // Locked group: find the next non-locked group
        // going up the neigbours of the group or create
        // a new group otherwise
        if is_group_locked_for_editor(&candidate_group, &editor) {
            for group in editor_group_service.get_groups(GroupsOrder::MostRecentlyActive) {
                if is_group_locked_for_editor(&group, &editor) {
                    continue;
                }

                candidate_group = group;
                break;
            }

            if is_group_locked_for_editor(&candidate_group, &editor) {
                // Group is still locked, so we have to create a new
                // group to the side of the candidate group
                MaybePromise::Value(editor_group_service.add_group(Some(GroupRef::Group(candidate_group)), preferred_side_by_side_group_direction(configuration_service)))
            } else {
                MaybePromise::Value(candidate_group)
            }
        }
        // Non-locked group: take as is
        else {
            MaybePromise::Value(candidate_group)
        }
    })
}

fn is_group_locked_for_editor(group: &Rc<dyn IEditorGroup>, editor: &EditorInputOrUntyped) -> bool {
    if !group.is_locked() {
        // only relevant for locked editor groups
        return false;
    }

    if is_opened(group, editor) {
        // special case: the locked group contains
        // the provided editor. in that case we do not want
        // to open the editor in any different group.
        return false;
    }

    // group is locked for this editor
    true
}

fn is_active(group: &Rc<dyn IEditorGroup>, editor: &EditorInputOrUntyped) -> bool {
    match group.active_editor() {
        Some(active_editor) => active_editor.matches(editor),
        None => false,
    }
}

fn is_opened(group: &Rc<dyn IEditorGroup>, editor: &EditorInputOrUntyped) -> bool {
    group.editors().iter().any(|typed_editor| typed_editor.matches(editor))
}
