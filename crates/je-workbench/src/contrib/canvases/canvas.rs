//! Porte de `src/vs/workbench/contrib/canvases/common/canvas.ts`: o modelo de dados dos canvases, as funções puras de
//! rótulo e identidade, e a entrada de editor `CanvasInput`.
//!
//! Divergências:
//! - **Fora do porte** (e por quê): `ICanvasDefinitionProvider`, `ICanvasContext`, `ICanvasContextService`,
//!   `ICanvasWorkingSets`, `ICanvasService` e `ICanvasModelResolution`, as interfaces de serviço que dependem de
//!   `IBrowserViewModel` (`contrib/browserView/common/browserView.ts`, ainda não portado) e do `IEditorGroup` completo.
//!   Entram com o contrato de que dependem. As interfaces de dados e as funções puras entram inteiras.
//! - **`canvas`** (`observableValue<ICanvas | undefined>`) guarda `Option<Rc<ICanvas>>`: o `===` do observável compara
//!   por identidade, como o objeto do JavaScript. `membershipPending` é um observável de `bool`.
//! - **Herança.** `CanvasInput extends EditorInput` é a struct que guarda o [`EditorInputBase`] e implementa o trait
//!   [`EditorInput`] (ver `common/editor/editor_input.rs`).

use std::any::Any;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

use je_base::collator::locale_compare;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::hash::hash;
use je_base::js_string::JsString;
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{Observable, SettableExt, observable_value, transaction};
use je_base::themables::ThemeIcon;
use je_base::uri::{URI, UriComponents, encode_uri_component};
use je_platform::resources::ext_uri;

use crate::common::editor::editor_input::{CanMove, EditorInput, EditorInputBase, EditorInputIcon, matches_by_default};
use crate::common::editor::{EditorInputCapabilities, EditorInputOrUntyped, GroupIdentifier, Verbosity};
use crate::services::editor::editor_groups_service::IEditorGroupsService;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

/// Reveals a live canvas beside its owning chat, reopening its closed tab. Takes an [`ICanvasReference`].
pub const REVEAL_CANVAS_COMMAND_ID: &str = "workbench.action.agentSessions.revealCanvas";

/// `ICanvas`.
#[derive(Clone, Debug)]
pub struct ICanvas {
    pub resource: URI,
    pub instance_id: Option<String>,
    pub title: String,
    pub status: Option<String>,
    pub source: Option<URI>,
}

/// Where a registered canvas definition was contributed from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasExtensionSource {
    User,
    Project,
    Session,
    Plugin,
    Unknown,
}

/// One canvas type registered for a live chat.
#[derive(Clone, Debug)]
pub struct ICanvasDefinition {
    pub canvas_id: String,
    pub extension_id: String,
    pub extension_source: CanvasExtensionSource,
    pub extension_name: Option<String>,
    pub display_name: String,
    pub description: String,
    pub requires_input: bool,
}

/// `ICanvasOwner`.
#[derive(Clone, Debug)]
pub struct ICanvasOwner {
    pub provider_id: String,
    pub session: URI,
    pub chat: URI,
}

/// `ICanvasReference`: o `ICanvasOwner` mais o recurso do canvas.
#[derive(Clone, Debug)]
pub struct ICanvasReference {
    pub owner: ICanvasOwner,
    pub canvas: URI,
}

/// `ICanvasReopenTarget`.
#[derive(Clone, Debug)]
pub struct ICanvasReopenTarget {
    pub reference: ICanvasReference,
    pub canvas: ICanvas,
}

/// `ICanvasOpenRequest`.
#[derive(Clone, Debug)]
pub struct ICanvasOpenRequest {
    pub id: String,
    pub succeeded: bool,
}

/// The owner of an editor working set: a whole session, or a single chat of it when the window scopes its layout to
/// chats.
#[derive(Clone, Debug)]
pub struct ICanvasWorkingSetOwner {
    pub session_resource: URI,
    /// The owning chat, or `None` when the working set belongs to the whole session.
    pub chat_resource: Option<URI>,
}

/// `ICanvasWorkingSetState`.
#[derive(Clone, Debug)]
pub struct ICanvasWorkingSetState {
    /// The owner whose working set was restored most recently, or `None` for an empty working set.
    pub owner: Option<ICanvasWorkingSetOwner>,
    /// Whether a restore that replaces every open editor is queued or running.
    pub restoring: bool,
    /// Whether the queued restore is currently replacing the editor working set.
    pub applying: bool,
}

/// `canvasOwnerKey(owner)`.
pub fn canvas_owner_key(owner: &ICanvasOwner) -> String {
    format!("{}\u{0}{}\u{0}{}", owner.provider_id, owner.session.to_string(false), owner.chat.to_string(false))
}

/// `getCanvasReferenceKey(reference)`.
pub fn get_canvas_reference_key(reference: &ICanvasReference) -> String {
    format!("{}\u{0}{}", canvas_owner_key(&reference.owner), reference.canvas.to_string(false))
}

/// `isCanvasOwner(first, second)`.
pub fn is_canvas_owner(first: &ICanvasOwner, second: &ICanvasOwner) -> bool {
    first.provider_id == second.provider_id
        && ext_uri().is_equal(Some(&first.session), Some(&second.session), false)
        && ext_uri().is_equal(Some(&first.chat), Some(&second.chat), false)
}

fn increment(counts: &mut std::collections::HashMap<String, usize>, key: &str) {
    *counts.entry(key.to_string()).or_insert(0) += 1;
}

/// Labels canvases by title, disambiguating repeated titles by their instance identifiers or positions.
pub fn get_canvas_instance_labels(canvases: &[ICanvas]) -> Vec<String> {
    let mut title_counts = std::collections::HashMap::new();
    let mut instance_id_counts = std::collections::HashMap::new();
    for canvas in canvases {
        increment(&mut title_counts, &canvas.title);
        if let Some(instance_id) = canvas.instance_id.as_deref().filter(|id| !id.is_empty()) {
            increment(&mut instance_id_counts, instance_id);
        }
    }

    let mut labels = vec![String::new(); canvases.len()];
    let mut used_labels = std::collections::HashSet::new();
    for (index, canvas) in canvases.iter().enumerate() {
        if title_counts.get(&canvas.title) == Some(&1) {
            labels[index] = canvas.title.clone();
            used_labels.insert(canvas.title.clone());
        }
    }

    let mut title_indexes = std::collections::HashMap::new();
    for (index, canvas) in canvases.iter().enumerate() {
        if title_counts.get(&canvas.title) == Some(&1) {
            continue;
        }

        let title_index = title_indexes.get(&canvas.title).copied().unwrap_or(0) + 1;
        title_indexes.insert(canvas.title.clone(), title_index);
        let instance_label = match canvas.instance_id.as_deref().filter(|id| !id.is_empty()) {
            Some(instance_id) if instance_id_counts.get(instance_id) == Some(&1) => instance_id.to_string(),
            _ => title_index.to_string(),
        };
        let mut label = localize("canvas.instanceTitle", "{0} ({1})", &[canvas.title.as_str(), instance_label.as_str()]);
        let mut collision_index = 2;
        while used_labels.contains(&label) {
            label = localize(
                "canvas.instanceTitleCollision",
                "{0} ({1}, {2})",
                &[canvas.title.as_str(), instance_label.as_str(), collision_index.to_string().as_str()],
            );
            collision_index += 1;
        }
        used_labels.insert(label.clone());
        labels[index] = label;
    }
    labels
}

/// `canvas.displayName || canvas.canvasId`.
fn definition_display_name(canvas: &ICanvasDefinition) -> &str {
    if canvas.display_name.is_empty() { &canvas.canvas_id } else { &canvas.display_name }
}

/// `canvas.extensionName || canvas.extensionId`.
fn definition_extension_label(canvas: &ICanvasDefinition) -> &str {
    canvas.extension_name.as_deref().filter(|name| !name.is_empty()).unwrap_or(&canvas.extension_id)
}

/// Labels registered canvas types while disambiguating extension and identifier collisions.
pub fn get_canvas_definition_labels(canvases: &[ICanvasDefinition]) -> Vec<String> {
    let mut display_name_counts = std::collections::HashMap::new();
    for canvas in canvases {
        increment(&mut display_name_counts, definition_display_name(canvas));
    }

    let mut labels = Vec::new();
    let mut used_labels = std::collections::HashSet::new();
    for canvas in canvases {
        let display_name = definition_display_name(canvas);
        let mut label = if display_name_counts.get(display_name) == Some(&1) {
            display_name.to_string()
        } else {
            localize("canvas.definitionTitle", "{0} ({1})", &[display_name, definition_extension_label(canvas)])
        };
        if used_labels.contains(&label) {
            label = localize(
                "canvas.definitionTitleWithId",
                "{0} ({1}, {2})",
                &[display_name, definition_extension_label(canvas), canvas.canvas_id.as_str()],
            );
        }
        let mut collision_index = 2;
        let base_label = label.clone();
        while used_labels.contains(&label) {
            label = localize("canvas.definitionTitleCollision", "{0} ({1})", &[base_label.as_str(), collision_index.to_string().as_str()]);
            collision_index += 1;
        }
        used_labels.insert(label.clone());
        labels.push(label);
    }
    labels
}

const CANVAS_INSTANCE_ID_MAX_LENGTH: usize = 128;

fn is_separator(character: char) -> bool {
    matches!(character, '.' | '_' | '-')
}

/// Produces the stable instance identifier used when directly opening a registered canvas type.
pub fn get_canvas_definition_instance_id(canvas: &ICanvasDefinition) -> String {
    let raw = format!("{}-{}", canvas.extension_id, canvas.canvas_id);

    // `.replace(/[^A-Za-z0-9._-]+/g, '-')`: cada sequência de caracteres fora do conjunto vira um único hífen.
    let mut replaced = String::new();
    let mut in_invalid_run = false;
    for character in raw.chars() {
        if character.is_ascii_alphanumeric() || is_separator(character) {
            replaced.push(character);
            in_invalid_run = false;
        } else if !in_invalid_run {
            replaced.push('-');
            in_invalid_run = true;
        }
    }
    // `.replace(/^[._-]+/, '').replace(/[._-]+$/, '')`.
    let normalized = replaced.trim_start_matches(is_separator).trim_end_matches(is_separator).to_string();
    if normalized == raw && normalized.len() <= CANVAS_INSTANCE_ID_MAX_LENGTH {
        return normalized;
    }

    // `(hash(raw) >>> 0).toString(16).padStart(8, '0')`.
    let hash_suffix = format!("-{:08x}", hash(&Dynamic::string(&raw)) as u32);
    // O `normalized` é só ASCII, então o `slice` do original (unidades UTF-16) é o corte por bytes.
    let cut = (CANVAS_INSTANCE_ID_MAX_LENGTH - hash_suffix.len()).min(normalized.len());
    let base = normalized[..cut].trim_end_matches(is_separator);
    let base = if base.is_empty() { "canvas" } else { base };
    format!("{base}{hash_suffix}")
}

/// Returns whether a registered extension canvas can be opened without another input flow.
pub fn is_openable_extension_canvas_definition(canvas: &ICanvasDefinition) -> bool {
    !canvas.requires_input
        && (canvas.extension_source != CanvasExtensionSource::Unknown
            || canvas.extension_id.starts_with("user:")
            || canvas.extension_id.starts_with("project:")
            || canvas.extension_id.starts_with("session:")
            || canvas.extension_id.starts_with("plugin:"))
}

/// Orders registered canvas types by extension and display name.
pub fn compare_canvas_definitions(first: &ICanvasDefinition, second: &ICanvasDefinition) -> Ordering {
    locale_compare(definition_extension_label(first), definition_extension_label(second))
        .then_with(|| locale_compare(definition_display_name(first), definition_display_name(second)))
        .then_with(|| locale_compare(&first.canvas_id, &second.canvas_id))
}

fn create_input_resource(reference: &ICanvasReference) -> URI {
    let query = Dynamic::object([
        ("providerId", Dynamic::string(&reference.owner.provider_id)),
        ("session", Dynamic::string(&reference.owner.session.to_string(false).to_string_lossy())),
        ("chat", Dynamic::string(&reference.owner.chat.to_string(false).to_string_lossy())),
        ("canvas", Dynamic::string(&reference.canvas.to_string(false).to_string_lossy())),
    ]);
    let json = json_stringify(&query).expect("o objeto sempre serializa");
    let json_units: Vec<u16> = json.encode_utf16().collect();
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from("session-canvas")),
            path: Some(JsString::from("/canvas")),
            query: Some(JsString::from_units(encode_uri_component(&json_units))),
            ..UriComponents::default()
        },
        false,
    )
}

/// `CanvasInput`.
pub struct CanvasInput {
    base: EditorInputBase,
    pub reference: ICanvasReference,
    resource: URI,
    pub canvas: Rc<ObservableValue<Option<Rc<ICanvas>>>>,
    pub membership_pending: Rc<ObservableValue<bool>>,
    serialization_id: RefCell<Option<String>>,
    editor_groups_service: Rc<dyn IEditorGroupsService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
}

impl CanvasInput {
    pub const ID: &'static str = "sessions.editorInput.canvas";
    pub const EDITOR_ID: &'static str = "sessions.editor.canvas";

    pub fn new(
        reference: ICanvasReference,
        canvas: Option<ICanvas>,
        editor_groups_service: Rc<dyn IEditorGroupsService>,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| {
            let canvas_value = observable_value::<Option<Rc<ICanvas>>, ()>("canvas", None);
            canvas_value.set(canvas.map(Rc::new), None);
            Self {
                base: EditorInputBase::new(this.clone()),
                resource: create_input_resource(&reference),
                reference,
                canvas: canvas_value,
                membership_pending: observable_value::<bool, ()>("membershipPending", false),
                serialization_id: RefCell::new(None),
                editor_groups_service,
                environment_service,
            }
        })
    }

    /// `serializationId`.
    pub fn serialization_id(&self) -> Option<String> {
        self.serialization_id.borrow().clone()
    }

    /// `setSerializationId(serializationId)`.
    pub fn set_serialization_id(&self, serialization_id: &str) {
        *self.serialization_id.borrow_mut() = Some(serialization_id.to_string());
    }

    /// `setCanvas(canvas, membershipPending = false)`.
    pub fn set_canvas(&self, canvas: Option<ICanvas>, membership_pending: bool) {
        let previous_title = self.canvas.get().map(|previous| previous.title.clone());
        let new_title = canvas.as_ref().map(|canvas| canvas.title.clone());
        transaction(|tx| {
            self.canvas.set(canvas.map(Rc::new), Some(tx));
            self.membership_pending.set(membership_pending, Some(tx));
        });
        if previous_title != new_title {
            self.base.fire_did_change_label();
        }
    }
}

impl IDisposable for CanvasInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for CanvasInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        Self::ID.to_string()
    }

    fn resource(&self) -> Option<URI> {
        Some(self.resource.clone())
    }

    fn editor_id(&self) -> Option<String> {
        Some(Self::EDITOR_ID.to_string())
    }

    fn capabilities(&self) -> EditorInputCapabilities {
        EditorInputCapabilities::READONLY | EditorInputCapabilities::SINGLETON | EditorInputCapabilities::FORCE_REVEAL
    }

    fn get_name(&self) -> String {
        match self.canvas.get() {
            Some(canvas) => canvas.title.clone(),
            None => localize("canvas.editorName", "Canvas", &[]),
        }
    }

    fn get_description(&self, _verbosity: Option<Verbosity>) -> Option<String> {
        // Only the Agents window offers an Add Tab menu.
        Some(if self.environment_service.is_sessions_window() {
            localize(
                "canvas.editorDescription",
                "Closing this tab hides the canvas. Reopen it from the Canvas submenu in Add Tab, use its canvas pill, or ask the agent to open it again.",
                &[],
            )
        } else {
            localize(
                "canvas.editorDescriptionEditorWindow",
                "Closing this tab hides the canvas. Reopen it from its canvas pill or ask the agent to open it again.",
                &[],
            )
        })
    }

    fn get_icon(&self) -> Option<EditorInputIcon> {
        Some(EditorInputIcon::Theme(ThemeIcon::from_id("preview")))
    }

    fn get_title(&self, _verbosity: Option<Verbosity>) -> String {
        self.get_name()
    }

    fn can_reopen(&self) -> bool {
        false
    }

    fn can_move(&self, _source_group: GroupIdentifier, target_group: GroupIdentifier) -> CanMove {
        let target_window = self.editor_groups_service.get_group(target_group).map(|group| group.window_id());
        if target_window == Some(self.editor_groups_service.main_part().window_id()) {
            CanMove::Yes
        } else {
            CanMove::No(localize(
                "canvas.mainWindowOnly",
                "Canvases can only be shown in the main window beside their owning conversation.",
                &[],
            ))
        }
    }

    fn matches(&self, other: &EditorInputOrUntyped) -> bool {
        if let EditorInputOrUntyped::Typed(typed) = other
            && let Some(other_canvas) = typed.as_any().downcast_ref::<CanvasInput>()
        {
            return ext_uri().is_equal(Some(&self.resource), Some(&other_canvas.resource), false);
        }
        matches_by_default(self, other)
    }
}
