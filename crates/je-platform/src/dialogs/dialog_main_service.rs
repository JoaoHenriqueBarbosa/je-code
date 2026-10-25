//! Porte de `src/vs/platform/dialogs/electron-main/dialogMainService.ts`: o `DialogMainService`, que enfileira os diálogos
//! nativos por janela (um depois do outro), impede o mesmo diálogo de arquivo de abrir duas vezes na mesma janela e
//! normaliza os caminhos devolvidos no macOS.
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único. Decisões de tradução:
//! - **`electron.dialog`** e **`electron.BrowserWindow.getFocusedWindow()`** são o trait [`NativeDialogs`], que as interfaces
//!   nativas implementam desenhando o diálogo. Os tipos do Electron que o serviço lê (`MessageBoxOptions`,
//!   `MessageBoxReturnValue`, `SaveDialogOptions`, `SaveDialogReturnValue`, `OpenDialogOptions`, `OpenDialogReturnValue`) são
//!   as structs deste módulo, só com os campos que o VS Code preenche: ficam de fora o `icon` (imagem nativa), o `signal`
//!   (`AbortSignal`) e os `securityScopedBookmarks` (só macOS App Store).
//! - **`electron.BrowserWindow`** é [`BrowserWindow`]: o serviço só lê o `id`.
//! - **`Queue<MessageBoxReturnValue | SaveDialogReturnValue | OpenDialogReturnValue>`** é `Queue<DialogResult>`, com
//!   [`DialogResult`] sendo a união dos três; o `as unknown as Queue<T>` do original é a conversão `TryFrom` de cada chamador.
//! - **`getWindowDialogQueue` e `windowDialogQueues`**, privados no original (o teste os alcança por um `as unknown as`),
//!   são [`DialogMainService::get_window_dialog_queue`] e [`DialogMainService::window_dialog_queue_count`], públicos pelo
//!   mesmo motivo.
//! - **`hash(options)`** do bloqueio de diálogos duplicados recebe as opções como [`Dynamic`] (só os campos preenchidos).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::async_::{Queue, async_fn};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::hash::hash;
use je_base::labels::mnemonic_button_label;
use je_base::lifecycle::{self, DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::node::pfs;
use je_base::normalization::normalize_nfc;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_instantiation::service_id;

use super::dialog_main_utils::massage_message_box_options;
use super::dialogs::{FileFilter, NativeOpenDialogOptions};
use crate::log::{LogArg, LogServiceRef, Logger};
use crate::product::IProductService;
use crate::workspace::workspace_filter;

/// `electron.BrowserWindow`, só com o `id` que o serviço lê.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BrowserWindow {
    pub id: u32,
}

/// `electron.MessageBoxOptions.type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageBoxType {
    None,
    Info,
    Error,
    Question,
    Warning,
}

/// `electron.MessageBoxOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageBoxOptions {
    /// `type`.
    pub message_box_type: Option<MessageBoxType>,
    pub buttons: Vec<String>,
    pub default_id: Option<i32>,
    pub title: Option<String>,
    pub message: String,
    pub detail: Option<String>,
    pub checkbox_label: Option<String>,
    pub checkbox_checked: Option<bool>,
    pub text_width: Option<u32>,
    pub cancel_id: Option<i32>,
    pub no_link: Option<bool>,
    pub normalize_access_keys: Option<bool>,
}

/// `electron.MessageBoxReturnValue`: o índice do botão apertado e o estado da caixa de seleção.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageBoxReturnValue {
    pub response: usize,
    pub checkbox_checked: bool,
}

/// Os itens de `electron.OpenDialogOptions.properties`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenDialogProperty {
    OpenFile,
    OpenDirectory,
    MultiSelections,
    ShowHiddenFiles,
    CreateDirectory,
    PromptToCreate,
    NoResolveAliases,
    TreatPackageAsDirectory,
    DontAddToRecent,
}

impl OpenDialogProperty {
    /// O texto do Electron (`'openFile'`, `'multiSelections'`...).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenFile => "openFile",
            Self::OpenDirectory => "openDirectory",
            Self::MultiSelections => "multiSelections",
            Self::ShowHiddenFiles => "showHiddenFiles",
            Self::CreateDirectory => "createDirectory",
            Self::PromptToCreate => "promptToCreate",
            Self::NoResolveAliases => "noResolveAliases",
            Self::TreatPackageAsDirectory => "treatPackageAsDirectory",
            Self::DontAddToRecent => "dontAddToRecent",
        }
    }
}

/// `electron.OpenDialogOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenDialogOptions {
    pub title: Option<String>,
    pub default_path: Option<String>,
    pub button_label: Option<String>,
    pub filters: Option<Vec<FileFilter>>,
    pub properties: Option<Vec<OpenDialogProperty>>,
    pub message: Option<String>,
}

/// `electron.OpenDialogReturnValue`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenDialogReturnValue {
    pub canceled: bool,
    pub file_paths: Vec<String>,
}

/// `electron.SaveDialogOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveDialogOptions {
    pub title: Option<String>,
    pub default_path: Option<String>,
    pub button_label: Option<String>,
    pub filters: Option<Vec<FileFilter>>,
    pub message: Option<String>,
    pub name_field_label: Option<String>,
    pub shows_tag_field: Option<bool>,
}

/// `electron.SaveDialogReturnValue`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveDialogReturnValue {
    pub canceled: bool,
    pub file_path: String,
}

/// Um objeto dinâmico só com os campos preenchidos (o `undefined` do original não entra).
fn dynamic_object(entries: Vec<(&'static str, Option<Dynamic>)>) -> Dynamic {
    Dynamic::object(entries.into_iter().filter_map(|(key, value)| value.map(|value| (key, value))))
}

fn text(value: &Option<String>) -> Option<Dynamic> {
    value.as_deref().map(Dynamic::string)
}

fn filters_to_dynamic(filters: &Option<Vec<FileFilter>>) -> Option<Dynamic> {
    filters.as_ref().map(|filters| {
        Dynamic::array(filters.iter().map(|filter| {
            Dynamic::object(vec![("name", Dynamic::string(&filter.name)), ("extensions", Dynamic::array(filter.extensions.iter().map(|extension| Dynamic::string(extension))))])
        }))
    })
}

impl OpenDialogOptions {
    /// As opções como valor dinâmico: o que o `hash(options)` e o log do bloqueio de diálogos duplicados recebem.
    pub fn to_dynamic(&self) -> Dynamic {
        dynamic_object(vec![
            ("title", text(&self.title)),
            ("defaultPath", text(&self.default_path)),
            ("buttonLabel", text(&self.button_label)),
            ("filters", filters_to_dynamic(&self.filters)),
            ("properties", self.properties.as_ref().map(|properties| Dynamic::array(properties.iter().map(|property| Dynamic::string(property.as_str()))))),
            ("message", text(&self.message)),
        ])
    }
}

impl SaveDialogOptions {
    /// Como [`OpenDialogOptions::to_dynamic`].
    pub fn to_dynamic(&self) -> Dynamic {
        dynamic_object(vec![
            ("title", text(&self.title)),
            ("defaultPath", text(&self.default_path)),
            ("buttonLabel", text(&self.button_label)),
            ("filters", filters_to_dynamic(&self.filters)),
            ("message", text(&self.message)),
            ("nameFieldLabel", text(&self.name_field_label)),
            ("showsTagField", self.shows_tag_field.map(Dynamic::Bool)),
        ])
    }
}

/// A união `MessageBoxReturnValue | SaveDialogReturnValue | OpenDialogReturnValue` das filas de diálogo, com a conversão de
/// ida (`From`) e de volta (`TryFrom`) de cada variante.
macro_rules! dialog_result_variants {
    ($($variant:ident($ty:ty)),+ $(,)?) => {
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum DialogResult {
            $($variant($ty)),+
        }

        $(
            impl From<$ty> for DialogResult {
                fn from(value: $ty) -> Self {
                    Self::$variant(value)
                }
            }

            impl TryFrom<DialogResult> for $ty {
                type Error = Error;

                fn try_from(result: DialogResult) -> Result<Self, Error> {
                    match result {
                        DialogResult::$variant(value) => Ok(value),
                        other => Err(Error::new(format!("O resultado do diálogo não é {}: {other:?}", stringify!($variant)))),
                    }
                }
            }
        )+
    };
}

dialog_result_variants!(MessageBox(MessageBoxReturnValue), SaveDialog(SaveDialogReturnValue), OpenDialog(OpenDialogReturnValue));

/// `electron.dialog` e `electron.BrowserWindow.getFocusedWindow()`: o desenho dos diálogos nativos.
pub trait NativeDialogs {
    /// `electron.BrowserWindow.getFocusedWindow()`.
    fn get_focused_window(&self) -> Option<BrowserWindow>;
    /// `electron.dialog.showMessageBox([window,] options)`.
    fn show_message_box(&self, window: Option<BrowserWindow>, options: MessageBoxOptions) -> Promise<MessageBoxReturnValue>;
    /// `electron.dialog.showSaveDialog([window,] options)`.
    fn show_save_dialog(&self, window: Option<BrowserWindow>, options: SaveDialogOptions) -> Promise<SaveDialogReturnValue>;
    /// `electron.dialog.showOpenDialog([window,] options)`.
    fn show_open_dialog(&self, window: Option<BrowserWindow>, options: OpenDialogOptions) -> Promise<OpenDialogReturnValue>;
}

/// `IDialogMainService`.
pub trait IDialogMainService {
    fn pick_file_folder(&self, options: NativeOpenDialogOptions, window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>>;
    fn pick_folder(&self, options: NativeOpenDialogOptions, window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>>;
    fn pick_file(&self, options: NativeOpenDialogOptions, window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>>;
    fn pick_workspace(&self, options: NativeOpenDialogOptions, window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>>;

    fn show_message_box(&self, options: MessageBoxOptions, window: Option<BrowserWindow>) -> Promise<MessageBoxReturnValue>;
    fn show_save_dialog(&self, options: SaveDialogOptions, window: Option<BrowserWindow>) -> Promise<SaveDialogReturnValue>;
    fn show_open_dialog(&self, options: OpenDialogOptions, window: Option<BrowserWindow>) -> Promise<OpenDialogReturnValue>;
}

service_id!(pub I_DIALOG_MAIN_SERVICE: dyn IDialogMainService = "dialogMainService");

/// `IInternalNativeOpenDialogOptions`.
#[derive(Clone, Default)]
struct InternalNativeOpenDialogOptions {
    /// O `INativeOpenDialogOptions` que o original espalha (`...options`).
    base: NativeOpenDialogOptions,
    pick_folders: Option<bool>,
    pick_files: Option<bool>,
    title: String,
    button_label: Option<String>,
    filters: Option<Vec<FileFilter>>,
}

/// `normalizePath`: no macOS, normaliza o caminho para NFC.
fn normalize_path(host: &Platform, path: &str) -> String {
    if !path.is_empty() && host.is_macintosh {
        let units: Vec<u16> = path.encode_utf16().collect();
        return normalize_nfc(&units).to_string_lossy();
    }

    path.to_owned()
}

/// `DialogMainService`. Tudo é compartilhado por contagem de referências, então o serviço é barato de clonar e os
/// trabalhos assíncronos levam o próprio clone (o `this` do original).
#[derive(Clone)]
pub struct DialogMainService {
    log_service: LogServiceRef,
    product_service: Rc<dyn IProductService>,
    native_dialogs: Rc<dyn NativeDialogs>,
    host: Rc<Platform>,
    window_file_dialog_locks: Rc<RefCell<HashMap<u32, HashSet<i32>>>>,
    window_dialog_queues: Rc<RefCell<HashMap<u32, Queue<DialogResult>>>>,
    no_window_dialog_queue: Queue<DialogResult>,
}

impl DialogMainService {
    pub fn new(log_service: LogServiceRef, product_service: Rc<dyn IProductService>, native_dialogs: Rc<dyn NativeDialogs>) -> Self {
        Self {
            log_service,
            product_service,
            native_dialogs,
            host: Rc::new(Platform::detect()),
            window_file_dialog_locks: Rc::default(),
            window_dialog_queues: Rc::default(),
            no_window_dialog_queue: Queue::new(),
        }
    }

    fn do_pick(&self, options: InternalNativeOpenDialogOptions, window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>> {
        // Ensure dialog options
        let mut dialog_options = OpenDialogOptions {
            title: Some(options.title.clone()),
            button_label: options.button_label.clone(),
            filters: options.filters.clone(),
            default_path: options.base.default_path.clone(),
            ..OpenDialogOptions::default()
        };

        // Ensure properties
        if options.pick_files.is_some() || options.pick_folders.is_some() {
            dialog_options.properties = None; // let it override based on the booleans

            if options.pick_files == Some(true) && options.pick_folders == Some(true) {
                dialog_options.properties = Some(vec![
                    OpenDialogProperty::MultiSelections,
                    OpenDialogProperty::OpenDirectory,
                    OpenDialogProperty::OpenFile,
                    OpenDialogProperty::CreateDirectory,
                ]);
            }
        }

        let mut properties = dialog_options.properties.take().unwrap_or_else(|| {
            vec![
                OpenDialogProperty::MultiSelections,
                if options.pick_folders == Some(true) { OpenDialogProperty::OpenDirectory } else { OpenDialogProperty::OpenFile },
                OpenDialogProperty::CreateDirectory,
            ]
        });

        if self.host.is_macintosh {
            properties.push(OpenDialogProperty::TreatPackageAsDirectory); // always drill into .app files
        }
        dialog_options.properties = Some(properties);

        // Show Dialog
        let window = window.or_else(|| self.native_dialogs.get_focused_window());
        self.show_open_dialog(dialog_options, window).then(|result| if result.file_paths.is_empty() { None } else { Some(result.file_paths) })
    }

    /// Queue message box requests per window so that one can show after the other.
    pub fn get_window_dialog_queue(&self, window: Option<BrowserWindow>) -> Queue<DialogResult> {
        let Some(window) = window else {
            return self.no_window_dialog_queue.clone();
        };

        let window_id = window.id;
        if let Some(queue) = self.window_dialog_queues.borrow().get(&window_id) {
            return queue.clone();
        }

        let queue = Queue::new();
        self.window_dialog_queues.borrow_mut().insert(window_id, queue.clone());
        let window_dialog_queues = self.window_dialog_queues.clone();
        queue.on_drained().once().listen(move |_| {
            window_dialog_queues.borrow_mut().remove(&window_id);
        });

        queue
    }

    /// Quantas filas por janela existem agora (o `windowDialogQueues.size` que o teste do original lê).
    pub fn window_dialog_queue_count(&self) -> usize {
        self.window_dialog_queues.borrow().len()
    }

    /// `getWindowDialogQueue<T>(window).queue(run)`: roda `run` na fila da janela e devolve o resultado tipado.
    fn queue_dialog<T>(&self, window: Option<BrowserWindow>, run: impl FnOnce() -> Promise<T> + 'static) -> Promise<T>
    where
        T: Clone + 'static + Into<DialogResult> + TryFrom<DialogResult, Error = Error>,
    {
        self.get_window_dialog_queue(window).queue(move || run().then(|value| value.into())).then_result(|result| result.and_then(T::try_from))
    }

    fn acquire_file_dialog_lock(&self, options: &Dynamic, window: Option<BrowserWindow>) -> Option<DisposableRef> {
        // If no window is provided, allow as many dialogs as
        // needed since we consider them not modal per window
        let Some(window) = window else {
            return Some(lifecycle::none());
        };

        // If a window is provided, only allow a single dialog
        // at the same time because dialogs are modal and we
        // do not want to open one dialog after the other
        // (https://github.com/microsoft/vscode/issues/114432)
        // we figure this out by `hashing` the configuration
        // options for the dialog to prevent duplicates

        self.log_service.trace_args("[DialogMainService]: request to acquire file dialog lock", &[LogArg::Value(options.clone())]);

        let options_hash = hash(options);
        {
            let mut window_file_dialog_locks = self.window_file_dialog_locks.borrow_mut();
            let window_locks = window_file_dialog_locks.entry(window.id).or_default();
            if window_locks.contains(&options_hash) {
                return None; // prevent duplicates, return
            }

            window_locks.insert(options_hash);
        }

        self.log_service.trace_args("[DialogMainService]: new file dialog lock created", &[LogArg::Value(options.clone())]);

        let (window_file_dialog_locks, log_service, options) = (self.window_file_dialog_locks.clone(), self.log_service.clone(), options.clone());
        Some(lifecycle::to_disposable(move || {
            log_service.trace_args("[DialogMainService]: file dialog lock disposed", &[LogArg::Value(options)]);

            let mut window_file_dialog_locks = window_file_dialog_locks.borrow_mut();
            if let Some(window_locks) = window_file_dialog_locks.get_mut(&window.id) {
                window_locks.remove(&options_hash);

                // If the window has no more dialog locks, delete it from the set of locks
                if window_locks.is_empty() {
                    window_file_dialog_locks.remove(&window.id);
                }
            }
        }))
    }
}

impl IDialogMainService for DialogMainService {
    fn pick_file_folder(&self, options: NativeOpenDialogOptions, window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>> {
        self.do_pick(
            InternalNativeOpenDialogOptions {
                base: options,
                pick_folders: Some(true),
                pick_files: Some(true),
                title: localize("open", "Open", &[]),
                ..InternalNativeOpenDialogOptions::default()
            },
            window,
        )
    }

    fn pick_folder(&self, options: NativeOpenDialogOptions, window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>> {
        let mut options_internal =
            InternalNativeOpenDialogOptions { base: options, pick_folders: Some(true), title: localize("openFolder", "Open Folder", &[]), ..InternalNativeOpenDialogOptions::default() };

        if self.host.is_windows {
            // Due to Windows/Electron issue the labels on Open Folder dialog have no hot keys.
            // We can fix this here for the button label, but some other labels remain inaccessible.
            // See https://github.com/electron/electron/issues/48631 for more info.
            options_internal.button_label = Some(mnemonic_button_label(&localize("selectFolder", "&&Select folder", &[]), &self.host).with_mnemonic);
        }

        self.do_pick(options_internal, window)
    }

    fn pick_file(&self, options: NativeOpenDialogOptions, window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>> {
        self.do_pick(InternalNativeOpenDialogOptions { base: options, pick_files: Some(true), title: localize("openFile", "Open File", &[]), ..InternalNativeOpenDialogOptions::default() }, window)
    }

    fn pick_workspace(&self, options: NativeOpenDialogOptions, window: Option<BrowserWindow>) -> Promise<Option<Vec<String>>> {
        let title = localize("openWorkspaceTitle", "Open Workspace from File", &[]);
        let button_label = mnemonic_button_label(&localize("openWorkspace", "&&Open", &[]), &self.host).with_mnemonic;
        let filters = workspace_filter().into_iter().map(|(name, extensions)| FileFilter { extensions, name }).collect();

        self.do_pick(
            InternalNativeOpenDialogOptions { base: options, pick_files: Some(true), title, filters: Some(filters), button_label: Some(button_label), ..InternalNativeOpenDialogOptions::default() },
            window,
        )
    }

    fn show_message_box(&self, raw_options: MessageBoxOptions, window: Option<BrowserWindow>) -> Promise<MessageBoxReturnValue> {
        let (product_service, native_dialogs, host) = (self.product_service.clone(), self.native_dialogs.clone(), self.host.clone());
        self.queue_dialog(window, move || {
            let massaged = massage_message_box_options(raw_options, product_service.as_ref(), &host);
            let button_indeces = massaged.button_indeces;

            native_dialogs.show_message_box(window, massaged.options).then_result(move |result| {
                let result = result?;
                let response = button_indeces.get(result.response).copied().ok_or_else(|| Error::new(format!("O botão {} não existe no diálogo", result.response)))?;

                Ok(MessageBoxReturnValue { response, checkbox_checked: result.checkbox_checked })
            })
        })
    }

    fn show_save_dialog(&self, options: SaveDialogOptions, window: Option<BrowserWindow>) -> Promise<SaveDialogReturnValue> {
        // Prevent duplicates of the same dialog queueing at the same time
        let Some(file_dialog_lock) = self.acquire_file_dialog_lock(&options.to_dynamic(), window) else {
            self.log_service.error("[DialogMainService]: file save dialog is already or will be showing for the window with the same configuration");

            return Promise::resolved(SaveDialogReturnValue { canceled: true, file_path: String::new() });
        };

        let (native_dialogs, host) = (self.native_dialogs.clone(), self.host.clone());
        self.queue_dialog(window, move || {
            native_dialogs.show_save_dialog(window, options).then(move |mut result| {
                result.file_path = normalize_path(&host, &result.file_path);

                result
            })
        })
        .finally(move || file_dialog_lock.dispose())
    }

    fn show_open_dialog(&self, mut options: OpenDialogOptions, window: Option<BrowserWindow>) -> Promise<OpenDialogReturnValue> {
        let this = self.clone();
        async_fn(async move {
            // Ensure the path exists (if provided)
            if let Some(default_path) = options.default_path.clone().filter(|default_path| !default_path.is_empty()) {
                let path_exists = pfs::exists(&default_path).await;
                if !path_exists {
                    options.default_path = None;
                }
            }

            // Prevent duplicates of the same dialog queueing at the same time
            let Some(file_dialog_lock) = this.acquire_file_dialog_lock(&options.to_dynamic(), window) else {
                this.log_service.error("[DialogMainService]: file open dialog is already or will be showing for the window with the same configuration");

                return Ok(OpenDialogReturnValue { canceled: true, file_paths: Vec::new() });
            };

            let (native_dialogs, host) = (this.native_dialogs.clone(), this.host.clone());
            this.queue_dialog(window, move || {
                native_dialogs.show_open_dialog(window, options).then(move |mut result| {
                    result.file_paths = result.file_paths.iter().map(|path| normalize_path(&host, path)).collect();

                    result
                })
            })
            .finally(move || file_dialog_lock.dispose())
            .await
        })
    }
}
