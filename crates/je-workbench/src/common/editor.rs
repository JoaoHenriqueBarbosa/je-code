//! Porte de `src/vs/workbench/common/editor.ts`: as capacidades e os tipos de dados do modelo de editores do workbench
//! (`EditorInputCapabilities`, `Verbosity`, `GroupModelChangeKind`, `SideBySideEditor`, os eventos de grupo, as entradas
//! sem tipo, o `EditorResourceAccessor`...), mais o `SaveReason`, o `SaveSource`, as opções de salvar e reverter
//! (`ISaveOptions`, `IRevertOptions`) e o `GroupIdentifier` que os serviços de working copy já liam.
//!
//! O `EditorInput` (classe abstrata) e os modelos de `common/editor/` ficam em submódulos deste arquivo, um por `.ts`:
//! [`editor_input`], [`editor_model`], [`side_by_side_editor_input`], [`diff_editor_input`], [`editor_group_model`] e
//! [`filtered_editor_group_model`]. O registro de serializadores (`IEditorSerializer` e `IEditorFactoryRegistry`, que o
//! original declara neste `.ts`) fica em [`editor_factory_registry`].
//!
//! Notas de tradução:
//! - **`EditorInput` é o trait [`EditorInput`](editor_input::EditorInput)** e o handle compartilhado é
//!   [`EditorInputRef`] (`Rc<dyn EditorInput>`). A identidade (`===`) é a do `Rc` ([`je_base::lifecycle::id_of`]).
//! - **Tipagem estrutural.** O original distingue entradas por forma (`isEditorInput`, `isSideBySideEditorInput`,
//!   `isResourceDiffEditorInput`...) sobre `unknown`. As entradas sem tipo (`IUntypedEditorInput`, a união de
//!   `IResourceEditorInput`, `ITextResourceEditorInput`, `IUntitledTextResourceEditorInput`, `IResourceDiffEditorInput`,
//!   `IResourceMultiDiffEditorInput`, `IResourceSideBySideEditorInput` e `IResourceMergeEditorInput`) são uma struct só,
//!   [`UntypedEditorInput`], com todos os campos opcionais, e cada `isResource*` é a mesma checagem de campos do original.
//!   `EditorInput | IUntypedEditorInput` é [`EditorInputOrUntyped`]; um parâmetro `unknown` que nos testes recebe
//!   `undefined`, um `{}` ou um `EditorInput` é `Option<&EditorInputOrUntyped>`.
//! - **Tipos de `platform/editor/common/editor.ts`** que o workbench usa e que o porte de plataforma ainda não tem
//!   (`IBaseUntypedEditorInput`, `IResourceEditorInput`, `ITextEditorOptions`, `TextEditorSelectionRevealType`...) estão
//!   aqui: [`UntypedEditorInput`] e [`TextEditorOptions`] (que dá `Deref` para as [`EditorOptions`]).
//! - **Propriedades** viram métodos. `typeId` é `editor_type_id` (o `type_id` colidiria com o `Any::type_id`).
//! - **Sobrecargas** do `EditorResourceAccessor` (`URI` ou `{ primary?, secondary? }`) são [`EditorResourceAccessor::get_original_uri`]
//!   e [`EditorResourceAccessor::get_canonical_uri`] (só `URI`) e as variantes `_sides` (a união inteira, [`EditorUri`]).
//!
//! Fora deste módulo (e por quê): `IEditorPane` completo e `IEditorGroup` (dependem de `IComposite`, `IInstantiationService`
//! e do `IEditorGroupsService`), `IFileEditorFactory`, `IFileEditorInput`
//! (dependem do `IInstantiationService` e do `Registry` de plataforma), `pathsToEditors`, `createEditorOpenError`,
//! `createTooLargeFileError`, `IToolbarActions` (dependem de `IAction`), `IEditorMemento`, `IEditorPartConfiguration` e
//! `preventEditorClose` (entram com o `editorGroupModel`), `isEditorIdentifier`, `isEditorCommandsContext`,
//! `isEditorInputWithOptions` e `isEditorInputWithOptionsAndGroup` (checagens de forma sobre `unknown` que o sistema de
//! tipos do Rust resolve em tempo de compilação: as structs [`EditorIdentifier`] e [`EditorInputWithOptions`] já são o tipo).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::{BitAnd, BitOr, BitOrAssign, Deref, DerefMut, Not};
use std::rc::Rc;

use je_base::dynamic::{Dynamic, Truthy};
use je_base::lifecycle::id_of;
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::uri::URI;
use je_platform::opener::editor_types::{EditorOptions, EditorOverride, TextEditorSelection};

use crate::common::editor::editor_input::EditorInput;
use crate::services::editor::editor_service::{EditorControl, IEditorService};
use crate::services::files_configuration::files_configuration_service::{EditorOriginalUri, ResourceOrEditor};

pub mod binary_editor_model;
pub mod diff_editor_input;
pub mod diff_editor_model;
pub mod editor_factory_registry;
pub mod editor_group_model;
pub mod editor_input;
pub mod editor_model;
pub mod filtered_editor_group_model;
pub mod resource_editor_input;
pub mod side_by_side_editor_input;
pub mod text_editor_model;
pub mod text_resource_editor_input;
pub mod text_resource_editor_model;

/// Handle compartilhado de um `EditorInput`.
pub type EditorInputRef = Rc<dyn EditorInput>;

/// `EditorExtensions`: valores estáticos para as contribuições de editor.
pub struct EditorExtensions;

impl EditorExtensions {
    pub const EDITOR_PANE: &'static str = "workbench.contributions.editors";
    pub const EDITOR_FACTORY: &'static str = "workbench.contributions.editor.inputFactories";
}

/// O tipo de `DEFAULT_EDITOR_ASSOCIATION`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultEditorAssociation {
    pub id: &'static str,
    pub display_name: String,
    pub provider_display_name: String,
}

/// O `id` de `DEFAULT_EDITOR_ASSOCIATION`.
pub const DEFAULT_EDITOR_ASSOCIATION_ID: &str = "default";

/// `DEFAULT_EDITOR_ASSOCIATION`: informação estática sobre o editor de texto.
pub fn default_editor_association() -> DefaultEditorAssociation {
    DefaultEditorAssociation {
        id: DEFAULT_EDITOR_ASSOCIATION_ID,
        display_name: localize("promptOpenWith.defaultEditor.displayName", "Text Editor", &[]),
        provider_display_name: localize("builtinProviderDisplayName", "Built-in", &[]),
    }
}

/// `SIDE_BY_SIDE_EDITOR_ID`: side by side editor id.
pub const SIDE_BY_SIDE_EDITOR_ID: &str = "workbench.editor.sidebysideEditor";

/// `TEXT_DIFF_EDITOR_ID`: text diff editor id.
pub const TEXT_DIFF_EDITOR_ID: &str = "workbench.editors.textDiffEditor";

/// `BINARY_DIFF_EDITOR_ID`: binary diff editor id.
pub const BINARY_DIFF_EDITOR_ID: &str = "workbench.editors.binaryResourceDiffEditor";

/// `IEditorDescriptor` (sem `instantiate` e `describes`, que dependem do `IInstantiationService` e do `IEditorGroup`):
/// o que um `EditorInput` lê de um descritor para escolher o editor preferido.
pub trait IEditorDescriptor {
    /// `typeId`: the unique type identifier of the editor. All instances of the same `IEditorPane` should have the same
    /// type identifier.
    fn editor_type_id(&self) -> String;

    /// `name`: the display name of the editor.
    fn name(&self) -> String;
}

/// `IEditorPane`, reduzido ao que os consumidores portados leem (`findViewStateForEditor`): a entrada atribuída, o
/// grupo em que aparece (`group.id`) e o estado de visão. O restante (`IComposite`, os eventos de seleção e de
/// scroll, as restrições de tamanho) é interface.
pub trait IEditorPane {
    /// `input`: the assigned input of this editor.
    fn input(&self) -> Option<EditorInputRef>;

    /// `group.id`: the id of the assigned group this editor is showing in.
    fn group_id(&self) -> GroupIdentifier;

    /// Returns the current view state of the editor if any.
    fn get_view_state(&self) -> Option<Dynamic>;

    /// `onDidChangeControl`: an event to notify whenever the control of the pane changes.
    fn on_did_change_control(&self) -> je_base::event::Event<()>;

    /// `getId()`: the identifier of the editor pane (`IComposite.getId`). Tem implementação padrão para não quebrar quem
    /// implementa só o recorte acima: lança o erro `Method not implemented.` do dublê do original.
    fn get_id(&self) -> String {
        je_base::errors::throw(je_base::errors::Error::new("Method not implemented."))
    }

    /// `options`: as opções com que a entrada foi atribuída ao painel (`EditorPane.options`), ou `None` quando não há. Tem
    /// implementação padrão (sem opções) para não quebrar quem implementa só o recorte acima.
    fn options(&self) -> Option<EditorOptions> {
        None
    }

    /// `getControl()`: the control of the editor pane (o `IEditorControl` do original), ou `None` quando o painel não tem
    /// controle. Tem implementação padrão (sem controle) para não quebrar quem implementa só o recorte acima.
    fn get_control(&self) -> Option<EditorControl> {
        None
    }

    /// `editorPane instanceof SideBySideEditorPane`: devolve `getPrimaryEditorPane()` e `getSecondaryEditorPane()` quando o
    /// painel é o do editor lado a lado, senão `None` (o padrão).
    fn side_by_side_panes(&self) -> Option<(Option<Rc<dyn IEditorPane>>, Option<Rc<dyn IEditorPane>>)> {
        None
    }
}

/// `EditorPaneSelectionChangeReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorPaneSelectionChangeReason {
    /// The selection was changed as a result of a programmatic method invocation.
    Programmatic = 1,
    /// The selection was changed by the user.
    User,
    /// The selection was changed as a result of editing in the editor pane.
    Edit,
    /// The selection was changed as a result of a navigation action.
    Navigation,
    /// The selection was changed as a result of a jump action from within the editor pane.
    Jump,
}

/// `IEditorPaneSelectionChangeEvent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorPaneSelectionChangeEvent {
    /// More details for how the selection was made.
    pub reason: EditorPaneSelectionChangeReason,
}

/// `EditorPaneSelectionCompareResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorPaneSelectionCompareResult {
    /// The selections are identical.
    Identical = 1,
    /// The selections are similar.
    Similar = 2,
    /// The selections are entirely different.
    Different = 3,
}

/// `IEditorPaneSelection`.
pub trait IEditorPaneSelection {
    /// Asks to compare this selection to another selection.
    fn compare(&self, other_selection: &dyn IEditorPaneSelection) -> EditorPaneSelectionCompareResult;

    /// Asks to massage the provided `options` in a way that the selection can be restored when the editor is opened again.
    fn restore(&self, options: &TextEditorOptions) -> TextEditorOptions;

    /// Only used for logging to print more info about the selection.
    fn log(&self) -> Option<String> {
        None
    }
}

/// `IEditorPaneScrollPosition`: scroll position of a pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorPaneScrollPosition {
    pub scroll_top: f64,
    pub scroll_left: Option<f64>,
}

/// `findViewStateForEditor`: try to retrieve the view state for the editor pane that has the provided editor input
/// opened, if at all. This method will return `None` if the editor input is not visible in any of the opened editor
/// panes.
pub fn find_view_state_for_editor(input: &EditorInputRef, group: GroupIdentifier, editor_service: &dyn IEditorService) -> Option<Dynamic> {
    editor_service.visible_editor_panes().into_iter().find_map(|editor_pane| {
        let pane_input = editor_pane.input()?;
        (editor_pane.group_id() == group && input.matches(&EditorInputOrUntyped::Typed(pane_input))).then(|| editor_pane.get_view_state()).flatten()
    })
}

/// `TextEditorSelectionRevealType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEditorSelectionRevealType {
    /// Option to scroll vertically or horizontally as necessary and reveal a range centered vertically.
    Center = 0,
    /// Option to scroll vertically or horizontally as necessary and reveal a range centered vertically only if it lies
    /// outside the viewport.
    CenterIfOutsideViewport = 1,
    /// Option to scroll vertically or horizontally as necessary and reveal a range close to the top of the viewport, but
    /// not quite at the top.
    NearTop = 2,
    /// Same as [`Self::NearTop`], only if it lies outside the viewport.
    NearTopIfOutsideViewport = 3,
}

/// `TextEditorSelectionSource` (o `const enum` de textos): os valores de `ITextEditorOptions.selectionSource`.
pub struct TextEditorSelectionSource;

impl TextEditorSelectionSource {
    /// Programmatic source indicates a selection change that was not triggered by the user via keyboard or mouse but
    /// through text editor APIs.
    pub const PROGRAMMATIC: &'static str = "api";
    /// Navigation source indicates a selection change that was caused via some command or UI component such as an
    /// outline tree.
    pub const NAVIGATION: &'static str = "code.navigation";
    /// Jump source indicates a selection change that was caused from within the text editor to another location in the
    /// same or different text editor such as "Go to definition".
    pub const JUMP: &'static str = "code.jump";
}

/// `ITextEditorOptions`: as [`EditorOptions`] mais a seleção do editor de texto. O `Deref` dá acesso aos campos de
/// `IEditorOptions` (`options.pinned`, `options.override_`...), como a herança do original.
#[derive(Clone, Default)]
pub struct TextEditorOptions {
    pub base: EditorOptions,
    /// Text editor selection.
    pub selection: Option<TextEditorSelection>,
    /// Option to control the text editor selection reveal type. Defaults to `TextEditorSelectionRevealType::Center`.
    pub selection_reveal_type: Option<TextEditorSelectionRevealType>,
    /// Source of the call that caused the selection (`TextEditorSelectionSource | string`).
    pub selection_source: Option<String>,
}

impl Deref for TextEditorOptions {
    type Target = EditorOptions;

    fn deref(&self) -> &EditorOptions {
        &self.base
    }
}

impl DerefMut for TextEditorOptions {
    fn deref_mut(&mut self) -> &mut EditorOptions {
        &mut self.base
    }
}

impl From<EditorOptions> for TextEditorOptions {
    fn from(base: EditorOptions) -> Self {
        Self { base, ..Self::default() }
    }
}

impl TextEditorOptions {
    /// `{ override: id }`: as opções só com a sobrescrita de editor por id, o que as entradas dos testes usam.
    pub fn with_override_id(id: &str) -> Self {
        Self::from(EditorOptions { override_: Some(EditorOverride::Id(id.into())), ..EditorOptions::default() })
    }

    /// `options?.override` quando é um id de editor (`string`); `None` para `undefined` ou para um `EditorResolution`.
    pub fn override_id(&self) -> Option<String> {
        match &self.base.override_ {
            Some(EditorOverride::Id(id)) => Some(id.to_string_lossy()),
            Some(EditorOverride::Resolution(_)) | None => None,
        }
    }
}

/// `IMultiDiffEditorResource`: um item de `IResourceMultiDiffEditorInput.resources` (uma entrada de diff com o recurso
/// para onde ir ao abrir o arquivo).
#[derive(Clone, Default)]
pub struct MultiDiffEditorResource {
    /// A parte `IResourceDiffEditorInput`.
    pub diff: UntypedEditorInput,
    pub go_to_file_resource: Option<URI>,
}

/// `IUntypedEditorInput`: `IResourceEditorInput | ITextResourceEditorInput | IUntitledTextResourceEditorInput |
/// IResourceDiffEditorInput | IResourceMultiDiffEditorInput | IResourceSideBySideEditorInput |
/// IResourceMergeEditorInput`, todos os campos opcionais como as formas estruturais do original (ver o cabeçalho).
#[derive(Clone, Default)]
pub struct UntypedEditorInput {
    // `IBaseUntypedEditorInput`.
    /// Optional options to use when opening the input.
    pub options: Option<TextEditorOptions>,
    /// Label to show for the input.
    pub label: Option<String>,
    /// Description to show for the input.
    pub description: Option<String>,

    // `IBaseResourceEditorInput`.
    /// Hint to indicate that this input should be treated as a untitled file.
    pub force_untitled: Option<bool>,
    /// `IUntypedFileEditorInput.forceFile`: a marker to create a `IFileEditorInput` from this untyped input.
    pub force_file: Option<bool>,

    // `IBaseTextResourceEditorInput`.
    /// The contents of the text input if known.
    pub contents: Option<String>,
    /// The encoding of the text input if known.
    pub encoding: Option<String>,
    /// The identifier of the language id of the text input if known to use when displaying the contents.
    pub language_id: Option<String>,

    // `IResourceEditorInput` / `ITextResourceEditorInput` / `IUntitledTextResourceEditorInput`.
    /// The resource URI of the resource to open.
    pub resource: Option<URI>,

    // `IResourceDiffEditorInput`.
    /// The left hand side editor to open inside a diff editor.
    pub original: Option<Box<UntypedEditorInput>>,
    /// The right hand side editor to open inside a diff editor.
    pub modified: Option<Box<UntypedEditorInput>>,

    // `IResourceSideBySideEditorInput`.
    /// The right hand side editor to open inside a side-by-side editor.
    pub primary: Option<Box<UntypedEditorInput>>,
    /// The left hand side editor to open inside a side-by-side editor.
    pub secondary: Option<Box<UntypedEditorInput>>,

    // `IResourceMultiDiffEditorInput`.
    /// A unique identifier of this multi diff editor input.
    pub multi_diff_source: Option<URI>,
    /// The list of resources to compare.
    pub resources: Option<Vec<MultiDiffEditorResource>>,
    /// Whether the editor should be serialized and stored for subsequent sessions.
    pub is_transient: Option<bool>,

    // `IResourceMergeEditorInput`.
    /// The one changed version of the file.
    pub input1: Option<Box<UntypedEditorInput>>,
    /// The second changed version of the file.
    pub input2: Option<Box<UntypedEditorInput>>,
    /// The base common ancestor of the file to merge.
    pub base: Option<Box<UntypedEditorInput>>,
    /// The resulting output of the merge.
    pub result: Option<Box<UntypedEditorInput>>,
    /// `IResourceMergeEditorInputSide.detail`.
    pub detail: Option<String>,
}

impl UntypedEditorInput {
    /// `{ resource }`.
    pub fn from_resource(resource: URI) -> Self {
        Self { resource: Some(resource), ..Self::default() }
    }

    /// `isResourceEditorInput`: `URI.isUri(candidate?.resource)`.
    pub fn is_resource_editor_input(&self) -> bool {
        self.resource.is_some()
    }

    /// `isResourceDiffEditorInput`: `candidate?.original !== undefined && candidate.modified !== undefined`.
    pub fn is_resource_diff_editor_input(&self) -> bool {
        self.original.is_some() && self.modified.is_some()
    }

    /// `isResourceMultiDiffEditorInput`: `!!candidate.resources || !!candidate.multiDiffSource` (a lista vazia é verdadeira).
    pub fn is_resource_multi_diff_editor_input(&self) -> bool {
        self.resources.is_some() || self.multi_diff_source.is_some()
    }

    /// `isResourceSideBySideEditorInput`: nunca para um diff, senão `primary` e `secondary` definidos.
    pub fn is_resource_side_by_side_editor_input(&self) -> bool {
        !self.is_resource_diff_editor_input() && self.primary.is_some() && self.secondary.is_some()
    }

    /// `isUntitledResourceEditorInput`: sem recurso, com o esquema `untitled` ou com `forceUntitled`.
    pub fn is_untitled_resource_editor_input(&self) -> bool {
        match &self.resource {
            None => true,
            Some(resource) => *resource.scheme() == schemas::UNTITLED || self.force_untitled == Some(true),
        }
    }

    /// `isResourceMergeEditorInput`: `base`, `input1`, `input2` e `result` com recurso.
    pub fn is_resource_merge_editor_input(&self) -> bool {
        [&self.base, &self.input1, &self.input2, &self.result].into_iter().all(|side| side.as_ref().is_some_and(|side| side.resource.is_some()))
    }

    /// Alguma das formas que não têm um `resource` próprio: diff, diff múltiplo, lado a lado e merge.
    pub(crate) fn is_compound(&self) -> bool {
        self.is_resource_diff_editor_input()
            || self.is_resource_multi_diff_editor_input()
            || self.is_resource_side_by_side_editor_input()
            || self.is_resource_merge_editor_input()
    }
}

/// `EditorInput | IUntypedEditorInput`.
#[derive(Clone)]
pub enum EditorInputOrUntyped {
    Typed(EditorInputRef),
    Untyped(UntypedEditorInput),
}

impl From<EditorInputRef> for EditorInputOrUntyped {
    fn from(editor: EditorInputRef) -> Self {
        EditorInputOrUntyped::Typed(editor)
    }
}

impl From<&EditorInputRef> for EditorInputOrUntyped {
    fn from(editor: &EditorInputRef) -> Self {
        EditorInputOrUntyped::Typed(editor.clone())
    }
}

impl From<UntypedEditorInput> for EditorInputOrUntyped {
    fn from(editor: UntypedEditorInput) -> Self {
        EditorInputOrUntyped::Untyped(editor)
    }
}

impl From<&UntypedEditorInput> for EditorInputOrUntyped {
    fn from(editor: &UntypedEditorInput) -> Self {
        EditorInputOrUntyped::Untyped(editor.clone())
    }
}

/// `isEditorInput`: `editor instanceof AbstractEditorInput`.
pub fn is_editor_input(editor: Option<&EditorInputOrUntyped>) -> bool {
    matches!(editor, Some(EditorInputOrUntyped::Typed(_)))
}

/// Gera a função livre de um `isResource*`: a entrada tipada (que o original barra com `isEditorInput` para "não casar
/// por engano com entradas tipadas") e a ausente dão `false`, e a sem tipo responde pela checagem de campos.
macro_rules! untyped_predicate {
    ($(#[$doc:meta])* $name:ident => $method:ident) => {
        $(#[$doc])*
        pub fn $name(editor: Option<&EditorInputOrUntyped>) -> bool {
            match editor {
                Some(EditorInputOrUntyped::Untyped(candidate)) => candidate.$method(),
                _ => false,
            }
        }
    };
}

untyped_predicate!(
    /// `isResourceEditorInput`.
    is_resource_editor_input => is_resource_editor_input
);
untyped_predicate!(
    /// `isResourceDiffEditorInput`.
    is_resource_diff_editor_input => is_resource_diff_editor_input
);
untyped_predicate!(
    /// `isResourceMultiDiffEditorInput`.
    is_resource_multi_diff_editor_input => is_resource_multi_diff_editor_input
);
untyped_predicate!(
    /// `isResourceSideBySideEditorInput`.
    is_resource_side_by_side_editor_input => is_resource_side_by_side_editor_input
);
untyped_predicate!(
    /// `isUntitledResourceEditorInput`.
    is_untitled_resource_editor_input => is_untitled_resource_editor_input
);
untyped_predicate!(
    /// `isResourceMergeEditorInput`.
    is_resource_merge_editor_input => is_resource_merge_editor_input
);

/// `Verbosity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verbosity {
    Short,
    Medium,
    Long,
}

/// `SaveReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveReason {
    /// Explicit user gesture.
    Explicit = 1,
    /// Auto save after a timeout.
    Auto = 2,
    /// Auto save after editor focus change.
    FocusChange = 3,
    /// Auto save after window change.
    WindowChange = 4,
}

/// `SaveSource`: o identificador obtido de `SaveSourceRegistry.registerSource()`.
pub type SaveSource = String;

/// `ISaveSourceDescriptor`.
struct SaveSourceDescriptor {
    source: SaveSource,
    label: String,
}

/// `SaveSourceFactory`.
#[derive(Default)]
pub struct SaveSourceFactory {
    map_id_to_save_source: RefCell<HashMap<SaveSource, SaveSourceDescriptor>>,
}

impl SaveSourceFactory {
    /// Registers a `SaveSource` with an identifier and label to the registry so that it can be used in save operations.
    pub fn register_source(&self, id: &str, label: &str) -> SaveSource {
        let mut sources = self.map_id_to_save_source.borrow_mut();
        let descriptor = sources.entry(id.to_string()).or_insert_with(|| SaveSourceDescriptor { source: id.to_string(), label: label.to_string() });
        descriptor.source.clone()
    }

    pub fn get_source_label(&self, source: &str) -> String {
        self.map_id_to_save_source.borrow().get(source).map_or_else(|| source.to_string(), |descriptor| descriptor.label.clone())
    }
}

thread_local! {
    static SAVE_SOURCE_REGISTRY: SaveSourceFactory = SaveSourceFactory::default();
}

/// `SaveSourceRegistry`: o registro global (um por thread) de origens de salvamento.
pub fn with_save_source_registry<R>(f: impl FnOnce(&SaveSourceFactory) -> R) -> R {
    SAVE_SOURCE_REGISTRY.with(f)
}

/// `ISaveOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveOptions {
    /// An indicator how the save operation was triggered.
    pub reason: Option<SaveReason>,
    /// An indicator about the source of the save operation. Must use `SaveSourceRegistry.registerSource()` to obtain.
    pub source: Option<SaveSource>,
    /// Forces to save the contents of the working copy again even if the working copy is not dirty.
    pub force: Option<bool>,
    /// Instructs the save operation to skip any save participants.
    pub skip_save_participants: Option<bool>,
    /// A hint as to which file systems should be available for saving.
    pub available_file_systems: Option<Vec<String>>,
}

/// `IRevertOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RevertOptions {
    /// Forces to load the contents of the working copy again even if the working copy is not dirty.
    pub force: Option<bool>,
    /// A soft revert will clear dirty state of a working copy but will not attempt to load it from its persisted
    /// state. This option may be used in scenarios where an editor is closed and where we do not require to load the
    /// contents.
    pub soft: Option<bool>,
}

/// `IMoveResult`.
#[derive(Clone)]
pub struct MoveResult {
    pub editor: EditorInputOrUntyped,
    pub options: Option<TextEditorOptions>,
}

/// `EditorInputCapabilities`: máscara de bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EditorInputCapabilities(pub u32);

impl EditorInputCapabilities {
    /// Signals no specific capability for the input.
    pub const NONE: EditorInputCapabilities = EditorInputCapabilities(0);
    /// Signals that the input is readonly.
    pub const READONLY: EditorInputCapabilities = EditorInputCapabilities(1 << 1);
    /// Signals that the input is untitled.
    pub const UNTITLED: EditorInputCapabilities = EditorInputCapabilities(1 << 2);
    /// Signals that the input can only be shown in one group and not be split into multiple groups.
    pub const SINGLETON: EditorInputCapabilities = EditorInputCapabilities(1 << 3);
    /// Signals that the input requires workspace trust.
    pub const REQUIRES_TRUST: EditorInputCapabilities = EditorInputCapabilities(1 << 4);
    /// Signals that the editor can split into 2 in the same editor group.
    pub const CAN_SPLIT_IN_GROUP: EditorInputCapabilities = EditorInputCapabilities(1 << 5);
    /// Signals that the editor wants its description to be visible when presented to the user. By default, a UI
    /// component may decide to hide the description portion for brevity.
    pub const FORCE_DESCRIPTION: EditorInputCapabilities = EditorInputCapabilities(1 << 6);
    /// Signals that the editor supports dropping into the editor by holding shift.
    pub const CAN_DROP_INTO_EDITOR: EditorInputCapabilities = EditorInputCapabilities(1 << 7);
    /// Signals that the editor is composed of multiple editors within.
    pub const MULTIPLE_EDITORS: EditorInputCapabilities = EditorInputCapabilities(1 << 8);
    /// Signals that the editor cannot be in a dirty state and may still have unsaved changes.
    pub const SCRATCHPAD: EditorInputCapabilities = EditorInputCapabilities(1 << 9);
    /// Signals that the editor should be revealed when being opened if it is already opened in any editor group.
    pub const FORCE_REVEAL: EditorInputCapabilities = EditorInputCapabilities(1 << 10);
    /// Signals that the editor must be opened in a modal editor part. This is honored unless the user has explicitly
    /// opted out of modal editors via `workbench.editor.useModal: 'off'`.
    pub const REQUIRES_MODAL: EditorInputCapabilities = EditorInputCapabilities(1 << 11);
    /// Signals that the editor is exempt from the opened editors limit (`workbench.editor.limit`): it never counts
    /// towards the limit and is never auto-closed to satisfy it.
    pub const EXCLUDE_FROM_EDITOR_LIMIT: EditorInputCapabilities = EditorInputCapabilities(1 << 12);
    /// Signals that the editor cannot be closed through standard user initiated close actions, such as the tab close
    /// button, middle click, or close commands. Callers with an explicit lifecycle requirement can force the editor to
    /// close.
    pub const CANNOT_CLOSE: EditorInputCapabilities = EditorInputCapabilities(1 << 13);

    /// `(capabilities & capability) !== 0`.
    pub fn intersects(self, capability: EditorInputCapabilities) -> bool {
        self.0 & capability.0 != 0
    }
}

impl BitOr for EditorInputCapabilities {
    type Output = EditorInputCapabilities;

    fn bitor(self, other: EditorInputCapabilities) -> EditorInputCapabilities {
        EditorInputCapabilities(self.0 | other.0)
    }
}

impl BitOrAssign for EditorInputCapabilities {
    fn bitor_assign(&mut self, other: EditorInputCapabilities) {
        self.0 |= other.0;
    }
}

impl BitAnd for EditorInputCapabilities {
    type Output = EditorInputCapabilities;

    fn bitand(self, other: EditorInputCapabilities) -> EditorInputCapabilities {
        EditorInputCapabilities(self.0 & other.0)
    }
}

impl Not for EditorInputCapabilities {
    type Output = EditorInputCapabilities;

    fn not(self) -> EditorInputCapabilities {
        EditorInputCapabilities(!self.0)
    }
}

/// `isSideBySideEditorInput`: `isEditorInput(candidate?.primary) && isEditorInput(candidate?.secondary)`.
pub fn is_side_by_side_editor_input(editor: Option<&EditorInputOrUntyped>) -> bool {
    matches!(editor, Some(EditorInputOrUntyped::Typed(editor)) if editor.side_by_side_editors().is_some())
}

/// `editor instanceof SideBySideEditorInput`: o `(primary, secondary)` da entrada lado a lado, e também o da entrada de diff
/// (`DiffEditorInput` herda `SideBySideEditorInput`: o `primary` é o lado modificado e o `secondary` o original).
pub fn side_by_side_input_sides(editor: &EditorInputRef) -> Option<(EditorInputRef, EditorInputRef)> {
    editor.side_by_side_editors().or_else(|| editor.diff_editors())
}

/// `isDiffEditorInput`: `isEditorInput(candidate?.modified) && isEditorInput(candidate?.original)`.
pub fn is_diff_editor_input(editor: Option<&EditorInputOrUntyped>) -> bool {
    matches!(editor, Some(EditorInputOrUntyped::Typed(editor)) if editor.diff_editors().is_some())
}

/// `isEditorInputWithDiffResources`: `URI.isUri(candidate?.diffResources?.original) && URI.isUri(candidate.diffResources.modified)`.
pub fn is_editor_input_with_diff_resources(editor: Option<&EditorInputOrUntyped>) -> bool {
    matches!(editor, Some(EditorInputOrUntyped::Typed(editor)) if editor.diff_resources().is_some())
}

/// `EditorInputWithOptions`.
#[derive(Clone)]
pub struct EditorInputWithOptions {
    pub editor: EditorInputRef,
    pub options: Option<TextEditorOptions>,
}

/// `IEditorOpenContext`: context passed into `EditorPane#setInput` to give additional context information around why the
/// editor was opened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorOpenContext {
    /// An indicator if the editor input is new for the group the editor is in. An editor is new for a group if it was not
    /// part of the group before and otherwise was already opened in the group and just became the active editor.
    pub new_in_group: Option<bool>,
}

/// `IEditorIdentifier`.
#[derive(Clone)]
pub struct EditorIdentifier {
    pub group_id: GroupIdentifier,
    pub editor: EditorInputRef,
}

/// `IEditorCommandsContext`: the editor commands context is used for editor commands (e.g. in the editor title) and we
/// must ensure that the context is serializable because it potentially travels to the extension host!
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorCommandsContext {
    pub group_id: GroupIdentifier,
    pub editor_index: Option<usize>,
    pub preserve_focus: Option<bool>,
}

/// `EditorCloseContext`: more information around why an editor was closed in the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorCloseContext {
    /// No specific context for closing (e.g. explicit user gesture).
    Unknown,
    /// The editor closed because it was replaced with another editor. This can either happen via explicit replace call or
    /// when an editor is in preview mode and another editor opens.
    Replace,
    /// The editor closed as a result of moving it to another group.
    Move,
    /// The editor closed because another editor turned into preview and this used to be the preview editor before.
    Unpin,
}

/// `IEditorCloseEvent`.
#[derive(Clone)]
pub struct EditorCloseEvent {
    pub group_id: GroupIdentifier,
    pub editor: EditorInputRef,
    /// More information around why the editor was closed.
    pub context: EditorCloseContext,
    /// The index of the editor before closing.
    pub index: usize,
    /// Whether the editor was sticky or not.
    pub sticky: bool,
}

/// `IActiveEditorChangeEvent`.
#[derive(Clone)]
pub struct ActiveEditorChangeEvent {
    /// The new active editor or `None` if the group is empty.
    pub editor: Option<EditorInputRef>,
    /// Indicates whether the editor change is the result of an explicit user action (`true`) or happened automatically
    /// as a side effect. When omitted, callers should treat the change as explicit.
    pub is_explicit: Option<bool>,
}

/// `IEditorWillMoveEvent`.
#[derive(Clone)]
pub struct EditorWillMoveEvent {
    pub group_id: GroupIdentifier,
    pub editor: EditorInputRef,
    /// The target group of the move operation.
    pub target: GroupIdentifier,
}

/// `IEditorWillOpenEvent`.
#[derive(Clone)]
pub struct EditorWillOpenEvent {
    pub group_id: GroupIdentifier,
    pub editor: EditorInputRef,
}

/// `IWillInstantiateEditorPaneEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WillInstantiateEditorPaneEvent {
    /// `IEditorDescriptor.typeId`.
    pub editor_type_id: String,
}

/// `GroupIdentifier`.
pub type GroupIdentifier = i64;

/// `GroupModelChangeKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupModelChangeKind {
    // Group Changes
    GroupActive,
    GroupIndex,
    GroupLabel,
    GroupLocked,

    // Editors Change
    EditorsSelection,

    // Editor Changes
    EditorOpen,
    EditorClose,
    EditorMove,
    EditorActive,
    EditorLabel,
    EditorCapabilities,
    EditorPin,
    EditorTransient,
    EditorSticky,
    EditorDirty,
    EditorWillDispose,
}

/// `SideBySideEditor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideBySideEditor {
    Primary = 1,
    Secondary = 2,
    Both = 3,
    Any = 4,
}

/// `EditorsOrder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorsOrder {
    /// Editors sorted by most recent activity (most recent active first).
    MostRecentlyActive,
    /// Editors sorted by sequential order.
    Sequential,
}

/// `IFindEditorOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindEditorOptions {
    /// Whether to consider any or both side by side editor as matching. By default, side by side editors will not be
    /// considered as matching, even if the editor is opened in one of the sides (`PRIMARY | SECONDARY | ANY`).
    pub support_side_by_side: Option<SideBySideEditor>,
    /// The order in which to consider editors for finding.
    pub order: Option<EditorsOrder>,
}

/// `IMatchEditorOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatchEditorOptions {
    /// Whether to consider a side by side editor as matching (`ANY | BOTH`). By default, side by side editors will not be
    /// considered as matching, even if the editor is opened in one of the sides.
    pub support_side_by_side: Option<SideBySideEditor>,
    /// Only consider an editor to match when the `candidate === editor` but not when `candidate.matches(editor)`.
    pub strict_equals: Option<bool>,
}

/// `string | string[]`: o filtro de esquema do `IEditorResourceAccessorOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemeFilter {
    Single(String),
    Multiple(Vec<String>),
}

impl SchemeFilter {
    /// `filter === resource.scheme` ou `filter.some(scheme => resource.scheme === scheme)`.
    fn accepts(&self, resource: &URI) -> bool {
        let scheme = resource.scheme().to_string_lossy();
        match self {
            SchemeFilter::Single(filter) => *filter == scheme,
            SchemeFilter::Multiple(filters) => filters.iter().any(|candidate| *candidate == scheme),
        }
    }
}

impl From<&str> for SchemeFilter {
    fn from(scheme: &str) -> Self {
        SchemeFilter::Single(scheme.to_string())
    }
}

impl From<&[&str]> for SchemeFilter {
    fn from(schemes: &[&str]) -> Self {
        SchemeFilter::Multiple(schemes.iter().map(|scheme| scheme.to_string()).collect())
    }
}

/// `IEditorResourceAccessorOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorResourceAccessorOptions {
    /// Allows to access the `resource(s)` of side by side editors. If not specified, a `resource` for a side by side
    /// editor will always be `None`.
    pub support_side_by_side: Option<SideBySideEditor>,
    /// Allows to filter the scheme to consider. A resource scheme that does not match a filter will not be considered.
    pub filter_by_scheme: Option<SchemeFilter>,
}

/// `URI | { primary?: URI; secondary?: URI }`: o resultado do `EditorResourceAccessor`.
#[derive(Clone)]
pub enum EditorUri {
    Resource(URI),
    Sides { primary: Option<URI>, secondary: Option<URI> },
}

/// A forma de URI que o `EditorResourceAccessor` quer: a original (`preferredResource`) ou a canônica (`resource`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum UriKind {
    Original,
    Canonical,
}

/// `EditorResourceAccessorImpl`.
pub struct EditorResourceAccessor;

impl EditorResourceAccessor {
    /// The original URI of an editor is the URI that was used originally to open the editor and should be used whenever
    /// the URI is presented to the user, e.g. as a label together with utility methods such as `ResourceLabel` or
    /// `ILabelService` that can turn this original URI into the best form for presenting.
    ///
    /// In contrast, the canonical URI ([`Self::get_canonical_uri`]) may be different and should be used whenever the URI
    /// is used to e.g. compare with other editors or when caching certain data based on the URI.
    ///
    /// For example: on Windows and macOS, the same file URI with different casing may point to the same file. The editor
    /// may chose to "normalize" the URI into a canonical form so that only one editor opens for same file URIs with
    /// different casing. As such, the original URI and the canonical URI can be different.
    pub fn get_original_uri(editor: Option<&EditorInputOrUntyped>, options: Option<&EditorResourceAccessorOptions>) -> Option<URI> {
        match Self::resolve_uri(editor, options, UriKind::Original) {
            Some(EditorUri::Resource(resource)) => Some(resource),
            Some(EditorUri::Sides { .. }) | None => None,
        }
    }

    /// `getOriginalUri(editor, { supportSideBySide: SideBySideEditor.BOTH })`: a união inteira, com os dois lados.
    pub fn get_original_uri_sides(editor: Option<&EditorInputOrUntyped>, options: Option<&EditorResourceAccessorOptions>) -> Option<EditorUri> {
        Self::resolve_uri(editor, options, UriKind::Original)
    }

    /// The canonical URI of an editor is the true unique identifier of the editor and should be used whenever the URI is
    /// used e.g. to compare with other editors or when caching certain data based on the URI.
    ///
    /// In contrast, the original URI ([`Self::get_original_uri`]) may be different and should be used whenever the URI is
    /// presented to the user, e.g. as a label.
    pub fn get_canonical_uri(editor: Option<&EditorInputOrUntyped>, options: Option<&EditorResourceAccessorOptions>) -> Option<URI> {
        match Self::resolve_uri(editor, options, UriKind::Canonical) {
            Some(EditorUri::Resource(resource)) => Some(resource),
            Some(EditorUri::Sides { .. }) | None => None,
        }
    }

    /// `getCanonicalUri(editor, { supportSideBySide: SideBySideEditor.BOTH })`: a união inteira, com os dois lados.
    pub fn get_canonical_uri_sides(editor: Option<&EditorInputOrUntyped>, options: Option<&EditorResourceAccessorOptions>) -> Option<EditorUri> {
        Self::resolve_uri(editor, options, UriKind::Canonical)
    }

    /// O `getOriginalUri` e o `getCanonicalUri` do original diferem só na URI lida (`preferredResource ?? resource` ou
    /// `resource`), então o corpo é um só, parametrizado.
    fn resolve_uri(editor: Option<&EditorInputOrUntyped>, options: Option<&EditorResourceAccessorOptions>, kind: UriKind) -> Option<EditorUri> {
        let mut editor = editor?.clone();

        // Merge editors are handled with `merged` result editor
        if let EditorInputOrUntyped::Untyped(untyped) = &editor
            && untyped.is_resource_merge_editor_input()
        {
            let result = untyped.result.as_deref().cloned().map(EditorInputOrUntyped::Untyped);
            return Self::resolve_uri(result.as_ref(), options, kind);
        }

        // Optionally support side-by-side editors
        let support_side_by_side = options.and_then(|options| options.support_side_by_side);
        let filter_by_scheme = options.and_then(|options| options.filter_by_scheme.clone());
        if let Some(support_side_by_side) = support_side_by_side
            && let (Some(primary), Some(secondary)) = Self::get_side_editors(&editor)
        {
            let filter_only = EditorResourceAccessorOptions { support_side_by_side: None, filter_by_scheme: filter_by_scheme.clone() };
            let resource_of = |side: &EditorInputOrUntyped| match Self::resolve_uri(Some(side), Some(&filter_only), kind) {
                Some(EditorUri::Resource(resource)) => Some(resource),
                Some(EditorUri::Sides { .. }) | None => None,
            };
            match support_side_by_side {
                SideBySideEditor::Both => {
                    return Some(EditorUri::Sides { primary: resource_of(&primary), secondary: resource_of(&secondary) });
                }
                SideBySideEditor::Any => return resource_of(&primary).or_else(|| resource_of(&secondary)).map(EditorUri::Resource),
                SideBySideEditor::Primary => editor = primary,
                SideBySideEditor::Secondary => editor = secondary,
            }
        }

        let resource = match &editor {
            EditorInputOrUntyped::Untyped(untyped) => {
                if untyped.is_compound() {
                    return None;
                }
                untyped.resource.clone()
            }
            EditorInputOrUntyped::Typed(typed) => match kind {
                // Original URI is the `preferredResource` of an editor if any
                UriKind::Original => typed.preferred_resource().or_else(|| typed.resource()),
                // Canonical URI is the `resource` of an editor
                UriKind::Canonical => typed.resource(),
            },
        };
        let resource = resource?;

        match filter_by_scheme {
            Some(filter) if !filter.accepts(&resource) => None,
            _ => Some(EditorUri::Resource(resource)),
        }
    }

    /// `getSideEditors`: o lado primário e o secundário de uma entrada lado a lado ou de diff (tipada ou sem tipo).
    fn get_side_editors(editor: &EditorInputOrUntyped) -> (Option<EditorInputOrUntyped>, Option<EditorInputOrUntyped>) {
        match editor {
            EditorInputOrUntyped::Typed(typed) => {
                if let Some((primary, secondary)) = typed.side_by_side_editors() {
                    return (Some(primary.into()), Some(secondary.into()));
                }
                if let Some((modified, original)) = typed.diff_editors() {
                    return (Some(modified.into()), Some(original.into()));
                }
            }
            EditorInputOrUntyped::Untyped(untyped) => {
                let to_editor = |side: &Option<Box<UntypedEditorInput>>| side.as_deref().cloned().map(EditorInputOrUntyped::Untyped);
                if untyped.is_resource_side_by_side_editor_input() {
                    return (to_editor(&untyped.primary), to_editor(&untyped.secondary));
                }
                if untyped.is_resource_diff_editor_input() {
                    return (to_editor(&untyped.modified), to_editor(&untyped.original));
                }
            }
        }

        (None, None)
    }
}

/// O `EditorInput` visto como o recorte que o `FilesConfigurationService` lê: `EditorResourceAccessor.getOriginalUri(editor,
/// { supportSideBySide: SideBySideEditor.PRIMARY })`.
pub struct EditorOriginalUriOf(pub EditorInputRef);

impl EditorOriginalUri for EditorOriginalUriOf {
    fn primary_original_uri(&self) -> Option<URI> {
        let options = EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Primary), filter_by_scheme: None };
        EditorResourceAccessor::get_original_uri(Some(&EditorInputOrUntyped::Typed(self.0.clone())), Some(&options))
    }
}

impl ResourceOrEditor {
    /// A variante `EditorInput` da união `EditorInput | URI`.
    pub fn from_editor(editor: &EditorInputRef) -> Self {
        ResourceOrEditor::Editor(Rc::new(EditorOriginalUriOf(editor.clone())))
    }
}

/// `EditorCloseMethod`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorCloseMethod {
    Unknown,
    Keyboard,
    Mouse,
}

/// `PreventPinnedEditorClose`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreventPinnedEditorClose {
    KeyboardAndMouse,
    Keyboard,
    Mouse,
    Never,
}

/// `CloseDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseDirection {
    Left,
    Right,
}

/// `isTextEditorViewState`: um estado de diff vale pelo `modified`; um estado de editor de código tem `contributionsState`,
/// `viewState` e `cursorState` (array). Os estados são objetos dinâmicos, como o `object` do original.
pub fn is_text_editor_view_state(candidate: &Dynamic) -> bool {
    if !candidate.is_truthy() {
        return false;
    }

    let diff_modified = candidate.get_str("modified");
    if diff_modified.is_truthy() {
        return is_text_editor_view_state(&diff_modified);
    }

    candidate.get_str("contributionsState").is_truthy() && candidate.get_str("viewState").is_truthy() && candidate.get_str("cursorState").is_array()
}

/// Identidade do objeto de duas entradas (`a === b`).
pub fn is_same_editor(a: &EditorInputRef, b: &EditorInputRef) -> bool {
    id_of(a) == id_of(b)
}
