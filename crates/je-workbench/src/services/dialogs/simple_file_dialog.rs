//! Porte de `src/vs/workbench/services/dialogs/browser/simpleFileDialog.ts`: o diálogo de arquivos feito de quick pick,
//! usado quando não há diálogo nativo (janela remota, web) ou quando o `availableFileSystems` pede um esquema que o
//! sistema operacional não abre.
//!
//! Notas de tradução:
//! - **`@IFileService` ... `@IStorageService`.** São as quinze dependências do [`Injectable`] [`SimpleFileDialog`], na ordem
//!   dos parâmetros decorados. O estado mutável do original (campos soltos da classe) vira `Cell` e `RefCell`, com
//!   acessos curtos: o quick pick dispara os eventos de forma síncrona dentro dos setters (`value`, `activeItems`,
//!   `valueSelection`), e os ouvintes voltam ao diálogo, então nenhum empréstimo fica aberto numa chamada ao quick pick.
//! - **`FileQuickPickItem extends IQuickPickItem`.** O [`IQuickPickItem`] do porte não tem campos derivados abertos, então
//!   `uri` e `isFolder` ficam num registro ([`FileQuickPickItem`]) que o diálogo guarda ao lado dos itens do quick pick,
//!   achado pelo `base.id` (o `uri` em texto, único por pasta). O que o original lê de `activeItems[0]` e `selectedItems[0]`
//!   é recuperado por esse id.
//! - **`getActiveDocument().execCommand('insertText', ...)`.** Não há DOM: o texto inserido substitui a seleção corrente
//!   do campo (`valueSelection`, que o próprio diálogo definiu logo antes) e o caret vai para o fim do texto inserido; o
//!   evento `onDidChangeValue` dispara no setter, como o evento `input` do navegador. O buffer de desfazer do campo fica
//!   com o quick pick.
//! - **`IOpenDialogOptions` e `ISaveDialogOptions`.** `getOptions` devolve as opções de abrir; do salvar, o diálogo guarda
//!   o `saveLabel` à parte (`save_label`) para devolvê-lo ao `showSaveDialog` do serviço no botão "Mostrar local".
//! - **`Promise<URI[] | URI | undefined>` de `pickResource`.** É o [`PickedResources`] dentro de `Option`: o botão
//!   personalizado devolve a lista do serviço, o aceite devolve um recurso só.
//! - **`createCancelablePromise` de `updateItems`.** Quando a promessa termina de forma síncrona, ela já está resolvida
//!   ao ser guardada em `updatingPromise`, então não é guardada (o original a limpa no fim do mesmo jeito, só que depois).
//! - **`console.log(e)` de `createItems`.** O erro de leitura da pasta é ignorado sem registro.
//! - **`async createItem`.** Não espera nada de verdade, então é uma função síncrona.
//! - **`OpenLocalFileCommand` e afins.** São os módulos `open_local_file_command`, `save_local_file_command`,
//!   `open_local_folder_command` e `open_local_file_folder_command`, com `ID`, `label()` e `handler()`. O retorno do
//!   comando (uma promessa) é um objeto de carga nativa ([`ObjectRef::with_host`]) com a `Promise`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{async_fn, create_cancelable_promise};
use je_base::cancellation::CancellationToken;
use je_base::collator::locale_compare;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::Error;
use je_base::event::Emitter;
use je_base::extpath::is_valid_basename;
use je_base::keybindings::ResolvedKeybinding;
use je_base::labels::normalize_drive_letter;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::network::schemas;
use je_base::nls::{format, localize};
use je_base::path::Platform;
use je_base::platform::OperatingSystem;
use je_base::promise::{CancelablePromise, Promise};
use je_base::resources::{ExtUri, to_local_resource};
use je_base::strings;
use je_base::themables::ThemeIcon;
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_editor::get_icon_classes::get_icon_classes;
use je_editor::language_service::ILanguageService;
use je_editor::services_model::IModelService;
use je_instantiation::{Injectable, ServicesAccessor};
use je_platform::accessibility::IAccessibilityService;
use je_platform::commands::commands::CommandHandler;
use je_platform::contextkey::contextkey::{ContextKeyMeta, IContextKey, IContextKeyService, RawContextKey};
use je_platform::dialogs::dialogs::{DialogLabel, IFileDialogService, OpenDialogOptions, PickAndOpenOptions, SaveDialogOptions};
use je_platform::files::files::{FileKind, FileStat, FileStatWithMetadata, FileSystemProviderErrorCode, to_file_system_provider_error_code};
use je_platform::files::i_file_service::IFileService;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::label::label::ILabelService;
use je_platform::notification::notification::{INotificationService, NotificationMessage};
use je_platform::path::path_service::IPathService;
use je_platform::quickinput::quick_input::{
    IQuickInputButton, IQuickInputService, IQuickItem, IQuickPick, IQuickPickItem, ItemActivation, QuickInputButtonRef, QuickPickInput, QuickPickOk,
};
use je_platform::remote::remote_agent_environment::RemoteAgentEnvironment;
use je_platform::resources::{ext_uri, fs_path, host_environment, join_path, uri_file};
use je_platform::storage::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::workspace::IWorkspaceContextService;
use je_storage::storage::StorageValue;

use crate::common::editor::{EditorIdentifier, SaveOptions, SaveReason};
use crate::services::editor::editor_service::{I_EDITOR_SERVICE, SaveEditorsOptions, SaveEditorsResult};
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::remote::remote_agent_service::IRemoteAgentService;

/// `OpenLocalFileCommand`.
pub mod open_local_file_command {
    use super::*;

    pub const ID: &str = "workbench.action.files.openLocalFile";

    pub fn label() -> String {
        localize("openLocalFile", "Open Local File...", &[])
    }

    pub fn handler() -> CommandHandler {
        Rc::new(|accessor: &ServicesAccessor, _args: &[Dynamic]| {
            let dialog_service = accessor.get(je_platform::dialogs::dialogs::I_FILE_DIALOG_SERVICE);
            promise_result(dialog_service.pick_file_and_open(local_pick_options()))
        })
    }
}

/// `SaveLocalFileCommand`.
pub mod save_local_file_command {
    use super::*;

    pub const ID: &str = "workbench.action.files.saveLocalFile";

    pub fn label() -> String {
        localize("saveLocalFile", "Save Local File...", &[])
    }

    pub fn handler() -> CommandHandler {
        Rc::new(|accessor: &ServicesAccessor, _args: &[Dynamic]| {
            let editor_service = accessor.get(I_EDITOR_SERVICE);
            let pane = editor_service.active_editor_pane();
            let Some(pane) = pane else {
                return promise_result(Promise::resolved(None::<SaveEditorsResult>));
            };
            let Some(editor) = pane.input() else {
                return promise_result(Promise::resolved(None::<SaveEditorsResult>));
            };
            let options = SaveEditorsOptions {
                base: SaveOptions {
                    reason: Some(SaveReason::Explicit),
                    available_file_systems: Some(vec![schemas::FILE.to_string()]),
                    ..SaveOptions::default()
                },
                save_as: Some(true),
            };
            promise_result(editor_service.save(vec![EditorIdentifier { group_id: pane.group_id(), editor }], Some(options)).then(Some))
        })
    }
}

/// `OpenLocalFolderCommand`.
pub mod open_local_folder_command {
    use super::*;

    pub const ID: &str = "workbench.action.files.openLocalFolder";

    pub fn label() -> String {
        localize("openLocalFolder", "Open Local Folder...", &[])
    }

    pub fn handler() -> CommandHandler {
        Rc::new(|accessor: &ServicesAccessor, _args: &[Dynamic]| {
            let dialog_service = accessor.get(je_platform::dialogs::dialogs::I_FILE_DIALOG_SERVICE);
            promise_result(dialog_service.pick_folder_and_open(local_pick_options()))
        })
    }
}

/// `OpenLocalFileFolderCommand`.
pub mod open_local_file_folder_command {
    use super::*;

    pub const ID: &str = "workbench.action.files.openLocalFileFolder";

    pub fn label() -> String {
        localize("openLocalFileFolder", "Open Local...", &[])
    }

    pub fn handler() -> CommandHandler {
        Rc::new(|accessor: &ServicesAccessor, _args: &[Dynamic]| {
            let dialog_service = accessor.get(je_platform::dialogs::dialogs::I_FILE_DIALOG_SERVICE);
            promise_result(dialog_service.pick_file_folder_and_open(local_pick_options()))
        })
    }
}

/// `{ forceNewWindow: false, availableFileSystems: [Schemas.file] }`.
fn local_pick_options() -> PickAndOpenOptions {
    PickAndOpenOptions { force_new_window: false, available_file_systems: Some(vec![schemas::FILE.to_string()]), ..PickAndOpenOptions::default() }
}

/// O retorno de um comando assíncrono: a promessa dentro de um objeto de carga nativa.
fn promise_result<T: Clone + 'static>(promise: Promise<T>) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(promise)))
}

/// `RemoteFileDialogContext`: o nome da chave de contexto.
pub const REMOTE_FILE_DIALOG_CONTEXT_KEY: &str = "remoteFileDialogVisible";

thread_local! {
    static REMOTE_FILE_DIALOG_CONTEXT: RawContextKey = RawContextKey::new(REMOTE_FILE_DIALOG_CONTEXT_KEY, false, ContextKeyMeta::None);
}

/// `RemoteFileDialogContext`.
pub fn remote_file_dialog_context() -> RawContextKey {
    REMOTE_FILE_DIALOG_CONTEXT.with(|key| key.clone())
}

/// `FileQuickPickItem`: o item do quick pick com o recurso e se é pasta.
#[derive(Clone)]
struct FileQuickPickItem {
    item: IQuickPickItem,
    uri: URI,
    is_folder: bool,
}

impl FileQuickPickItem {
    fn new(label: &str, uri: URI, is_folder: bool, icon_classes: Option<Vec<String>>) -> Self {
        let item = IQuickPickItem {
            base: IQuickItem { id: Some(uri.to_string(false).to_string_lossy()), label: JsString::from(label), icon_classes, ..IQuickItem::default() },
            ..IQuickPickItem::default()
        };
        Self { item, uri, is_folder }
    }

    fn id(&self) -> Option<String> {
        self.item.base.id.clone()
    }

    fn label(&self) -> String {
        self.item.base.label.to_string_lossy()
    }
}

/// `UpdateResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateResult {
    Updated,
    UpdatedWithTrailing,
    #[allow(dead_code)]
    Updating,
    NotUpdated,
    InvalidPath,
}

/// O `URI[] | URI` que o diálogo resolve.
#[derive(Clone)]
enum PickedResources {
    One(URI),
    Many(Vec<URI>),
}

/// O estado de uma rodada de `pickResource` que os ouvintes dividem (`isResolving`, `isAcceptHandled`, `busyDisposable`
/// e o `resolve` da promessa).
struct PickSession {
    is_save: bool,
    is_resolving: Cell<u32>,
    is_accept_handled: Cell<bool>,
    busy_disposable: Rc<MutableDisposable<dyn IDisposable>>,
    resolver: je_base::promise::Resolver<Option<PickedResources>>,
}

/// `ISimpleFileDialog`.
pub trait ISimpleFileDialog: IDisposable {
    fn show_open_dialog(&self, options: OpenDialogOptions) -> Promise<Option<Vec<URI>>>;
    fn show_save_dialog(&self, options: SaveDialogOptions) -> Promise<Option<URI>>;
}

// ---- helpers sobre texto em unidades UTF-16 (o `length` e o `substring` do JavaScript) --------------------------

fn units(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

fn js_len(value: &str) -> usize {
    value.encode_utf16().count()
}

/// `value.substring(start, end)`.
fn substring(value: &str, start: usize, end: usize) -> String {
    let all = units(value);
    let (start, end) = (start.min(all.len()), end.min(all.len()));
    let (start, end) = if start > end { (end, start) } else { (start, end) };
    String::from_utf16_lossy(&all[start..end])
}

/// `value.substring(start)`.
fn substring_from(value: &str, start: usize) -> String {
    substring(value, start, usize::MAX)
}

fn equals_ignore_case(a: &str, b: &str) -> bool {
    strings::equals_ignore_case(&units(a), &units(b))
}

fn starts_with_ignore_case(value: &str, candidate: &str) -> bool {
    strings::starts_with_ignore_case(&units(value), &units(candidate))
}

fn ends_with_slash(value: &str) -> bool {
    value.ends_with('/') || value.ends_with('\\')
}

fn trim_trailing_slash(path: &str) -> String {
    if js_len(path) > 1 && ends_with_slash(path) { substring(path, 0, js_len(path) - 1) } else { path.to_string() }
}

/// `/^[a-zA-Z]:$/.test(value)`.
fn is_drive_letter_only(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// `resources.extUriIgnorePathCase`.
fn ext_uri_ignore_path_case() -> ExtUri {
    ExtUri::ignore_path_case(host_environment())
}

/// `extUri.isEqual(a, b)`.
fn uri_equals(a: &URI, b: &URI) -> bool {
    ext_uri().is_equal(Some(a), Some(b), false)
}

/// `extUriIgnorePathCase.isEqual(a, b)`.
fn uri_equals_ignore_case(a: &URI, b: &URI) -> bool {
    ext_uri_ignore_path_case().is_equal(Some(a), Some(b), false)
}

fn uri_scheme(uri: &URI) -> String {
    uri.scheme().to_string_lossy()
}

/// `URI.from({ scheme, authority, path })` (e, quando há dica, `query` e `fragment`).
fn uri_with_parts(scheme: &str, authority: Option<&str>, path: &str, hint: Option<&URI>) -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(scheme)),
            authority: authority.map(JsString::from),
            path: Some(JsString::from(path)),
            query: hint.map(|hint| hint.query().clone()),
            fragment: hint.map(|hint| hint.fragment().clone()),
        },
        false,
    )
}

fn path_separator(flavor: Platform) -> &'static str {
    match flavor {
        Platform::Posix => "/",
        Platform::Win32 => "\\",
    }
}

/// O diálogo de arquivos simples (o `SimpleFileDialog`).
pub struct SimpleFileDialog {
    this: Weak<SimpleFileDialog>,
    file_service: Rc<dyn IFileService>,
    quick_input_service: Rc<dyn IQuickInputService>,
    label_service: Rc<dyn ILabelService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
    notification_service: Rc<dyn INotificationService>,
    file_dialog_service: Rc<dyn IFileDialogService>,
    model_service: Rc<dyn IModelService>,
    language_service: Rc<dyn ILanguageService>,
    pub(crate) environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    remote_agent_service: Rc<dyn IRemoteAgentService>,
    pub(crate) path_service: Rc<dyn IPathService>,
    keybinding_service: Rc<dyn IKeybindingService>,
    accessibility_service: Rc<dyn IAccessibilityService>,
    storage_service: Rc<dyn StorageService>,
    /// O `_store` do `Disposable`.
    store: Rc<DisposableStore>,
    options: RefCell<OpenDialogOptions>,
    /// O `saveLabel` do `ISaveDialogOptions` original (o `getOptions` só devolve as opções de abrir).
    save_label: RefCell<Option<DialogLabel>>,
    current_folder: RefCell<URI>,
    file_pick_box: RefCell<Option<Rc<dyn IQuickPick>>>,
    /// Os itens que o diálogo pôs no quick pick, com o recurso de cada um.
    file_items: RefCell<Vec<FileQuickPickItem>>,
    hidden: Cell<bool>,
    allow_file_selection: Cell<bool>,
    allow_folder_selection: Cell<bool>,
    remote_authority: Option<String>,
    requires_trailing: Cell<bool>,
    trailing: RefCell<Option<String>>,
    pub(crate) scheme: RefCell<String>,
    context_key: Rc<dyn IContextKey>,
    user_entered_path_segment: RefCell<String>,
    auto_complete_path_segment: RefCell<String>,
    /// O `id` do `activeItem`.
    active_item: RefCell<Option<String>>,
    user_home: RefCell<URI>,
    true_home: RefCell<URI>,
    is_windows: Cell<bool>,
    bad_path: RefCell<Option<String>>,
    /// `undefined` (ainda não pedido) ou o resultado do pedido (`null` é `None`).
    remote_agent_environment: RefCell<Option<Option<RemoteAgentEnvironment>>>,
    separator: RefCell<String>,
    /// When set, the dialog is scoped to a specific URI authority (e.g. for browsing an `agenthost://{authority}/...`
    /// filesystem that uses per-connection authorities rather than the global `remoteAuthority`).
    scoped_authority: RefCell<Option<String>>,
    on_busy_change_emitter: Rc<Emitter<bool>>,
    updating_promise: RefCell<Option<CancelablePromise<bool>>>,
    show_dot_files: Cell<bool>,
}

impl Injectable for SimpleFileDialog {
    type Args = ();
    type Deps = (
        Rc<dyn IFileService>,
        Rc<dyn IQuickInputService>,
        Rc<dyn ILabelService>,
        Rc<dyn IWorkspaceContextService>,
        Rc<dyn INotificationService>,
        Rc<dyn IFileDialogService>,
        Rc<dyn IModelService>,
        Rc<dyn ILanguageService>,
        Rc<dyn IWorkbenchEnvironmentService>,
        Rc<dyn IRemoteAgentService>,
        Rc<dyn IPathService>,
        Rc<dyn IKeybindingService>,
        Rc<dyn IContextKeyService>,
        Rc<dyn IAccessibilityService>,
        Rc<dyn StorageService>,
    );

    fn create(
        _: (),
        (
            file_service,
            quick_input_service,
            label_service,
            workspace_context_service,
            notification_service,
            file_dialog_service,
            model_service,
            language_service,
            environment_service,
            remote_agent_service,
            path_service,
            keybinding_service,
            context_key_service,
            accessibility_service,
            storage_service,
        ): Self::Deps,
    ) -> Rc<Self> {
        let remote_authority = environment_service.remote_authority();
        let context_key = remote_file_dialog_context().bind_to(&*context_key_service);
        let scheme = path_service.default_uri_scheme();
        let root = uri_with_parts(schemas::FILE, None, "/", None);
        let dialog = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            file_service,
            quick_input_service,
            label_service,
            workspace_context_service,
            notification_service,
            file_dialog_service,
            model_service,
            language_service,
            environment_service,
            remote_agent_service,
            path_service,
            keybinding_service,
            accessibility_service,
            storage_service,
            store: DisposableStore::new(),
            options: RefCell::new(OpenDialogOptions::default()),
            save_label: RefCell::new(None),
            current_folder: RefCell::new(root.clone()),
            file_pick_box: RefCell::new(None),
            file_items: RefCell::new(Vec::new()),
            hidden: Cell::new(false),
            allow_file_selection: Cell::new(true),
            allow_folder_selection: Cell::new(false),
            remote_authority,
            requires_trailing: Cell::new(false),
            trailing: RefCell::new(None),
            scheme: RefCell::new(scheme),
            context_key,
            user_entered_path_segment: RefCell::new(String::new()),
            auto_complete_path_segment: RefCell::new(String::new()),
            active_item: RefCell::new(None),
            user_home: RefCell::new(root.clone()),
            true_home: RefCell::new(root),
            is_windows: Cell::new(false),
            bad_path: RefCell::new(None),
            remote_agent_environment: RefCell::new(None),
            separator: RefCell::new("/".to_string()),
            scoped_authority: RefCell::new(None),
            on_busy_change_emitter: Emitter::new(),
            updating_promise: RefCell::new(None),
            show_dot_files: Cell::new(true),
        });

        dialog.get_show_dot_files();
        let disposable_store = DisposableStore::new();
        dialog.store.add_rc(disposable_store.clone());
        let weak = dialog.this.clone();
        let listener = dialog.storage_service.on_did_change_value(StorageScope::Workspace, Some("remoteFileDialog.showDotFiles"), &disposable_store).listen(move |_| {
            if let Some(dialog) = weak.upgrade() {
                // `async _ => ...`: o resultado da promessa não é lido.
                let _ = dialog.on_did_change_show_dot_files();
            }
        });
        disposable_store.add(listener);
        dialog
    }
}

impl IDisposable for SimpleFileDialog {
    fn dispose(&self) {
        self.store.dispose();
    }
}

impl ISimpleFileDialog for SimpleFileDialog {
    fn show_open_dialog(&self, options: OpenDialogOptions) -> Promise<Option<Vec<URI>>> {
        let this = self.rc();
        async_fn(async move {
            *this.scheme.borrow_mut() = this.get_scheme(options.available_file_systems.as_deref(), options.default_uri.as_ref());
            *this.scoped_authority.borrow_mut() = this.get_scoped_authority(options.default_uri.as_ref());
            let user_home = this.get_user_home(false).await?;
            *this.user_home.borrow_mut() = user_home;
            let true_home = this.get_user_home(true).await?;
            *this.true_home.borrow_mut() = true_home;
            let Some(new_options) = this.get_options(&options, false) else {
                return Ok(None);
            };
            *this.options.borrow_mut() = new_options;
            let result = this.pick_resource(false).await?;
            Ok(match result {
                Some(PickedResources::Many(uris)) => Some(uris),
                Some(PickedResources::One(uri)) => Some(vec![uri]),
                None => None,
            })
        })
    }

    fn show_save_dialog(&self, options: SaveDialogOptions) -> Promise<Option<URI>> {
        let this = self.rc();
        async_fn(async move {
            *this.scheme.borrow_mut() = this.get_scheme(options.available_file_systems.as_deref(), options.default_uri.as_ref());
            *this.scoped_authority.borrow_mut() = this.get_scoped_authority(options.default_uri.as_ref());
            let user_home = this.get_user_home(false).await?;
            *this.user_home.borrow_mut() = user_home;
            let true_home = this.get_user_home(true).await?;
            *this.true_home.borrow_mut() = true_home;
            this.requires_trailing.set(true);
            let as_open_options = OpenDialogOptions {
                title: options.title.clone(),
                default_uri: options.default_uri.clone(),
                filters: options.filters.clone(),
                available_file_systems: options.available_file_systems.clone(),
                ..OpenDialogOptions::default()
            };
            let Some(mut new_options) = this.get_options(&as_open_options, true) else {
                return Ok(None);
            };
            new_options.can_select_folders = Some(true);
            new_options.can_select_files = Some(true);
            *this.options.borrow_mut() = new_options;
            *this.save_label.borrow_mut() = options.save_label.clone();

            let result = this.pick_resource(true).await?;
            Ok(match result {
                Some(PickedResources::Many(uris)) => uris.into_iter().next(),
                Some(PickedResources::One(uri)) => Some(uri),
                None => None,
            })
        })
    }
}

impl SimpleFileDialog {
    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o SimpleFileDialog está vivo enquanto um de seus métodos roda")
    }

    fn pick_box(&self) -> Rc<dyn IQuickPick> {
        self.file_pick_box.borrow().clone().expect("o quick pick do diálogo existe depois de pickResource começar")
    }

    fn scheme(&self) -> String {
        self.scheme.borrow().clone()
    }

    fn separator(&self) -> String {
        self.separator.borrow().clone()
    }

    fn current_folder(&self) -> URI {
        self.current_folder.borrow().clone()
    }

    fn user_entered_path_segment(&self) -> String {
        self.user_entered_path_segment.borrow().clone()
    }

    fn auto_complete_path_segment(&self) -> String {
        self.auto_complete_path_segment.borrow().clone()
    }

    fn trailing(&self) -> Option<String> {
        self.trailing.borrow().clone().filter(|trailing| !trailing.is_empty())
    }

    /// `set busy(busy)`.
    fn set_busy(&self, busy: bool) {
        let pick_box = self.pick_box();
        if pick_box.busy() != busy {
            pick_box.set_busy(busy);
            self.on_busy_change_emitter.fire(busy);
        }
    }

    /// `get busy()`.
    fn busy(&self) -> bool {
        self.pick_box().busy()
    }

    /// `this.filePickBox.valueSelection = [start, end]`.
    fn select_value(&self, start: usize, end: usize) {
        self.pick_box().set_value_selection(Some((start as u32, end as u32)));
    }

    fn set_show_dot_files(&self, show_dot_files: bool) {
        self.storage_service.store("remoteFileDialog.showDotFiles", StorageValue::Bool(show_dot_files), StorageScope::Workspace, StorageTarget::User);
    }

    fn get_show_dot_files(&self) {
        self.show_dot_files.set(self.storage_service.get_boolean("remoteFileDialog.showDotFiles", StorageScope::Workspace, Some(true)).unwrap_or(true));
    }

    /// O ouvinte de `onDidChangeValue` do `remoteFileDialog.showDotFiles`.
    fn on_did_change_show_dot_files(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            this.get_show_dot_files();
            if this.file_pick_box.borrow().is_none() {
                // O quick pick ainda não existe: não há lista a atualizar.
                return Ok(());
            }
            this.set_buttons();
            let pick_box = this.pick_box();
            let starting_value = pick_box.value();
            let current_folder = this.current_folder();
            let folder_value = this.path_from_uri(&current_folder, true);
            pick_box.set_value(folder_value.clone());
            this.try_update_items(folder_value, current_folder, true).await?;
            this.pick_box().set_value(starting_value);
            Ok(())
        })
    }

    fn get_options(&self, source: &OpenDialogOptions, is_save: bool) -> Option<OpenDialogOptions> {
        let mut default_uri: Option<URI> = None;
        let mut filename: Option<String> = None;
        if let Some(source_default) = &source.default_uri {
            default_uri = (self.scheme() == uri_scheme(source_default)).then(|| source_default.clone());
            filename = is_save.then(|| ext_uri().basename(source_default, None));
        }
        let default_uri = default_uri.unwrap_or_else(|| {
            let home = self.user_home.borrow().clone();
            match filename.as_deref().filter(|name| !name.is_empty()) {
                Some(filename) => join_path(&home, &[filename]),
                None => home,
            }
        });
        if self.scheme() != schemas::FILE && !self.file_service.has_provider(&default_uri) {
            self.notification_service.info(vec![NotificationMessage::Text(localize(
                "remoteFileDialog.notConnectedToRemote",
                "File system provider for {0} is not available.",
                &[default_uri.to_string(false).to_string_lossy().as_str()],
            ))]);
            return None;
        }
        Some(OpenDialogOptions { default_uri: Some(default_uri), ..source.clone() })
    }

    fn remote_uri_from(&self, path: &str, hint_uri: Option<&URI>) -> URI {
        let scheme = self.scheme();
        if let Some(scoped_authority) = self.scoped_authority.borrow().clone() {
            let mut path = path.replace('\\', "/");
            if !path.is_empty() && !path.starts_with('/') {
                path = format!("/{path}");
            }
            return uri_with_parts(&scheme, Some(&scoped_authority), &path, hint_uri);
        }
        let path = if path.starts_with("\\\\") { path.to_string() } else { path.replace('\\', "/") };
        let uri = if scheme == schemas::FILE { uri_file(&path) } else { uri_with_parts(&scheme, None, &path, hint_uri) };
        // If the default scheme is file, then we don't care about the remote authority or the hint authority
        let authority: Option<String> = if uri_scheme(&uri) == schemas::FILE {
            None
        } else {
            self.remote_authority.clone().or_else(|| hint_uri.map(|hint| hint.authority().to_string_lossy()))
        };
        let has_authority = authority.as_deref().is_some_and(|authority| !authority.is_empty());
        to_local_resource(
            &uri,
            authority.as_deref(),
            // If there is a remote authority, then we should use the system's default URI as the local scheme.
            // If there is *no* remote authority, then we should use the default scheme for this dialog as that is already local.
            &if has_authority { self.path_service.default_uri_scheme() } else { uri_scheme(&uri) },
        )
    }

    fn get_scheme(&self, available: Option<&[String]>, default_uri: Option<&URI>) -> String {
        if let Some(available) = available.filter(|available| !available.is_empty()) {
            if let Some(default_uri) = default_uri {
                let scheme = uri_scheme(default_uri);
                if available.contains(&scheme) {
                    return scheme;
                }
            }
            return available[0].clone();
        } else if let Some(default_uri) = default_uri {
            return uri_scheme(default_uri);
        }
        schemas::FILE.to_string()
    }

    /// Returns the per-URI authority from `defaultUri` if the dialog should be scoped to a specific authority (e.g.
    /// `agenthost://host/...`).
    ///
    /// Returns `None` when the authority matches the global `remoteAuthority` (standard SSH remotes), since that path is
    /// already handled by the existing logic.
    fn get_scoped_authority(&self, default_uri: Option<&URI>) -> Option<String> {
        let default_uri = default_uri?;
        let authority = default_uri.authority().to_string_lossy();
        if uri_scheme(default_uri) == self.scheme() && !authority.is_empty() && self.remote_authority.as_deref() != Some(authority.as_str()) {
            return Some(authority);
        }
        None
    }

    fn get_remote_agent_environment(&self) -> Promise<Option<RemoteAgentEnvironment>> {
        if let Some(cached) = self.remote_agent_environment.borrow().clone() {
            return Promise::resolved(cached);
        }
        let this = self.rc();
        self.remote_agent_service.get_environment().then(move |environment| {
            *this.remote_agent_environment.borrow_mut() = Some(environment.clone());
            environment
        })
    }

    pub(crate) fn get_user_home(&self, true_home: bool) -> Promise<URI> {
        // When scoped to a custom authority, the platform userHome is not meaningful (it would return a local file://
        // path). Use the root of the scoped filesystem as the home directory instead.
        if let Some(scoped_authority) = self.scoped_authority.borrow().clone() {
            return Promise::resolved(uri_with_parts(&self.scheme(), Some(&scoped_authority), "/", None));
        }
        if true_home {
            if self.scheme() == schemas::FILE { Promise::resolved(self.path_service.local_user_home()) } else { self.path_service.user_home() }
        } else {
            self.file_dialog_service.preferred_home(Some(&self.scheme()))
        }
    }

    fn normalize_uri(&self, uri: &URI) -> URI {
        // Ensures that c: is c:/ since this comes from user input and can be incorrect.
        let uri = ext_uri().add_trailing_path_separator(uri, Some(&self.separator()));
        // To be consistent, we should never have a trailing path separator on directories (or anything else). Will not
        // remove from c:/.
        ext_uri().remove_trailing_path_separator(&uri, None)
    }

    /// Envolve um ouvinte para que ele segure o diálogo por `Weak` (o quick pick é do diálogo, e um `Rc` aqui formaria um
    /// ciclo) e só rode enquanto o diálogo vive.
    fn guarded<T: 'static>(&self, handler: impl Fn(&Rc<Self>, &T) + 'static) -> Box<dyn Fn(&T) + 'static> {
        let weak = self.this.clone();
        Box::new(move |event: &T| {
            if let Some(dialog) = weak.upgrade() {
                handler(&dialog, event);
            }
        })
    }

    /// O `FileQuickPickItem` que o diálogo pôs no quick pick com o `id` do item dado.
    fn file_item_for(&self, item: &IQuickPickItem) -> Option<FileQuickPickItem> {
        let id = item.base.id.as_ref()?;
        self.file_items.borrow().iter().find(|file_item| file_item.item.base.id.as_ref() == Some(id)).cloned()
    }

    /// Os itens que o quick pick mostra agora, como `FileQuickPickItem`.
    fn shown_file_items(&self) -> Vec<FileQuickPickItem> {
        self.pick_box().items().iter().filter_map(|item| item.as_item().and_then(|item| self.file_item_for(item))).collect()
    }

    fn pick_resource(&self, is_save: bool) -> Promise<Option<PickedResources>> {
        let this = self.rc();
        async_fn(async move {
            let (can_select_folders, can_select_files, default_uri) = {
                let options = this.options.borrow();
                (options.can_select_folders.unwrap_or(false), options.can_select_files.unwrap_or(false), options.default_uri.clone())
            };
            this.allow_folder_selection.set(can_select_folders);
            this.allow_file_selection.set(can_select_files);
            this.resolve_path_formatting().await?;
            this.hidden.set(false);
            let mut homedir: URI = default_uri.clone().unwrap_or_else(|| {
                this.workspace_context_service.get_workspace().folders().first().map(|folder| folder.uri.clone()).unwrap_or_else(|| this.user_home.borrow().clone())
            });
            let ext = ext_uri().extname(&homedir);
            if let Some(default_uri) = &default_uri {
                // The file or folder doesn't exist when the stat fails.
                let stat = this.file_service.stat(default_uri).await.ok();
                if !stat.is_some_and(|stat| stat.is_directory) {
                    homedir = ext_uri().dirname(default_uri);
                    *this.trailing.borrow_mut() = Some(ext_uri().basename(default_uri, None));
                }
            }

            let (promise, resolver) = Promise::<Option<PickedResources>>::new();
            let busy_disposable = MutableDisposable::<dyn IDisposable>::new();
            this.store.add_rc(busy_disposable.clone());
            let session = Rc::new(PickSession { is_save, is_resolving: Cell::new(0), is_accept_handled: Cell::new(false), busy_disposable, resolver });

            let pick_box = this.quick_input_service.create_quick_pick(None);
            let pick_box_disposable: DisposableRef = pick_box.clone();
            this.store.add(pick_box_disposable);
            *this.file_pick_box.borrow_mut() = Some(pick_box.clone());
            this.set_busy(true);
            pick_box.set_match_on_label(false);
            pick_box.set_sort_by_label(false);
            pick_box.set_ignore_focus_out(true);
            pick_box.set_placeholder(Some(localize("remoteFileDialog.placeholder", "Folder path", &[])));
            pick_box.set_ok(QuickPickOk::Bool(true));
            let options = this.options.borrow().clone();
            pick_box.set_ok_label(match &options.open_label {
                Some(DialogLabel::Plain(label)) => Some(label.clone()),
                Some(DialogLabel::WithMnemonic { without_mnemonic, .. }) => Some(without_mnemonic.clone()),
                None => None,
            });
            if this.scheme() != schemas::FILE
                && options.available_file_systems.as_ref().is_some_and(|file_systems| file_systems.len() > 1 && file_systems.iter().any(|scheme| scheme == schemas::FILE))
            {
                pick_box.set_custom_button(true);
                pick_box.set_custom_label(Some(localize("remoteFileDialog.local", "Show Local", &[])));
                pick_box.set_custom_button_secondary(Some(true));
                let (action_id, action_label) = if is_save {
                    (save_local_file_command::ID, save_local_file_command::label())
                } else if this.allow_file_selection.get() {
                    if this.allow_folder_selection.get() {
                        (open_local_file_folder_command::ID, open_local_file_folder_command::label())
                    } else {
                        (open_local_file_command::ID, open_local_file_command::label())
                    }
                } else {
                    (open_local_folder_command::ID, open_local_folder_command::label())
                };
                if let Some(label) = this.keybinding_service.lookup_keybinding(action_id, None, false).and_then(|keybinding| keybinding.get_label()).filter(|label| !label.is_empty()) {
                    pick_box.set_custom_hover(Some(format("{0} ({1})", &[action_label.as_str(), label.as_str()])));
                }
            }

            this.set_buttons();
            this.store.add(pick_box.on_did_trigger_button().listen(this.guarded(|dialog, _: &QuickInputButtonRef| {
                dialog.set_show_dot_files(!dialog.show_dot_files.get());
            })));

            *this.current_folder.borrow_mut() = ext_uri().dirname(&homedir);
            *this.user_entered_path_segment.borrow_mut() = String::new();
            *this.auto_complete_path_segment.borrow_mut() = String::new();

            pick_box.set_title(options.title.clone());
            pick_box.set_value(this.path_from_uri(&this.current_folder(), true));
            let value_len = js_len(&pick_box.value());
            this.select_value(value_len, value_len);

            this.store.add(pick_box.on_did_custom().listen(this.guarded({
                let session = session.clone();
                move |dialog, _: &()| {
                    if session.is_accept_handled.get() || dialog.busy() {
                        return;
                    }
                    session.is_accept_handled.set(true);
                    session.is_resolving.set(session.is_resolving.get() + 1);
                    let options = {
                        let mut options = dialog.options.borrow_mut();
                        if let Some(file_systems) = options.available_file_systems.as_mut()
                            && file_systems.len() > 1
                        {
                            file_systems.remove(0);
                        }
                        options.clone()
                    };
                    dialog.pick_box().hide();
                    let (dialog_for_result, session_for_result) = (dialog.clone(), session.clone());
                    if session.is_save {
                        let save_options = SaveDialogOptions {
                            title: options.title,
                            default_uri: options.default_uri,
                            filters: options.filters,
                            save_label: dialog.save_label.borrow().clone(),
                            available_file_systems: options.available_file_systems,
                        };
                        let _ = dialog.file_dialog_service.show_save_dialog(save_options).then(move |result| {
                            dialog_for_result.do_resolve(&session_for_result, result.map(PickedResources::One));
                        });
                    } else {
                        let _ = dialog.file_dialog_service.show_open_dialog(options).then(move |result| {
                            dialog_for_result.do_resolve(&session_for_result, result.map(PickedResources::Many));
                        });
                    }
                }
            })));

            this.store.add(pick_box.on_did_accept().listen(this.guarded({
                let session = session.clone();
                move |dialog, _: &je_platform::quickinput::quick_input::IQuickPickDidAcceptEvent| dialog.handle_accept(&session)
            })));

            this.store.add(pick_box.on_did_change_active().listen(this.guarded({
                let session = session.clone();
                move |dialog, items: &Vec<IQuickPickItem>| {
                    session.is_accept_handled.set(false);
                    // update input box to match the first selected item
                    if items.len() == 1 && dialog.is_selection_change_from_user() {
                        let pick_box = dialog.pick_box();
                        pick_box.set_validation_message(None);
                        let user_path = dialog.construct_full_user_path();
                        let value = pick_box.value();
                        if !equals_ignore_case(&substring(&value, 0, js_len(&user_path)), &user_path) {
                            dialog.select_value(0, js_len(&value));
                            dialog.insert_text(&user_path, &user_path);
                        }
                        if let Some(item) = dialog.file_item_for(&items[0]) {
                            dialog.set_auto_complete(&user_path, &dialog.user_entered_path_segment(), &item, true);
                        }
                    }
                }
            })));

            this.store.add(pick_box.on_did_change_value().listen(this.guarded(|dialog, value: &String| {
                // `async value => this.handleValueChange(value)`: o resultado da promessa não é lido.
                let _ = dialog.handle_value_change(value.clone());
            })));

            this.store.add(pick_box.on_did_hide().listen(this.guarded({
                let session = session.clone();
                move |dialog, _: &je_platform::quickinput::quick_input::IQuickInputHideEvent| {
                    dialog.hidden.set(true);
                    if session.is_resolving.get() == 0 {
                        dialog.do_resolve(&session, None);
                    }
                }
            })));

            pick_box.show();
            this.context_key.set(Dynamic::from(true));
            let trailing = this.trailing.borrow().clone();
            let dialog = this.clone();
            let _ = this.update_items(homedir, true, trailing).then(move |_| {
                let value_len = js_len(&dialog.pick_box().value());
                match dialog.trailing() {
                    Some(trailing) => dialog.select_value(value_len.saturating_sub(js_len(&trailing)), value_len.saturating_sub(js_len(&ext))),
                    None => dialog.select_value(value_len, value_len),
                }
                dialog.set_busy(false);
            });

            promise.await
        })
    }

    /// O `doResolve` de `pickResource`.
    fn do_resolve(&self, session: &PickSession, value: Option<PickedResources>) {
        let value = value.map(|value| match value {
            PickedResources::One(uri) => PickedResources::One(self.normalize_uri(&uri)),
            PickedResources::Many(uris) => PickedResources::Many(uris.iter().map(|uri| self.normalize_uri(uri)).collect()),
        });
        session.resolver.resolve(value);
        self.context_key.set(Dynamic::from(false));
        self.dispose();
    }

    /// O `handleAccept` de `pickResource`.
    fn handle_accept(&self, session: &Rc<PickSession>) {
        if self.busy() {
            // Save the accept until the file picker is not busy.
            let session_for_busy = session.clone();
            let listener = self.on_busy_change_emitter.event().listen(self.guarded(move |dialog, busy: &bool| {
                if !*busy {
                    dialog.handle_accept(&session_for_busy);
                }
            }));
            session.busy_disposable.set(Some(listener));
            return;
        } else if session.is_accept_handled.get() {
            return;
        }

        session.is_accept_handled.set(true);
        session.is_resolving.set(session.is_resolving.get() + 1);
        let (dialog, session) = (self.rc(), session.clone());
        let _ = self.on_did_accept().then(move |resolve_value| {
            if let Some(resolve_value) = resolve_value {
                dialog.pick_box().hide();
                dialog.do_resolve(&session, Some(PickedResources::One(resolve_value)));
            } else if dialog.hidden.get() {
                dialog.do_resolve(&session, None);
            } else {
                session.is_resolving.set(session.is_resolving.get().saturating_sub(1));
                session.is_accept_handled.set(false);
            }
        });
    }

    fn handle_value_change(&self, value: String) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            // Since any text can be entered in the input box, there is potential for error causing input. If this happens,
            // do nothing.
            let _: Result<(), Error> = async {
                // onDidChangeValue can also be triggered by the auto complete, so if it looks like the auto complete, don't do anything
                if this.is_value_change_from_user() {
                    // If the user has just entered more bad path, don't change anything
                    if !equals_ignore_case(&value, &this.construct_full_user_path()) && (!this.is_bad_subpath(&value) || this.can_tilda_escape_hatch(&value)) {
                        this.pick_box().set_validation_message(None);
                        let file_pick_box_uri = this.file_pick_box_value();
                        let mut updated = UpdateResult::NotUpdated;
                        let current_folder = this.current_folder();
                        if !ext_uri().has_trailing_path_separator(&file_pick_box_uri, Some(&this.separator()))
                            && uri_equals_ignore_case(&current_folder, &ext_uri().dirname(&file_pick_box_uri))
                        {
                            this.set_active_items(&value);
                            return Ok(());
                        } else if !uri_equals_ignore_case(&current_folder, &file_pick_box_uri) {
                            updated = this.try_update_items(value.clone(), file_pick_box_uri, false).await?;
                        }
                        if updated == UpdateResult::NotUpdated || updated == UpdateResult::UpdatedWithTrailing {
                            this.set_active_items(&value);
                        }
                    } else {
                        this.pick_box().set_active_items(Vec::new());
                        *this.user_entered_path_segment.borrow_mut() = String::new();
                    }
                }
                Ok(())
            }
            .await;
            Ok(())
        })
    }

    fn set_buttons(&self) {
        let show = self.show_dot_files.get();
        let button = IQuickInputButton {
            icon_class: Some(ThemeIcon::from_id(if show { "eye" } else { "eye-closed" }).as_class_name()),
            tooltip: Some(if show {
                localize("remoteFileDialog.hideDotFiles", "Hide dot files", &[])
            } else {
                localize("remoteFileDialog.showDotFiles", "Show dot files", &[])
            }),
            always_visible: Some(true),
            ..IQuickInputButton::default()
        };
        self.pick_box().set_buttons(vec![Rc::new(button)]);
    }

    fn is_bad_subpath(&self, value: &str) -> bool {
        match self.bad_path.borrow().as_deref() {
            Some(bad_path) => js_len(value) > js_len(bad_path) && equals_ignore_case(&substring(value, 0, js_len(bad_path)), bad_path),
            None => false,
        }
    }

    fn is_value_change_from_user(&self) -> bool {
        let expected = self.path_append(&self.current_folder(), &(self.user_entered_path_segment() + &self.auto_complete_path_segment()));
        !equals_ignore_case(&self.pick_box().value(), &expected)
    }

    fn is_selection_change_from_user(&self) -> bool {
        let first = self.pick_box().active_items().first().and_then(|item| item.base.id.clone());
        *self.active_item.borrow() != first
    }

    fn construct_full_user_path(&self) -> String {
        let current_folder_path = self.path_from_uri(&self.current_folder(), false);
        let value = self.pick_box().value();
        let user_entered = self.user_entered_path_segment();
        if equals_ignore_case(&substring(&value, 0, js_len(&user_entered)), &user_entered) {
            if equals_ignore_case(&substring(&value, 0, js_len(&current_folder_path)), &current_folder_path) { current_folder_path } else { user_entered }
        } else {
            self.path_append(&self.current_folder(), &user_entered)
        }
    }

    fn file_pick_box_value(&self) -> URI {
        // The file pick box can't render everything, so we use the current folder to create the uri so that it is an existing path.
        let value = self.pick_box().value();
        let current_folder = self.current_folder();
        let direct_uri = self.remote_uri_from(value.trim_end(), Some(&current_folder));
        let current_path = self.path_from_uri(&current_folder, false);
        if equals_ignore_case(&value, &current_path) {
            return current_folder;
        }
        let current_display_uri = self.remote_uri_from(&current_path, Some(&current_folder));
        let relative_path = ext_uri().relative_path(&current_display_uri, &direct_uri).filter(|path| !path.is_empty());
        let is_same_root = if js_len(&value) > 1 && js_len(&current_path) > 1 { equals_ignore_case(&substring(&value, 0, 2), &substring(&current_path, 0, 2)) } else { false };
        match relative_path {
            Some(relative_path) if is_same_root => {
                let mut path = join_path(&current_folder, &[relative_path.as_str()]);
                let direct_basename = ext_uri().basename(&direct_uri, None);
                if direct_basename == "." || direct_basename == ".." {
                    path = self.remote_uri_from(&self.path_append(&path, &direct_basename), Some(&current_folder));
                }
                if ext_uri().has_trailing_path_separator(&direct_uri, None) { ext_uri().add_trailing_path_separator(&path, None) } else { path }
            }
            _ => direct_uri,
        }
    }

    fn on_did_accept(&self) -> Promise<Option<URI>> {
        let this = self.rc();
        async_fn(async move {
            this.set_busy(true);
            let pick_box = this.pick_box();
            let updating = this.updating_promise.borrow().is_some();
            if !updating && pick_box.active_items().len() == 1 {
                let item = pick_box.selected_items().first().and_then(|item| this.file_item_for(item));
                if let Some(item) = item
                    && item.is_folder
                {
                    if let Some(trailing) = this.trailing() {
                        this.update_items(item.uri.clone(), true, Some(trailing)).await?;
                    } else {
                        // When possible, cause the update to happen by modifying the input box.
                        // This allows all input box updates to happen first, and uses the same code path as the user typing.
                        let new_path = this.path_from_uri(&item.uri, false);
                        let value = pick_box.value();
                        if starts_with_ignore_case(&new_path, &value) && equals_ignore_case(&item.label(), &ext_uri().basename(&item.uri, None)) {
                            this.select_value(js_len(&this.path_from_uri(&this.current_folder(), false)), js_len(&value));
                            this.insert_text(&new_path, &this.basename_with_trailing_slash(&item.uri));
                        } else if item.label() == ".." && starts_with_ignore_case(&value, &new_path) {
                            this.select_value(js_len(&new_path), js_len(&value));
                            this.insert_text(&new_path, "");
                        } else {
                            this.update_items(item.uri.clone(), true, None).await?;
                        }
                    }
                    this.pick_box().set_busy(false);
                    return Ok(None);
                }
            } else if !updating {
                // If the items have updated, don't try to resolve
                let value = pick_box.value();
                if this.try_update_items(value, this.file_pick_box_value(), false).await? != UpdateResult::NotUpdated {
                    this.pick_box().set_busy(false);
                    return Ok(None);
                }
            }

            // Find resolve value
            let pick_box = this.pick_box();
            let mut resolve_value: Option<URI> = match pick_box.active_items().len() {
                0 => Some(this.file_pick_box_value()),
                1 => pick_box.selected_items().first().and_then(|item| this.file_item_for(item)).map(|item| item.uri),
                _ => None,
            };
            if let Some(uri) = &resolve_value {
                resolve_value = Some(this.add_postfix(uri));
            }
            if this.validate(resolve_value.clone()).await? {
                this.set_busy(false);
                return Ok(resolve_value);
            }
            this.set_busy(false);
            Ok(None)
        })
    }

    fn root(&self, value: &URI) -> URI {
        let mut last_dir = value.clone();
        let mut dir = ext_uri().dirname(value);
        while !uri_equals(&last_dir, &dir) {
            last_dir = dir.clone();
            dir = ext_uri().dirname(&dir);
        }
        dir
    }

    fn can_tilda_escape_hatch(&self, value: &str) -> bool {
        value.ends_with('~') && self.is_bad_subpath(value)
    }

    fn tilda_replace(&self, value: &str) -> URI {
        let home = self.true_home.borrow().clone();
        if value.starts_with('~') {
            join_path(&home, &[&value[1..]])
        } else if self.can_tilda_escape_hatch(value) {
            home
        } else {
            self.remote_uri_from(value, None)
        }
    }

    fn try_add_trailing_separator_to_directory(&self, uri: &URI, is_directory: bool) -> URI {
        if is_directory && !ends_with_slash(&uri.path().to_string_lossy()) {
            // At this point we know it's a directory and can add the trailing path separator
            return ext_uri().add_trailing_path_separator(uri, None);
        }
        uri.clone()
    }

    fn try_update_items(&self, value: String, value_uri: URI, reset: bool) -> Promise<UpdateResult> {
        let this = self.rc();
        async_fn(async move {
            let updated_result = |with_trailing: bool| if with_trailing { UpdateResult::UpdatedWithTrailing } else { UpdateResult::Updated };
            if !value.is_empty() && (value.starts_with('~') || this.can_tilda_escape_hatch(&value)) {
                let new_dir = this.tilda_replace(&value);
                return Ok(updated_result(this.update_items(new_dir, true, None).await?));
            } else if value == "\\" {
                let root = this.root(&this.current_folder());
                return Ok(updated_result(this.update_items(root, true, None).await?));
            }

            let current_folder = this.current_folder();
            let value_dirname = ext_uri().dirname(&value_uri);
            let new_folder_is_old_folder = uri_equals_ignore_case(&current_folder, &value_uri);
            let new_folder_is_sub_folder = uri_equals_ignore_case(&current_folder, &value_dirname);
            let new_folder_is_parent = ext_uri_ignore_path_case().is_equal_or_parent(&current_folder, &value_dirname, false, false);
            let new_folder_is_unrelated = !new_folder_is_parent && !new_folder_is_sub_folder;
            if (!new_folder_is_old_folder && (ends_with_slash(&value) || new_folder_is_parent || new_folder_is_unrelated)) || reset {
                // A missing path is not an error here.
                let stat = this.file_service.stat(&value_uri).await.ok();
                if let Some(stat) = stat.as_ref().filter(|stat| stat.is_directory)
                    && ext_uri().basename(&value_uri, None) != "."
                    && ends_with_slash(&value)
                {
                    let value_uri = this.try_add_trailing_separator_to_directory(&value_uri, stat.is_directory);
                    return Ok(updated_result(this.update_items(value_uri, false, None).await?));
                } else if ends_with_slash(&value) {
                    // The input box contains a path that doesn't exist on the system.
                    this.pick_box().set_validation_message(Some(localize(
                        "remoteFileDialog.badPath",
                        "The path does not exist. Use ~ to go to your home directory.",
                        &[],
                    )));
                    // Save this bad path. It can take too long to a stat on every user entered character, but once a user enters a bad path they are likely
                    // to keep typing more bad path. We can compare against this bad path and see if the user entered path starts with it.
                    *this.bad_path.borrow_mut() = Some(value);
                    return Ok(UpdateResult::InvalidPath);
                } else {
                    let input_uri_dirname = ext_uri().dirname(&value_uri);
                    let without_separator = |uri: &URI| ext_uri().remove_trailing_path_separator(&ext_uri().add_trailing_path_separator(uri, None), None);
                    let pick_box_value = this.pick_box().value();
                    if !uri_equals_ignore_case(&without_separator(&current_folder), &without_separator(&input_uri_dirname))
                        && (!is_drive_letter_only(&pick_box_value)
                            || !equals_ignore_case(&substring(&this.path_from_uri(&current_folder, false), 0, js_len(&pick_box_value)), &pick_box_value))
                    {
                        let stat_without_trailing = this.file_service.stat(&input_uri_dirname).await.ok();
                        if let Some(stat_without_trailing) = stat_without_trailing.filter(|stat| stat.is_directory) {
                            *this.bad_path.borrow_mut() = None;
                            let input_uri_dirname = this.try_add_trailing_separator_to_directory(&input_uri_dirname, stat_without_trailing.is_directory);
                            let updated = this.update_items(input_uri_dirname, false, Some(ext_uri().basename(&value_uri, None))).await?;
                            return Ok(updated_result(updated));
                        }
                    }
                }
            }
            *this.bad_path.borrow_mut() = None;
            Ok(UpdateResult::NotUpdated)
        })
    }

    fn try_update_trailing(&self, value: &URI) {
        let ext = ext_uri().extname(value);
        if self.trailing().is_some() && !ext.is_empty() {
            *self.trailing.borrow_mut() = Some(ext_uri().basename(value, None));
        }
    }

    fn set_active_items(&self, value: &str) {
        let value = self.path_from_uri(&self.tilda_replace(value), false);
        let as_uri = self.remote_uri_from(&value, None);
        let input_basename = ext_uri().basename(&as_uri, None);
        let user_path = self.construct_full_user_path();
        let pick_box = self.pick_box();
        // Make sure that the folder whose children we are currently viewing matches the path in the input
        let paths_equal = equals_ignore_case(&user_path, &substring(&value, 0, js_len(&user_path))) || equals_ignore_case(&value, &substring(&user_path, 0, js_len(&value)));
        if paths_equal {
            let has_match = self.shown_file_items().iter().any(|item| self.set_auto_complete(&value, &input_basename, item, false));
            if !has_match {
                let user_basename = if js_len(&input_basename) >= 2 {
                    let start = (js_len(&user_path) as isize - js_len(&input_basename) as isize + 2).max(0) as usize;
                    substring_from(&user_path, start)
                } else {
                    String::new()
                };
                *self.user_entered_path_segment.borrow_mut() = if user_basename == input_basename { input_basename } else { String::new() };
                *self.auto_complete_path_segment.borrow_mut() = String::new();
                pick_box.set_active_items(Vec::new());
                self.try_update_trailing(&as_uri);
            }
        } else {
            *self.user_entered_path_segment.borrow_mut() = input_basename;
            *self.auto_complete_path_segment.borrow_mut() = String::new();
            pick_box.set_active_items(Vec::new());
            self.try_update_trailing(&as_uri);
        }
    }

    fn set_auto_complete(&self, _starting_value: &str, starting_basename: &str, quick_pick_item: &FileQuickPickItem, force: bool) -> bool {
        if self.busy() {
            // We're in the middle of something else. Doing an auto complete now can result jumbled or incorrect autocompletes.
            *self.user_entered_path_segment.borrow_mut() = starting_basename.to_string();
            *self.auto_complete_path_segment.borrow_mut() = String::new();
            return false;
        }
        let item_basename = quick_pick_item.label();
        // Either force the autocomplete, or the old value should be one smaller than the new value and match the new value.
        if item_basename == ".." {
            // Don't match on the up directory item ever.
            *self.user_entered_path_segment.borrow_mut() = String::new();
            *self.auto_complete_path_segment.borrow_mut() = String::new();
            *self.active_item.borrow_mut() = quick_pick_item.id();
            if force {
                // clear any selected text
                self.exec_insert_text("");
            }
            false
        } else if !force
            && js_len(&item_basename) >= js_len(starting_basename)
            && equals_ignore_case(&substring(&item_basename, 0, js_len(starting_basename)), starting_basename)
        {
            *self.user_entered_path_segment.borrow_mut() = starting_basename.to_string();
            *self.active_item.borrow_mut() = quick_pick_item.id();
            // Changing the active items will trigger the onDidActiveItemsChanged. Clear the autocomplete first, then set it after.
            *self.auto_complete_path_segment.borrow_mut() = String::new();
            if quick_pick_item.is_folder || self.trailing().is_none() {
                self.pick_box().set_active_items(vec![quick_pick_item.item.clone()]);
            } else {
                self.pick_box().set_active_items(Vec::new());
            }
            true
        } else if force && !equals_ignore_case(&self.basename_with_trailing_slash(&quick_pick_item.uri), &(self.user_entered_path_segment() + &self.auto_complete_path_segment())) {
            *self.user_entered_path_segment.borrow_mut() = String::new();
            let screen_reader_optimized = self.accessibility_service.is_screen_reader_optimized();
            if !screen_reader_optimized {
                *self.auto_complete_path_segment.borrow_mut() = trim_trailing_slash(&item_basename);
            }
            *self.active_item.borrow_mut() = quick_pick_item.id();
            if !screen_reader_optimized {
                let pick_box = self.pick_box();
                let current_folder = self.current_folder();
                self.select_value(js_len(&self.path_from_uri(&current_folder, true)), js_len(&pick_box.value()));
                // use insert text to preserve undo buffer
                let auto_complete = self.auto_complete_path_segment();
                self.insert_text(&self.path_append(&current_folder, &auto_complete), &auto_complete);
                let value_len = js_len(&pick_box.value());
                self.select_value(value_len.saturating_sub(js_len(&auto_complete)), value_len);
            }
            true
        } else {
            *self.user_entered_path_segment.borrow_mut() = starting_basename.to_string();
            *self.auto_complete_path_segment.borrow_mut() = String::new();
            false
        }
    }

    /// O `execCommand('insertText', false, text)`: o texto substitui a seleção do campo, o caret vai para depois dele e o
    /// evento de mudança do valor dispara (ver as notas do módulo).
    fn exec_insert_text(&self, text: &str) {
        let pick_box = self.pick_box();
        let value = pick_box.value();
        let len = js_len(&value);
        let (start, end) = pick_box.value_selection().map(|(start, end)| (start as usize, end as usize)).unwrap_or((len, len));
        let (start, end) = (start.min(len), end.min(len));
        let (start, end) = if start > end { (end, start) } else { (start, end) };
        pick_box.set_value(format!("{}{}{}", substring(&value, 0, start), text, substring_from(&value, end)));
        let caret = start + js_len(text);
        self.select_value(caret, caret);
    }

    fn insert_text(&self, whole_value: &str, insert_text: &str) {
        let pick_box = self.pick_box();
        if pick_box.input_has_focus() {
            self.exec_insert_text(insert_text);
            if pick_box.value() != whole_value {
                pick_box.set_value(whole_value.to_string());
                let _ = self.handle_value_change(whole_value.to_string());
            }
        } else {
            pick_box.set_value(whole_value.to_string());
            let _ = self.handle_value_change(whole_value.to_string());
        }
    }

    fn add_postfix(&self, uri: &URI) -> URI {
        let options = self.options.borrow();
        let Some(filters) = options.filters.as_ref().filter(|filters| !filters.is_empty()) else {
            return uri.clone();
        };
        if !self.requires_trailing.get() || ext_uri().has_trailing_path_separator(uri, None) {
            return uri.clone();
        }
        // Make sure that the suffix is added. If the user deleted it, we automatically add it here
        let current_ext = substring_from(&ext_uri().extname(uri), 1);
        let has_ext = filters.iter().any(|filter| filter.extensions.iter().any(|extension| extension == "*" || *extension == current_ext));
        if has_ext {
            return uri.clone();
        }
        match filters[0].extensions.first() {
            Some(extension) => join_path(&ext_uri().dirname(uri), &[format!("{}.{}", ext_uri().basename(uri, None), extension).as_str()]),
            None => uri.clone(),
        }
    }

    fn yes_no_prompt(&self, uri: &URI, message: String) -> Promise<bool> {
        let this = self.rc();
        let store = DisposableStore::new();
        let prompt = this.quick_input_service.create_quick_pick(None);
        let prompt_disposable: DisposableRef = prompt.clone();
        store.add(prompt_disposable);
        prompt.set_title(Some(message));
        prompt.set_ignore_focus_out(true);
        prompt.set_ok(QuickPickOk::Bool(true));
        prompt.set_custom_button(true);
        prompt.set_custom_label(Some(localize("remoteFileDialog.cancel", "Cancel", &[])));
        prompt.set_custom_button_secondary(Some(true));
        prompt.set_value(this.path_from_uri(uri, false));

        let is_resolving = Rc::new(Cell::new(false));
        let (promise, resolver) = Promise::<bool>::new();
        store.add(prompt.on_did_accept().listen({
            let (is_resolving, resolver, prompt) = (is_resolving.clone(), resolver.clone(), Rc::downgrade(&prompt));
            move |_| {
                is_resolving.set(true);
                if let Some(prompt) = prompt.upgrade() {
                    prompt.hide();
                }
                resolver.resolve(true);
            }
        }));
        store.add(prompt.on_did_hide().listen(this.guarded({
            let (is_resolving, resolver, store) = (is_resolving.clone(), resolver.clone(), store.clone());
            move |dialog, _: &je_platform::quickinput::quick_input::IQuickInputHideEvent| {
                if !is_resolving.get() {
                    resolver.resolve(false);
                    let pick_box = dialog.pick_box();
                    pick_box.show();
                    // The quick pick UI's list is shared between quick picks, so showing the yes/no prompt above replaced
                    // the items in the underlying list. Re-assign the items so they are rendered again when the file
                    // picker is shown.
                    let current_items = pick_box.items();
                    pick_box.set_items(current_items);
                }
                dialog.hidden.set(false);
                store.dispose();
            }
        })));
        store.add(prompt.on_did_change_value().listen({
            let prompt = Rc::downgrade(&prompt);
            move |_| {
                if let Some(prompt) = prompt.upgrade() {
                    prompt.hide();
                }
            }
        }));
        store.add(prompt.on_did_custom().listen({
            let prompt = Rc::downgrade(&prompt);
            move |_| {
                if let Some(prompt) = prompt.upgrade() {
                    prompt.hide();
                }
            }
        }));
        prompt.show();
        promise
    }

    fn validate(&self, uri: Option<URI>) -> Promise<bool> {
        let this = self.rc();
        async_fn(async move {
            let Some(uri) = uri else {
                this.pick_box().set_validation_message(Some(localize("remoteFileDialog.invalidPath", "Please enter a valid path.", &[])));
                return Ok(false);
            };

            // A granted workspace root can be accessible even when its parent is not.
            let mut settled = Promise::all_settled(vec![this.file_service.stat(&ext_uri().dirname(&uri)), this.file_service.stat(&uri)]).await?.into_iter();
            let stat_dirname: Option<FileStatWithMetadata> = settled.next().and_then(|result| result.ok());
            let stat: Option<FileStatWithMetadata> = settled.next().and_then(|result| result.ok());
            let pick_box = this.pick_box();
            let is_windows = this.is_windows.get();
            let basename = ext_uri().basename(&uri, None);

            if this.requires_trailing.get() {
                // save
                if stat.as_ref().is_some_and(|stat| stat.is_directory) {
                    // Can't do this
                    pick_box.set_validation_message(Some(localize("remoteFileDialog.validateFolder", "The folder already exists. Please use a new file name.", &[])));
                    return Ok(false);
                } else if stat.is_some() {
                    // Replacing a file. Show a yes/no prompt
                    let message = localize("remoteFileDialog.validateExisting", "{0} already exists. Are you sure you want to overwrite it?", &[basename.as_str()]);
                    return this.yes_no_prompt(&uri, message).await;
                } else if !is_valid_basename(Some(&basename), is_windows) {
                    // Filename not allowed
                    pick_box.set_validation_message(Some(localize("remoteFileDialog.validateBadFilename", "Please enter a valid file name.", &[])));
                    return Ok(false);
                } else if let Some(stat_dirname) = &stat_dirname {
                    if !stat_dirname.is_directory {
                        pick_box.set_validation_message(Some(localize("remoteFileDialog.validateNonexistentDir", "Please enter a path that exists.", &[])));
                        return Ok(false);
                    } else if stat_dirname.readonly() {
                        pick_box.set_validation_message(Some(localize(
                            "remoteFileDialog.validateReadonlyFolder",
                            "This folder cannot be used as a save destination. Please choose another folder",
                            &[],
                        )));
                        return Ok(false);
                    }
                } else {
                    // Folder to save in doesn't exist
                    let message = localize(
                        "remoteFileDialog.validateCreateDirectory",
                        "The folder {0} does not exist. Would you like to create it?",
                        &[ext_uri().basename(&ext_uri().dirname(&uri), None).as_str()],
                    );
                    return this.yes_no_prompt(&uri, message).await;
                }
            } else {
                // open
                let allow_folder_selection = this.allow_folder_selection.get();
                let allow_file_selection = this.allow_file_selection.get();
                match &stat {
                    None => {
                        // For a folder-only picker, offer to create the folder if a writable ancestor exists.
                        if allow_folder_selection && !allow_file_selection && this.can_create_folder(&uri, stat_dirname.as_ref()).await? {
                            let message = localize(
                                "remoteFileDialog.validateCreateDirectoryOpen",
                                "The folder {0} does not exist. Would you like to create it?",
                                &[basename.as_str()],
                            );
                            if !this.yes_no_prompt(&uri, message).await? {
                                return Ok(false);
                            }
                            return match this.file_service.create_folder(&uri).await {
                                Ok(_) => Ok(true),
                                Err(error) => {
                                    pick_box.set_validation_message(Some(localize("remoteFileDialog.createFolderFailed", "Could not create folder: {0}", &[error.message.as_str()])));
                                    Ok(false)
                                }
                            };
                        }
                        // File or folder doesn't exist
                        pick_box.set_validation_message(Some(localize("remoteFileDialog.validateNonexistentDir", "Please enter a path that exists.", &[])));
                        return Ok(false);
                    }
                    Some(_) if uri.path().to_string_lossy() == "/" && is_windows => {
                        pick_box.set_validation_message(Some(localize("remoteFileDialog.windowsDriveLetter", "Please start the path with a drive letter.", &[])));
                        return Ok(false);
                    }
                    Some(stat) if stat.is_directory && !allow_folder_selection => {
                        // Folder selected when folder selection not permitted
                        pick_box.set_validation_message(Some(localize("remoteFileDialog.validateFileOnly", "Please select a file.", &[])));
                        return Ok(false);
                    }
                    Some(stat) if !stat.is_directory && !allow_file_selection => {
                        // File selected when file selection not permitted
                        pick_box.set_validation_message(Some(localize("remoteFileDialog.validateFolderOnly", "Please select a folder.", &[])));
                        return Ok(false);
                    }
                    Some(_) => {}
                }
            }
            Ok(true)
        })
    }

    fn can_create_folder(&self, uri: &URI, parent_stat: Option<&FileStatWithMetadata>) -> Promise<bool> {
        let this = self.rc();
        let uri = uri.clone();
        let parent_stat = parent_stat.cloned();
        async_fn(async move {
            let immediate_parent = ext_uri().dirname(&uri);
            let mut candidate = uri;
            loop {
                let name = ext_uri().basename(&candidate, None);
                if name.is_empty() || !is_valid_basename(Some(&name), this.is_windows.get()) {
                    return Ok(false);
                }

                let parent = ext_uri().dirname(&candidate);
                if uri_equals(&parent, &candidate) {
                    return Ok(false);
                }

                let stat = match &parent_stat {
                    Some(parent_stat) if uri_equals(&parent, &immediate_parent) => Ok(parent_stat.clone()),
                    _ => this.file_service.stat(&parent).await,
                };
                match stat {
                    Ok(stat) => return Ok(stat.is_directory && !stat.readonly()),
                    Err(error) => {
                        if to_file_system_provider_error_code(Some(&error)) != FileSystemProviderErrorCode::FileNotFound {
                            return Ok(false);
                        }
                        candidate = parent;
                    }
                }
            }
        })
    }

    // Returns true if there is a file at the end of the URI.
    fn update_items(&self, new_folder: URI, force: bool, trailing: Option<String>) -> Promise<bool> {
        let this = self.rc();
        this.set_busy(true);
        *this.auto_complete_path_segment.borrow_mut() = String::new();
        let was_dot_dot = trailing.as_deref() == Some("..");
        let trailing = if was_dot_dot { None } else { trailing.filter(|trailing| !trailing.is_empty()) };

        let updating_promise = {
            let this = this.clone();
            create_cancelable_promise(move |token| this.run_update_items(new_folder, force, trailing, was_dot_dot, token))
        };

        let previous = this.updating_promise.borrow_mut().take();
        if let Some(previous) = previous {
            previous.cancel();
        }
        if !updating_promise.is_settled() {
            *this.updating_promise.borrow_mut() = Some(updating_promise.clone());
        }
        updating_promise.promise.clone()
    }

    /// O corpo da promessa cancelável de `updateItems`.
    fn run_update_items(&self, new_folder: URI, force: bool, trailing: Option<String>, was_dot_dot: bool, token: CancellationToken) -> Promise<bool> {
        let this = self.rc();
        async_fn(async move {
            let (mut new_folder, mut trailing) = (new_folder, trailing);
            let is_save = trailing.is_some();
            let mut result = false;
            let mut folder_stat: Option<FileStat> = None;
            // The file/directory doesn't exist when the resolve fails.
            if let Ok(stat) = this.file_service.resolve(&new_folder, None).await {
                if stat.is_directory {
                    folder_stat = Some(stat);
                } else {
                    trailing = Some(ext_uri().basename(&new_folder, None));
                    new_folder = ext_uri().dirname(&new_folder);
                    result = true;
                }
            }
            let new_value = match &trailing {
                Some(trailing) => this.path_append(&new_folder, trailing),
                None => this.path_from_uri(&new_folder, true),
            };
            let current_folder = if ends_with_slash(&new_folder.path().to_string_lossy()) {
                new_folder.clone()
            } else {
                ext_uri().add_trailing_path_separator(&new_folder, Some(&this.separator()))
            };
            let user_entered_path_segment = trailing.clone().unwrap_or_default();

            let items = this.create_items(folder_stat, &current_folder, &token).await?;
            if token.is_cancellation_requested() {
                this.set_busy(false);
                return Ok(false);
            }

            *this.current_folder.borrow_mut() = current_folder;
            *this.user_entered_path_segment.borrow_mut() = user_entered_path_segment;
            let pick_box = this.pick_box();
            pick_box.set_item_activation(ItemActivation::None);
            *this.file_items.borrow_mut() = items.clone();
            pick_box.set_items(items.into_iter().map(|item| QuickPickInput::Item(item.item)).collect());

            // the user might have continued typing while we were updating. Only update the input box if it doesn't match the directory.
            let value = pick_box.value();
            if !equals_ignore_case(&value, &new_value) && (force || was_dot_dot) {
                this.select_value(0, js_len(&value));
                this.insert_text(&new_value, &new_value);
            }
            let value_len = js_len(&pick_box.value());
            match &trailing {
                // Keep the cursor position in front of the save as name.
                Some(trailing) if force && is_save => this.select_value(value_len.saturating_sub(js_len(trailing)), value_len.saturating_sub(js_len(trailing))),
                // If there is trailing, we don't move the cursor. If there is no trailing, cursor goes at the end.
                None => this.select_value(value_len, value_len),
                Some(_) => {}
            }
            this.set_busy(false);
            *this.updating_promise.borrow_mut() = None;
            Ok(result)
        })
    }

    fn resolve_path_formatting(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let scoped_authority = this.scoped_authority.borrow().clone();
            if let Some(scoped_authority) = scoped_authority {
                let resource = uri_with_parts(&this.scheme(), Some(&scoped_authority), "/", None);
                let operating_system = this.path_service.get_operating_system(&resource).await?;
                let path = this.path_service.get_path(&resource).await?;
                this.is_windows.set(operating_system == Some(OperatingSystem::Windows));
                *this.separator.borrow_mut() = path.map_or("/", path_separator).to_string();
                return Ok(());
            }

            *this.separator.borrow_mut() = this.label_service.get_separator(&this.scheme(), this.remote_authority.as_deref()).as_str().to_string();
            let is_windows = this.check_is_windows_os().await?;
            this.is_windows.set(is_windows);
            Ok(())
        })
    }

    fn path_from_uri(&self, uri: &URI, end_with_separator: bool) -> String {
        let is_windows = self.is_windows.get();
        let mut result = if self.scoped_authority.borrow().is_some() {
            let mut path = uri.path().to_string_lossy().replace('\n', "");
            let bytes = path.as_bytes();
            if is_windows && bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
                path = path[1..].to_string();
            }
            normalize_drive_letter(&path, is_windows)
        } else {
            normalize_drive_letter(&fs_path(uri), is_windows).replace('\n', "")
        };
        let separator = self.separator();
        result = if separator == "/" { result.replace('\\', &separator) } else { result.replace('/', &separator) };
        if end_with_separator && !ends_with_slash(&result) {
            result.push_str(&separator);
        }
        result
    }

    fn path_append(&self, uri: &URI, additional: &str) -> String {
        if additional == ".." || additional == "." {
            let base_path = self.path_from_uri(uri, true);
            format!("{base_path}{additional}")
        } else {
            self.path_from_uri(&join_path(uri, &[additional]), false)
        }
    }

    fn check_is_windows_os(&self) -> Promise<bool> {
        self.get_remote_agent_environment().then(|environment| match environment {
            Some(environment) => environment.os == OperatingSystem::Windows,
            None => cfg!(windows),
        })
    }

    fn basename_with_trailing_slash(&self, full_path: &URI) -> String {
        let child = self.path_from_uri(full_path, true);
        let parent = self.path_from_uri(&ext_uri().dirname(full_path), true);
        substring_from(&child, js_len(&parent))
    }

    fn create_back_item(&self, curr_folder: &URI) -> Promise<Option<FileQuickPickItem>> {
        let this = self.rc();
        let curr_folder = curr_folder.clone();
        async_fn(async move {
            // For authority-scoped URIs, compare within the original scheme so that the authority is preserved and the
            // root is detected correctly.
            let scoped_authority = this.scoped_authority.borrow().clone();
            let (compare_scheme, compare_authority) = match scoped_authority {
                Some(authority) => (this.scheme(), authority),
                None => (schemas::FILE.to_string(), String::new()),
            };
            let file_representation_curr =
                curr_folder.with(&UriChange { scheme: Field::from(compare_scheme.as_str()), authority: Field::from(compare_authority.as_str()), ..UriChange::default() });
            let file_representation_parent = ext_uri().dirname(&file_representation_curr);
            if !uri_equals(&file_representation_curr, &file_representation_parent) {
                let parent_folder = ext_uri().dirname(&curr_folder);
                if this.file_service.exists(&parent_folder).await? {
                    let uri = ext_uri().add_trailing_path_separator(&parent_folder, Some(&this.separator()));
                    return Ok(Some(FileQuickPickItem::new("..", uri, true, None)));
                }
            }
            Ok(None)
        })
    }

    fn create_items(&self, folder: Option<FileStat>, current_folder: &URI, token: &CancellationToken) -> Promise<Vec<FileQuickPickItem>> {
        let this = self.rc();
        let (current_folder, token) = (current_folder.clone(), token.clone());
        async_fn(async move {
            let back_dir = this.create_back_item(&current_folder).await?;
            let listed: Result<Vec<FileQuickPickItem>, Error> = async {
                let folder = match folder {
                    Some(folder) => folder,
                    None => this.file_service.resolve(&current_folder, None).await?,
                };
                let children = folder.children.unwrap_or_default();
                let show_dot_files = this.show_dot_files.get();
                Ok(children
                    .iter()
                    .filter(|child| show_dot_files || !child.name.starts_with('.'))
                    .filter_map(|child| this.create_item(child, &current_folder, &token))
                    .collect())
            }
            .await;
            // The folder can't be read: show only the back item (if any).
            let mut result = listed.unwrap_or_default();
            if token.is_cancellation_requested() {
                return Ok(Vec::new());
            }
            let trim = |label: String| if ends_with_slash(&label) { substring(&label, 0, js_len(&label) - 1) } else { label };
            result.sort_by(|i1, i2| {
                if i1.is_folder != i2.is_folder {
                    return if i1.is_folder { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
                }
                locale_compare(&trim(i1.label()), &trim(i2.label()))
            });

            if let Some(back_dir) = back_dir {
                result.insert(0, back_dir);
            }
            Ok(result)
        })
    }

    fn filter_file(&self, file: &URI) -> bool {
        let options = self.options.borrow();
        match options.filters.as_ref() {
            Some(filters) => {
                let path = file.path().to_string_lossy();
                filters.iter().any(|filter| filter.extensions.iter().any(|test_ext| test_ext == "*" || path.ends_with(&format!(".{test_ext}"))))
            }
            None => true,
        }
    }

    fn create_item(&self, stat: &FileStat, parent: &URI, token: &CancellationToken) -> Option<FileQuickPickItem> {
        if token.is_cancellation_requested() {
            return None;
        }
        let mut full_path = join_path(parent, &[stat.name.as_str()]);
        if stat.is_directory {
            let filename = ext_uri().basename(&full_path, None);
            full_path = ext_uri().add_trailing_path_separator(&full_path, Some(&self.separator()));
            let icon_classes = get_icon_classes(&*self.model_service, &*self.language_service, Some(&full_path), Some(FileKind::Folder), None);
            Some(FileQuickPickItem::new(&filename, full_path, true, Some(icon_classes)))
        } else if self.allow_file_selection.get() && self.filter_file(&full_path) {
            let icon_classes = get_icon_classes(&*self.model_service, &*self.language_service, Some(&full_path), None, None);
            Some(FileQuickPickItem::new(&stat.name, full_path, false, Some(icon_classes)))
        } else {
            None
        }
    }
}
