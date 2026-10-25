//! Porte parcial de `src/vs/workbench/contrib/files/common/files.ts`: o `UndoConfirmLevel`, os valores de
//! `explorer.incrementalNaming`, `VIEW_ID`, `SESSIONS_FILES_VIEW_ID`, `SortOrder`, `LexicographicOptions` e
//! `ISortOrderConfiguration`, que o explorador lê da configuração, e o `TextFileContentProvider`.
//!
//! O resto do módulo (`VIEWLET_ID`, `IFilesConfiguration` e as chaves de contexto) entra com
//! os consumidores que o leem. O [`OpenEditor`] já está aqui.
//!
//! Divergências do `TextFileContentProvider`:
//! - **Injeção.** `@ITextFileService`, `@IFileService`, `@ILanguageService` e `@IModelService` são parâmetros de
//!   [`TextFileContentProvider::new`]. Como as promessas de `provideTextContent` e o ouvinte de mudança de arquivos
//!   não podem emprestar `self`, os quatro serviços ficam num `Rc` compartilhado e `resolveEditorModel` é uma função livre
//!   sobre ele.
//! - **`MutableDisposable` do `fileWatcherDisposable`.** O ouvinte `Event.once(onWillDispose)` guarda um `Weak` dele, porque o
//!   store que ele limpa é o próprio dono do ouvinte.
//! - **`ITextModel`.** É o [`TextModel`] concreto; `ITextModel | null` é `Option`. `content.value` é o `TextBufferFactoryRef`,
//!   consumido uma vez.
//! - **`JSON.stringify` e `JSON.parse`.** São os do `serde_json`; o texto do `query` guarda `{"scheme":...,"query":...}` na
//!   mesma ordem do original, e `JSON.parse` inválido rejeita com `SyntaxError`.
//! - **`async provideTextContent`.** É o [`MaybePromise`] do contrato; sem `query` devolve `null` já resolvido.
//! - **`resolveEditorModel` sem aguardar.** O ouvinte de mudança descarta a promessa, como o original (não há `await`).

use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::{URI, UriChange};
use je_editor::language_service::{ILanguageSelection, ILanguageService};
use je_editor::services_model::IModelService;
use je_editor::text_model::{TextModel, TextSource};
use je_platform::files::files::FileChangeType;
use je_platform::files::i_file_service::IFileService;

use crate::common::editor::{
    EditorInputOrUntyped, EditorInputRef, EditorResourceAccessor, EditorResourceAccessorOptions, GroupIdentifier, SideBySideEditor, TextEditorOptions, UntypedEditorInput,
};
use crate::common::editor::editor_group_model::EditorOrIndex;
use crate::services::editor::editor_groups_service::IEditorGroup;
use crate::services::editor::editor_service::IEditorService;
use crate::services::textfile::textfiles::ITextFileService;
use crate::services::textmodel_resolver::resolver_service::ITextModelContentProvider;

/// `TEXT_FILE_EDITOR_ID`: id of the text file editor pane.
pub const TEXT_FILE_EDITOR_ID: &str = "workbench.editors.files.textFileEditor";

/// `FILE_EDITOR_INPUT_ID`: id of the file editor input (`FileEditorInput.typeId`).
pub const FILE_EDITOR_INPUT_ID: &str = "workbench.editors.files.fileEditorInput";

/// `BINARY_FILE_EDITOR_ID`: id of the binary file editor pane.
pub const BINARY_FILE_EDITOR_ID: &str = "workbench.editors.files.binaryFileEditor";

/// `UndoConfirmLevel` (`explorer.confirmUndo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoConfirmLevel {
    Verbose,
    Default,
    Light,
}

impl UndoConfirmLevel {
    /// O valor de texto do `const enum` (`'verbose' | 'default' | 'light'`).
    pub const fn as_str(self) -> &'static str {
        match self {
            UndoConfirmLevel::Verbose => "verbose",
            UndoConfirmLevel::Default => "default",
            UndoConfirmLevel::Light => "light",
        }
    }

    /// `value === UndoConfirmLevel.X` para o valor que a configuração devolveu.
    pub fn is(self, value: Option<&str>) -> bool {
        value == Some(self.as_str())
    }
}

/// `explorer.incrementalNaming`: `'simple' | 'smart' | 'disabled'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncrementalNamingSetting {
    Simple,
    Smart,
    Disabled,
}

impl IncrementalNamingSetting {
    /// O valor que a configuração devolveu. O original compara o texto com `'disabled'` e com `'simple'` e trata todo o
    /// resto como o ramo inteligente (`incrementFileName` só testa `=== 'simple'`), então um valor desconhecido ou ausente
    /// é [`IncrementalNamingSetting::Smart`].
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("simple") => IncrementalNamingSetting::Simple,
            Some("disabled") => IncrementalNamingSetting::Disabled,
            _ => IncrementalNamingSetting::Smart,
        }
    }

    /// O `'simple' | 'smart'` que o `incrementFileName` aceita, ou `None` quando é `'disabled'`.
    pub fn enabled(self) -> Option<IncrementalNaming> {
        match self {
            IncrementalNamingSetting::Simple => Some(IncrementalNaming::Simple),
            IncrementalNamingSetting::Smart => Some(IncrementalNaming::Smart),
            IncrementalNamingSetting::Disabled => None,
        }
    }
}

/// `'simple' | 'smart'`: o modo de numeração que `incrementFileName` recebe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncrementalNaming {
    Simple,
    Smart,
}

/// `VIEW_ID`: explorer file view id.
pub const VIEW_ID: &str = "workbench.explorer.fileView";

/// `SESSIONS_FILES_VIEW_ID`.
pub const SESSIONS_FILES_VIEW_ID: &str = "sessions.files.explorer";

/// `ExplorerFolderContext`: `explorerResourceIsFolder`, true when the focused item in the EXPLORER is a folder. A chave é
/// criada (e registrada) na primeira chamada, como as de `common/contextkeys.rs`.
pub fn explorer_folder_context() -> je_platform::contextkey::contextkey::RawContextKey {
    use je_platform::contextkey::contextkey::{ContextKeyMeta, RawContextKey};
    thread_local! {
        static KEY: RawContextKey = RawContextKey::new(
            "explorerResourceIsFolder",
            je_base::dynamic::Dynamic::Bool(false),
            ContextKeyMeta::Info {
                ty: "boolean".to_string(),
                description: je_base::nls::localize("explorerResourceIsFolder", "True when the focused item in the EXPLORER is a folder.", &[]),
            },
        );
    }
    KEY.with(Clone::clone)
}

/// Define uma chave de contexto do explorador (`new RawContextKey<boolean>(key, default, { type: 'boolean', description })`),
/// criada e registrada na primeira chamada, como [`explorer_folder_context`].
macro_rules! explorer_boolean_context_key {
    ($(#[$doc:meta])* $name:ident, $key:literal, $default:literal, $description_id:literal, $description:literal) => {
        $(#[$doc])*
        pub fn $name() -> je_platform::contextkey::contextkey::RawContextKey {
            use je_platform::contextkey::contextkey::{ContextKeyMeta, RawContextKey};
            thread_local! {
                static KEY: RawContextKey = RawContextKey::new(
                    $key,
                    je_base::dynamic::Dynamic::Bool($default),
                    ContextKeyMeta::Info { ty: "boolean".to_string(), description: je_base::nls::localize($description_id, $description, &[]) },
                );
            }
            KEY.with(Clone::clone)
        }
    };
}

explorer_boolean_context_key!(
    /// `FoldersViewVisibleContext`.
    folders_view_visible_context, "foldersViewVisible", true, "foldersViewVisible",
    "True when the FOLDERS view (the file tree within the explorer view container) is visible."
);
explorer_boolean_context_key!(
    /// `ExplorerResourceReadonlyContext`.
    explorer_resource_readonly_context, "explorerResourceReadonly", false, "explorerResourceReadonly",
    "True when the focused item in the EXPLORER is read-only."
);
explorer_boolean_context_key!(
    /// `ExplorerResourceParentReadOnlyContext`.
    explorer_resource_parent_read_only_context, "explorerResourceParentReadonly", false, "explorerResourceParentReadonly",
    "True when the focused item in the EXPLORER's parent is read-only."
);
explorer_boolean_context_key!(
    /// `ExplorerRootContext`.
    explorer_root_context, "explorerResourceIsRoot", false, "explorerResourceIsRoot",
    "True when the focused item in the EXPLORER is a root folder."
);
explorer_boolean_context_key!(
    /// `ExplorerResourceCut`.
    explorer_resource_cut, "explorerResourceCut", false, "explorerResourceCut",
    "True when an item in the EXPLORER has been cut for cut and paste."
);
explorer_boolean_context_key!(
    /// `ExplorerResourceMoveableToTrash`.
    explorer_resource_moveable_to_trash, "explorerResourceMoveableToTrash", false, "explorerResourceMoveableToTrash",
    "True when the focused item in the EXPLORER can be moved to trash."
);
explorer_boolean_context_key!(
    /// `FilesExplorerFocusedContext`.
    files_explorer_focused_context, "filesExplorerFocus", true, "filesExplorerFocus",
    "True when the focus is inside the EXPLORER view."
);
explorer_boolean_context_key!(
    /// `ExplorerFocusedContext`.
    explorer_focused_context, "explorerViewletFocus", true, "explorerViewletFocus",
    "True when the focus is inside the EXPLORER viewlet."
);
explorer_boolean_context_key!(
    /// `ExplorerFindProviderActive`.
    explorer_find_provider_active, "explorerFindProviderActive", false, "explorerFindProviderActive",
    "True when the explorer tree is using the explorer find provider."
);
explorer_boolean_context_key!(
    /// `ExplorerCompressedFocusContext`.
    explorer_compressed_focus_context, "explorerViewletCompressedFocus", true, "explorerViewletCompressedFocus",
    "True when the focused item in the EXPLORER view is a compact item."
);
explorer_boolean_context_key!(
    /// `ExplorerCompressedFirstFocusContext`.
    explorer_compressed_first_focus_context, "explorerViewletCompressedFirstFocus", true, "explorerViewletCompressedFirstFocus",
    "True when the focus is inside a compact item's first part in the EXPLORER view."
);
explorer_boolean_context_key!(
    /// `ExplorerCompressedLastFocusContext`.
    explorer_compressed_last_focus_context, "explorerViewletCompressedLastFocus", true, "explorerViewletCompressedLastFocus",
    "True when the focus is inside a compact item's last part in the EXPLORER view."
);
explorer_boolean_context_key!(
    /// `ViewHasSomeCollapsibleRootItemContext`.
    view_has_some_collapsible_root_item_context, "viewHasSomeCollapsibleItem", false, "viewHasSomeCollapsibleItem",
    "True when a workspace in the EXPLORER view has some collapsible root child."
);

/// `ExplorerResourceAvailableEditorIdsContext`: `new RawContextKey<string>('explorerResourceAvailableEditorIds', '')`, sem
/// metadado.
pub fn explorer_resource_available_editor_ids_context() -> je_platform::contextkey::contextkey::RawContextKey {
    use je_platform::contextkey::contextkey::{ContextKeyMeta, RawContextKey};
    thread_local! {
        static KEY: RawContextKey = RawContextKey::new("explorerResourceAvailableEditorIds", je_base::dynamic::Dynamic::string(""), ContextKeyMeta::None);
    }
    KEY.with(Clone::clone)
}

/// `SortOrder` (`explorer.sortOrder`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Default,
    Mixed,
    FilesFirst,
    Type,
    Modified,
    FoldersNestsFiles,
}

impl SortOrder {
    /// O valor de texto do `const enum`.
    pub const fn as_str(self) -> &'static str {
        match self {
            SortOrder::Default => "default",
            SortOrder::Mixed => "mixed",
            SortOrder::FilesFirst => "filesFirst",
            SortOrder::Type => "type",
            SortOrder::Modified => "modified",
            SortOrder::FoldersNestsFiles => "foldersNestsFiles",
        }
    }

    /// O valor que a configuração devolveu; um texto fora do enum (que o esquema da configuração não admite) vale
    /// [`SortOrder::Default`].
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("mixed") => SortOrder::Mixed,
            Some("filesFirst") => SortOrder::FilesFirst,
            Some("type") => SortOrder::Type,
            Some("modified") => SortOrder::Modified,
            Some("foldersNestsFiles") => SortOrder::FoldersNestsFiles,
            _ => SortOrder::Default,
        }
    }
}

/// `LexicographicOptions` (`explorer.sortOrderLexicographicOptions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexicographicOptions {
    Default,
    Upper,
    Lower,
    Unicode,
}

impl LexicographicOptions {
    /// O valor de texto do `const enum`.
    pub const fn as_str(self) -> &'static str {
        match self {
            LexicographicOptions::Default => "default",
            LexicographicOptions::Upper => "upper",
            LexicographicOptions::Lower => "lower",
            LexicographicOptions::Unicode => "unicode",
        }
    }

    /// O valor que a configuração devolveu; um texto fora do enum vale [`LexicographicOptions::Default`].
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("upper") => LexicographicOptions::Upper,
            Some("lower") => LexicographicOptions::Lower,
            Some("unicode") => LexicographicOptions::Unicode,
            _ => LexicographicOptions::Default,
        }
    }
}

/// `ISortOrderConfiguration`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortOrderConfiguration {
    pub sort_order: SortOrder,
    pub lexicographic_options: LexicographicOptions,
    pub reverse: bool,
}

/// Os serviços que o `TextFileContentProvider` injeta.
struct TextFileContentProviderServices {
    text_file_service: Rc<dyn ITextFileService>,
    file_service: Rc<dyn IFileService>,
    language_service: Rc<dyn ILanguageService>,
    model_service: Rc<dyn IModelService>,
}

/// `TextFileContentProvider`.
pub struct TextFileContentProvider {
    base: Rc<Disposable>,
    file_watcher_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    services: Rc<TextFileContentProviderServices>,
}

/// `JSON.parse` com o erro do motor.
fn parse_query(resource: &URI) -> Result<serde_json::Value, Error> {
    serde_json::from_str(&resource.query().to_string_lossy()).map_err(|error| Error::named("SyntaxError", error.to_string()))
}

impl TextFileContentProvider {
    pub fn new(
        text_file_service: Rc<dyn ITextFileService>,
        file_service: Rc<dyn IFileService>,
        language_service: Rc<dyn ILanguageService>,
        model_service: Rc<dyn IModelService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let file_watcher_disposable = MutableDisposable::<dyn IDisposable>::new();
        base.register(file_watcher_disposable.clone());
        Rc::new(Self {
            base,
            file_watcher_disposable,
            services: Rc::new(TextFileContentProviderServices { text_file_service, file_service, language_service, model_service }),
        })
    }

    /// `open(resource, scheme, label, editorService, options?)`.
    pub fn open(resource: &URI, scheme: &str, label: &str, editor_service: &dyn IEditorService, options: Option<TextEditorOptions>) -> Promise<()> {
        let side = |resource: URI| Some(Box::new(UntypedEditorInput { resource: Some(resource), ..UntypedEditorInput::default() }));
        let editor = UntypedEditorInput {
            original: side(Self::resource_to_text_file(scheme, resource)),
            modified: side(resource.clone()),
            label: Some(label.to_string()),
            options,
            ..UntypedEditorInput::default()
        };
        editor_service.open_editor(editor).then(|_| ())
    }

    /// `resourceToTextFile(scheme, resource)` (private static).
    fn resource_to_text_file(scheme: &str, resource: &URI) -> URI {
        let query = serde_json::json!({ "scheme": resource.scheme().to_string_lossy(), "query": resource.query().to_string_lossy() }).to_string();
        resource.with(&UriChange { scheme: scheme.into(), query: query.as_str().into(), ..UriChange::default() })
    }

    /// `textFileToResource(resource)` (private static).
    fn text_file_to_resource(resource: &URI) -> Result<URI, Error> {
        let parsed = parse_query(resource)?;
        let Some(object) = parsed.as_object() else {
            return Err(Error::type_error("Cannot destructure the query of a text file resource"));
        };
        let mut change = UriChange::default();
        if let Some(scheme) = object.get("scheme").and_then(serde_json::Value::as_str) {
            change.scheme = scheme.into();
        }
        if let Some(query) = object.get("query").and_then(serde_json::Value::as_str) {
            change.query = query.into();
        }
        Ok(resource.with(&change))
    }

    /// `provideTextContent(resource)`.
    fn provide_text_content_async(&self, resource: URI) -> Promise<Option<Rc<TextModel>>> {
        let services = self.services.clone();
        let file_watcher_disposable = self.file_watcher_disposable.clone();
        async_fn(async move {
            let saved_file_resource = Self::text_file_to_resource(&resource)?;

            // Make sure our text file is resolved up to date
            let code_editor_model = Self::resolve_editor_model(&services, &resource, true).await?;

            // Make sure to keep contents up to date when it changes
            if file_watcher_disposable.value().is_none() {
                let disposables = DisposableStore::new();
                file_watcher_disposable.set(Some(disposables.clone() as DisposableRef));
                disposables.add(services.file_service.on_did_files_change().listen({
                    let (services, resource) = (services.clone(), resource.clone());
                    move |changes| {
                        if changes.contains(&saved_file_resource, &[FileChangeType::Updated]) {
                            // update model when resource changes (do not create if missing)
                            Self::resolve_editor_model(&services, &resource, false);
                        }
                    }
                }));

                if let Some(code_editor_model) = &code_editor_model {
                    let watcher = Rc::downgrade(&file_watcher_disposable);
                    disposables.add(code_editor_model.on_will_dispose().once().listen(move |_| {
                        if let Some(watcher) = watcher.upgrade() {
                            watcher.clear();
                        }
                    }));
                }
            }

            Ok(code_editor_model)
        })
    }

    /// `resolveEditorModel(resource, createAsNeeded = true)` (private).
    fn resolve_editor_model(services: &Rc<TextFileContentProviderServices>, resource: &URI, create_as_needed: bool) -> Promise<Option<Rc<TextModel>>> {
        let (services, resource) = (services.clone(), resource.clone());
        async_fn(async move {
            let saved_file_resource = Self::text_file_to_resource(&resource)?;

            let content = services.text_file_service.read_stream(&saved_file_resource, None).await?;
            let value = || content.value.take().expect("a fábrica de buffer é consumida uma única vez");

            let mut code_editor_model = services.model_service.get_model(&resource);
            if let Some(model) = &code_editor_model {
                services.model_service.update_model(model, TextSource::Factory(value()), None);
            } else if create_as_needed {
                let language_selector: Rc<dyn ILanguageSelection> = match services.model_service.get_model(&saved_file_resource) {
                    Some(text_file_model) => services.language_service.create_by_id(Some(&text_file_model.get_language_id())),
                    None => services.language_service.create_by_filepath_or_first_line(Some(&saved_file_resource), None),
                };
                code_editor_model = Some(services.model_service.create_model(TextSource::Factory(value()), Some(&*language_selector), Some(&resource), false));
            }

            Ok(code_editor_model)
        })
    }
}

impl ITextModelContentProvider for TextFileContentProvider {
    fn provide_text_content(&self, resource: &URI) -> MaybePromise<Option<Rc<TextModel>>> {
        if resource.query().is_empty() {
            // We require the URI to use the `query` to transport the original scheme and query
            // as done by `resourceToTextFile`
            return MaybePromise::Value(None);
        }
        MaybePromise::Promise(self.provide_text_content_async(resource.clone()))
    }
}

impl IDisposable for TextFileContentProvider {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `OpenEditor.COUNTER`: o contador estático que numera cada `OpenEditor` criado.
static OPEN_EDITOR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// `OpenEditor` (`IEditorIdentifier`): um editor aberto em um grupo, como a vista de editores abertos o enxerga.
#[derive(Clone)]
pub struct OpenEditor {
    editor: EditorInputRef,
    group: Rc<dyn IEditorGroup>,
    id: u64,
}

impl OpenEditor {
    /// `constructor(_editor, _group)`: `this.id = OpenEditor.COUNTER++`.
    pub fn new(editor: EditorInputRef, group: Rc<dyn IEditorGroup>) -> Self {
        Self { editor, group, id: OPEN_EDITOR_COUNTER.fetch_add(1, Ordering::Relaxed) }
    }

    /// `editor`.
    pub fn editor(&self) -> &EditorInputRef {
        &self.editor
    }

    /// `group`.
    pub fn group(&self) -> &Rc<dyn IEditorGroup> {
        &self.group
    }

    /// `groupId`.
    pub fn group_id(&self) -> GroupIdentifier {
        self.group.id()
    }

    /// `getId()`: `openeditor:${groupId}:${id}`.
    pub fn get_id(&self) -> String {
        format!("openeditor:{}:{}", self.group_id(), self.id)
    }

    /// `isPreview()`: `!this._group.isPinned(this.editor)`.
    pub fn is_preview(&self) -> bool {
        !self.group.is_pinned(&EditorOrIndex::Editor(self.editor.clone()))
    }

    /// `isSticky()`.
    pub fn is_sticky(&self) -> bool {
        self.group.is_sticky(&EditorOrIndex::Editor(self.editor.clone()))
    }

    /// `getResource()`: o URI original do lado primário do editor.
    pub fn get_resource(&self) -> Option<URI> {
        EditorResourceAccessor::get_original_uri(
            Some(&EditorInputOrUntyped::Typed(self.editor.clone())),
            Some(&EditorResourceAccessorOptions { support_side_by_side: Some(SideBySideEditor::Primary), filter_by_scheme: None }),
        )
    }
}
