//! Porte de `src/vs/workbench/browser/parts/editor/diffEditorCommandsService.ts`: o serviço que sustenta os comandos do editor
//! de diff (alternar inline e lado a lado, o modo de visão, ir para a próxima mudança, focar um lado, ignorar espaços nas
//! pontas e trocar os lados).
//!
//! Divergências:
//! - **`args: unknown[]`.** O serviço só lê `args[0] instanceof URI`, e o argumento é o [`CommandArg`] do contexto de comandos de
//!   editor ([`CommandArg::Uri`]).
//! - **`instanceof TextDiffEditor`.** É o `downcast_ref` de [`IEditorPane::as_any`] para o [`TextDiffEditor`]
//!   (`text_diff_editor`). O controle (`getControl()`) é [`IEditorPane::get_diff_editor_control`], que também cobre o painel do
//!   editor de diff de vários arquivos (o `isDiffEditor(pane.getControl())` do original).
//! - **`DiffEditorViewMode`** (`'inline' | 'sideBySide' | 'automatic'`, de `editorOptions.ts`) é o enum [`DiffEditorViewMode`].
//! - **`async`.** Os métodos `async` são [`async_fn`]: o corpo roda já, de forma síncrona, até o primeiro `await`, como no
//!   JavaScript. `Promise.all([a, b])` chama `a` e `b` na ordem e espera os dois ([`Promise::all`]).
//! - **`IDiffEditorWidthBasedLayoutReset`.** O teste por forma do original é [`IEditorPane::as_diff_editor_width_based_layout_reset`].
//! - **Injeção.** O serviço é criado com os três serviços de que depende (`IEditorService`, `ITextResourceConfigurationService` e
//!   `IContextKeyService`); o `IDiffEditorCommandsService` é o contrato que a janela de agentes sobrescreve no original.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::text_resource_configuration_service::ITextResourceConfigurationService;
use je_instantiation::service_id;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::resources::ext_uri;

use crate::browser::parts::editor::editor_commands_context::CommandArg;
use crate::browser::parts::editor::text_diff_editor::{GoToDiffTarget, IDiffEditor, TextDiffEditor};
use crate::common::contextkeys::active_custom_editor_diff_can_toggle_layout_context;
use crate::common::editor::diff_editor_input::DiffEditorInput;
use crate::common::editor::editor_input::UntypedEditorOptions;
use crate::common::editor::{EditorInputOrUntyped, EditorInputRef, EditorResourceAccessor, EditorResourceAccessorOptions, IEditorPane, SideBySideEditor};
use crate::services::editor::editor_groups_service::{GroupRef, PreferredGroup};
use crate::services::editor::editor_service::{ActiveTextEditorControl, EditorReplacementInput, IEditorService, ResourceEditorInputIdentifier, UntypedEditorReplacement};

/// `DiffEditorViewMode`: o layout do editor de diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffEditorViewMode {
    Inline,
    SideBySide,
    Automatic,
}

/// `FocusTextDiffEditorMode`: which side of the active diff editor to focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTextDiffEditorMode {
    Original,
    Modified,
    Toggle,
}

/// `IDiffEditorCommandsService`: backs the diff-editor commands. The Agents window overrides this to also drive its
/// multi-diff Changes editor.
pub trait IDiffEditorCommandsService {
    /// Toggles inline vs. side-by-side rendering for the active diff editor.
    fn toggle_render_side_by_side(&self, args: &[CommandArg]) -> Promise<()>;

    /// Sets the layout mode for the active diff editor.
    fn set_view_mode(&self, args: &[CommandArg], mode: DiffEditorViewMode) -> Promise<()>;

    /// Opens the original or modified side of the active diff editor, whichever has focus, as its own editor.
    fn open_active_diff_side(&self) -> Promise<()>;

    /// Navigates to the next or previous change in the active diff editor.
    fn navigate_in_diff_editor(&self, args: &[CommandArg], next: bool);

    /// Focuses the original, modified, or currently unfocused side of the active diff editor.
    fn focus_in_diff_editor(&self, args: &[CommandArg], mode: FocusTextDiffEditorMode);

    /// Toggles whether the active diff editor ignores trim whitespace.
    fn toggle_diff_ignore_trim_whitespace(&self, args: &[CommandArg]) -> Promise<()>;

    /// Swaps the original and modified sides of the active diff editor.
    fn swap_diff_sides(&self, args: &[CommandArg]) -> Promise<()>;
}

service_id!(pub I_DIFF_EDITOR_COMMANDS_SERVICE: dyn IDiffEditorCommandsService = "diffEditorCommandsService");

/// O painel e o controle do editor de diff ativo (`{ pane, control }`).
struct ActiveDiffEditor {
    pane: Rc<dyn IEditorPane>,
    control: Rc<dyn IDiffEditor>,
}

/// `args.length > 0 && args[0] instanceof URI ? args[0] : undefined`.
fn resource_arg(args: &[CommandArg]) -> Option<&URI> {
    match args.first() {
        Some(CommandArg::Uri(uri)) => Some(uri),
        _ => None,
    }
}

/// `isEqual(a, b)`.
fn is_same_resource(a: Option<&URI>, b: &URI) -> bool {
    ext_uri().is_equal(a, Some(b), false)
}

/// `editor instanceof TextDiffEditor`.
fn as_text_diff_editor(pane: &Rc<dyn IEditorPane>) -> Option<&TextDiffEditor> {
    pane.as_any().downcast_ref::<TextDiffEditor>()
}

/// `{ supportSideBySide: SideBySideEditor.PRIMARY }`.
fn primary_side() -> EditorResourceAccessorOptions {
    EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Primary), filter_by_scheme: None }
}

/// `textResourceConfigurationService.updateValue(resource, key, value)`.
fn update_setting(service: &dyn ITextResourceConfigurationService, resource: &URI, key: &str, value: bool) -> Promise<()> {
    service.update_value(Some(resource), key, Dynamic::Bool(value), None)
}

pub struct DiffEditorCommandsService {
    pub(crate) editor_service: Rc<dyn IEditorService>,
    text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
    context_key_service: Rc<dyn IContextKeyService>,
}

impl DiffEditorCommandsService {
    pub fn new(
        editor_service: Rc<dyn IEditorService>,
        text_resource_configuration_service: Rc<dyn ITextResourceConfigurationService>,
        context_key_service: Rc<dyn IContextKeyService>,
    ) -> Self {
        Self { editor_service, text_resource_configuration_service, context_key_service }
    }

    /// `const value = getValue(resource, key); await updateValue(resource, key, !value)`.
    fn toggle_setting(&self, resource: &URI, key: &str) -> Promise<()> {
        let value = self.text_resource_configuration_service.get_value(Some(resource), Some(key));
        update_setting(&*self.text_resource_configuration_service, resource, key, !value.is_truthy())
    }

    /// `[this.editorService.activeEditorPane, ...this.editorService.visibleEditorPanes]`.
    fn candidate_panes(&self) -> Vec<Rc<dyn IEditorPane>> {
        let mut panes: Vec<Rc<dyn IEditorPane>> = self.editor_service.active_editor_pane().into_iter().collect();
        panes.extend(self.editor_service.visible_editor_panes());
        panes
    }

    fn get_active_text_diff_editor(&self, args: &[CommandArg]) -> Option<Rc<dyn IEditorPane>> {
        let resource = resource_arg(args);

        self.candidate_panes().into_iter().find(|editor| {
            as_text_diff_editor(editor).is_some()
                && resource.is_none_or(|resource| {
                    editor.input().is_some_and(|input| {
                        input.as_any().downcast_ref::<DiffEditorInput>().is_some_and(|diff_input| is_same_resource(diff_input.modified().resource().as_ref(), resource))
                    })
                })
        })
    }

    fn get_active_diff_editor(&self, args: &[CommandArg]) -> Option<ActiveDiffEditor> {
        if let Some(pane) = self.get_active_text_diff_editor(args)
            && let Some(control) = pane.get_diff_editor_control()
        {
            return Some(ActiveDiffEditor { pane, control });
        }

        let resource = resource_arg(args);
        for pane in self.candidate_panes() {
            let Some(control) = pane.get_diff_editor_control() else {
                continue;
            };

            let modified_resource = control.get_modified_editor().get_model_uri();
            let input_resource = pane
                .input()
                .and_then(|input| EditorResourceAccessor::get_canonical_uri(Some(&EditorInputOrUntyped::Typed(input)), Some(&primary_side())));
            let matches = resource.is_none_or(|resource| {
                modified_resource.as_ref().is_some_and(|modified_resource| is_same_resource(Some(modified_resource), resource))
                    || input_resource.as_ref().is_some_and(|input_resource| is_same_resource(Some(input_resource), resource))
            });
            if matches {
                return Some(ActiveDiffEditor { pane, control });
            }
        }

        None
    }

    fn get_active_diff_modified_resource(&self, args: &[CommandArg]) -> Option<URI> {
        if let Some(model_uri) = self.get_active_diff_editor(args).and_then(|active| active.control.get_modified_editor().get_model_uri()) {
            return Some(model_uri);
        }

        let resource = resource_arg(args);
        if active_custom_editor_diff_can_toggle_layout_context().get_value(&*self.context_key_service).is_truthy() {
            let active_custom_diff_modified_resource =
                EditorResourceAccessor::get_original_uri(self.editor_service.active_editor().as_ref(), Some(&primary_side()));
            if let Some(active_custom_diff_modified_resource) = active_custom_diff_modified_resource
                && resource.is_none_or(|resource| is_same_resource(Some(&active_custom_diff_modified_resource), resource))
            {
                return Some(active_custom_diff_modified_resource);
            }
        }

        let mut editors: Vec<EditorInputRef> = Vec::new();
        if let Some(EditorInputOrUntyped::Typed(active_editor)) = self.editor_service.active_editor() {
            editors.push(active_editor);
        }
        editors.extend(self.editor_service.visible_editors());
        for editor in editors {
            // `isDiffEditorInput(editor) && editor.modified.resource && (!resource || isEqual(editor.modified.resource, resource))`
            let modified_resource = editor.diff_editors().and_then(|(modified, _original)| modified.resource());
            if let Some(modified_resource) = modified_resource
                && resource.is_none_or(|resource| is_same_resource(Some(&modified_resource), resource))
            {
                return Some(modified_resource);
            }
        }

        None
    }
}

impl IDiffEditorCommandsService for DiffEditorCommandsService {
    fn toggle_render_side_by_side(&self, args: &[CommandArg]) -> Promise<()> {
        let Some(modified_resource) = self.get_active_diff_modified_resource(args) else {
            return Promise::resolved(());
        };

        self.toggle_setting(&modified_resource, "diffEditor.renderSideBySide")
    }

    fn set_view_mode(&self, args: &[CommandArg], mode: DiffEditorViewMode) -> Promise<()> {
        let Some(active_diff_editor) = self.get_active_diff_editor(args) else {
            return Promise::resolved(());
        };
        let Some(modified_resource) = active_diff_editor.control.get_modified_editor().get_model_uri() else {
            return Promise::resolved(());
        };

        let service = self.text_resource_configuration_service.clone();
        async_fn(async move {
            let update = |key: &str, value: bool| update_setting(&*service, &modified_resource, key, value);

            match mode {
                DiffEditorViewMode::Inline => {
                    update("diffEditor.renderSideBySide", false).await?;
                }
                DiffEditorViewMode::SideBySide => {
                    Promise::all(vec![update("diffEditor.renderSideBySide", true), update("diffEditor.useInlineViewWhenSpaceIsLimited", false)]).await?;
                }
                DiffEditorViewMode::Automatic => {
                    Promise::all(vec![update("diffEditor.renderSideBySide", true), update("diffEditor.useInlineViewWhenSpaceIsLimited", true)]).await?;
                    match active_diff_editor.pane.as_diff_editor_width_based_layout_reset() {
                        Some(pane) => pane.reset_diff_editor_width_based_layout(),
                        None => active_diff_editor.control.reset_width_based_layout(),
                    }
                }
            }

            Ok(())
        })
    }

    fn open_active_diff_side(&self) -> Promise<()> {
        let active_editor = self.editor_service.active_editor();
        let active_text_editor_control = self.editor_service.active_text_editor_control();
        let (Some(ActiveTextEditorControl::Diff { original, .. }), Some(EditorInputOrUntyped::Typed(active_editor))) = (active_text_editor_control, active_editor) else {
            return Promise::resolved(());
        };
        let Some(diff_input) = active_editor.as_any().downcast_ref::<DiffEditorInput>() else {
            return Promise::resolved(());
        };

        let editor = if original.has_text_focus() { diff_input.original() } else { diff_input.modified() };

        let editor_service = self.editor_service.clone();
        async_fn(async move {
            editor_service.open_editor_typed(&editor, None, None).await?;

            Ok(())
        })
    }

    fn navigate_in_diff_editor(&self, args: &[CommandArg], next: bool) {
        let active_text_diff_editor = self.get_active_text_diff_editor(args);

        if let Some(control) = active_text_diff_editor.and_then(|editor| editor.get_diff_editor_control()) {
            control.go_to_diff(if next { GoToDiffTarget::Next } else { GoToDiffTarget::Previous });
        }
    }

    fn focus_in_diff_editor(&self, args: &[CommandArg], mode: FocusTextDiffEditorMode) {
        let Some(active_text_diff_editor) = self.get_active_text_diff_editor(args) else {
            return;
        };
        let control = active_text_diff_editor.get_diff_editor_control();

        match mode {
            FocusTextDiffEditorMode::Original => {
                if let Some(control) = control {
                    control.get_original_editor().focus();
                }
            }
            FocusTextDiffEditorMode::Modified => {
                if let Some(control) = control {
                    control.get_modified_editor().focus();
                }
            }
            FocusTextDiffEditorMode::Toggle => {
                let modified_has_focus = control.is_some_and(|control| control.get_modified_editor().has_widget_focus());
                self.focus_in_diff_editor(args, if modified_has_focus { FocusTextDiffEditorMode::Original } else { FocusTextDiffEditorMode::Modified });
            }
        }
    }

    fn toggle_diff_ignore_trim_whitespace(&self, args: &[CommandArg]) -> Promise<()> {
        let active_text_diff_editor = self.get_active_text_diff_editor(args);

        let model_uri = active_text_diff_editor
            .and_then(|editor| editor.get_diff_editor_control())
            .and_then(|control| control.get_modified_editor().get_model_uri());
        let Some(model_uri) = model_uri else {
            return Promise::resolved(());
        };

        self.toggle_setting(&model_uri, "diffEditor.ignoreTrimWhitespace")
    }

    fn swap_diff_sides(&self, args: &[CommandArg]) -> Promise<()> {
        let Some(diff_editor) = self.get_active_text_diff_editor(args) else {
            return Promise::resolved(());
        };
        let Some(active_group) = as_text_diff_editor(&diff_editor).and_then(TextDiffEditor::group) else {
            return Promise::resolved(());
        };
        let Some(diff_editor_input) = diff_editor.input() else {
            return Promise::resolved(());
        };
        let Some(modified) = diff_editor_input.as_any().downcast_ref::<DiffEditorInput>().map(|diff_input| diff_input.modified()) else {
            return Promise::resolved(());
        };
        let Some(modified_resource) = modified.resource() else {
            return Promise::resolved(());
        };

        let untyped_options = UntypedEditorOptions { preserve_view_state: Some(active_group), preserve_resource: Some(true) };
        let Some(untyped_diff_input) = diff_editor_input.to_untyped(Some(&untyped_options)) else {
            return Promise::resolved(());
        };

        let editor_service = self.editor_service.clone();
        async_fn(async move {
            // Since we are about to replace the diff editor, make
            // sure to first open the modified side if it is not
            // yet opened. This ensures that the swapping is not
            // bringing up a confirmation dialog to save.
            let identifier = ResourceEditorInputIdentifier { type_id: modified.editor_type_id(), editor_id: modified.editor_id(), resource: modified_resource };
            if modified.is_modified() && editor_service.find_editors_by_identifier(&identifier, None).is_empty() {
                let mut editor_to_open = untyped_diff_input.modified.as_deref().cloned().unwrap_or_default();
                let mut options = editor_to_open.options.take().unwrap_or_default();
                options.base.pinned = Some(true);
                options.base.inactive = Some(true);
                editor_to_open.options = Some(options);

                editor_service.open_editor_in(editor_to_open, Some(PreferredGroup::Id(active_group))).await?;
            }

            // Replace the input with the swapped variant
            let mut options = untyped_diff_input.options.clone().unwrap_or_default();
            options.base.pinned = Some(true);
            let replacement = crate::common::editor::UntypedEditorInput {
                original: untyped_diff_input.modified.clone(),
                modified: untyped_diff_input.original.clone(),
                options: Some(options),
                ..untyped_diff_input
            };
            editor_service
                .replace_editors(
                    vec![EditorReplacementInput::Untyped(UntypedEditorReplacement { editor: diff_editor_input, replacement, force_replace_dirty: None })],
                    GroupRef::Id(active_group),
                )
                .await?;

            Ok(())
        })
    }
}
