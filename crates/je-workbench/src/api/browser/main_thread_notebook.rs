//! Porte de `src/vs/workbench/api/browser/mainThreadNotebook.ts`: o `MainThreadNotebooks`, que registra no `INotebookService` os
//! serializadores e os tipos de notebook que as extensões contribuem, e os provedores de itens da barra de status das
//! células no `INotebookCellStatusBarService`, e os comandos internos `_executeDataToNotebook` e `_executeNotebookToData`.
//!
//! Divergências:
//! - **Registro.** O `@extHostNamedCustomer(MainContext.MainThreadNotebook)` não existe: o objeto é criado por
//!   [`MainThreadNotebooks::new`] com os quatro argumentos do original (o `IExtHostContext` é o [`IMainContext`], que dá o
//!   `getProxy`). Os `CommandsRegistry.registerCommand` do fim do arquivo rodam na carga do módulo no original; aqui o
//!   registro é a chamada explícita de [`register_notebook_execute_commands`].
//! - **`searchInNotebooks`.** O serializador do porte não busca em notebooks fechados: o `$searchInNotebooks` não está em
//!   `ExtHostNotebookShape` (depende do `ExtHostSearch`) e o `INotebookSerializer` do porte não declara o método. O
//!   `getContributedNotebookType`, `INotebookFileMatchNoModel` e `NotebookPriorityInfo` que o original lê ali ficam fora
//!   junto.
//! - **Logs.** O segundo argumento de `trace` (`{ viewType, extensionId }`) é o objeto [`Dynamic`] de mesmo formato.
//!   `sw.elapsed()` é o do [`StopWatch`].
//! - **`dataToNotebook`/`notebookToData`.** `SerializableObjectWithBuffers<NotebookDataDto>` é o do protocolo. O
//!   `notebookToData` registra o `DONE` logo após a chamada (sem aguardar), como o original.
//! - **`save`.** `isFileOperationError(stat)` é a variante [`NotebookSaveResult::Error`]; o `throw new FileOperationError(...)`
//!   é a promessa rejeitada com [`FileOperationError::into_error`]. O `{ ...stat, children: undefined, resource: uri }` é o
//!   [`FileStatWithMetadata`] com o `resource` da chamada.
//! - **Itens da barra de status.** O `Dto<INotebookCellStatusBarItem>` que o host devolve é o
//!   [`INotebookCellStatusBarEntryDto`] (ver o protocolo); a conversão para o [`INotebookCellStatusBarItem`] leva o `tooltip` de
//!   texto para [`StringOrMarkdown::String`] e o `ICommandDto` para o [`Command`] do editor (sem o `$ident`). O `dispose` da
//!   lista devolve o `cacheId` ao host.
//! - **`_notebookCellStatusBarRegistrations`.** Guarda o `Emitter` do evento (que o `instanceof Emitter` do original
//!   reconhece) e o descartável do provedor no mesmo mapa: é o `enum` [`StatusBarRegistration`].
//! - **Comandos `_execute*`.** Os argumentos e os resultados viajam como [`Dynamic`] com carga nativa (o `VSBuffer` e o
//!   `SerializableObjectWithBuffers<NotebookDataDto>`, ver `ext_host_notebook_api_commands`). O `info instanceof
//!   SimpleNotebookProviderInfo` é sempre verdadeiro (o `withNotebookDataProvider` devolve só esse tipo).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, combined_disposable};
use je_base::promise::Promise;
use je_base::stopwatch::StopWatch;
use je_base::types::assert_type;
use je_base::uri::URI;
use je_editor::code_action::languages::Command;
use je_platform::commands::commands::{CommandHandler, commands_registry};
use je_platform::files::files::{FileOperationError, FileStat, FileStatWithMetadata, WriteFileOptions};
use je_platform::log::{LogArg, LogService, Logger};

use crate::api::browser::main_thread_commands::promise_to_dynamic;
use crate::api::browser::main_thread_notebook_dto::{from_notebook_data_dto, to_notebook_data_dto};
use crate::api::common::ext_host_notebook_api_commands::{buffer_from_dynamic, buffer_to_dynamic, notebook_data_dto_from_dynamic, notebook_data_dto_to_dynamic};
use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostNotebookShape, IMainContext, INotebookCellStatusBarEntryDto, MainThreadNotebookShape, NotebookDataDto, NotebookSaveResult,
    SerializableObjectWithBuffers,
};
use crate::contrib::notebook::common::notebook_cell_status_bar_service::INotebookCellStatusBarService;
use crate::contrib::notebook::common::notebook_common::{
    INotebookCellStatusBarItem, INotebookCellStatusBarItemList, INotebookCellStatusBarItemProvider, INotebookContributionData, NotebookData,
    NotebookExtensionDescription, StringOrCommand, StringOrMarkdown, TransientOptions,
};
use crate::contrib::notebook::common::notebook_service::{I_NOTEBOOK_SERVICE, INotebookSerializer, INotebookService};

/// O objeto `{ viewType, extensionId }` do segundo argumento dos `trace` do serializador.
fn serializer_log_args(view_type: &str, extension_id: &str) -> [LogArg; 1] {
    [LogArg::Value(Dynamic::object([("viewType", Dynamic::string(view_type)), ("extensionId", Dynamic::string(extension_id))]))]
}

/// O objeto literal passado a `registerNotebookSerializer`: fala com o host pelo `handle`.
struct NotebookSerializerBridge {
    handle: u32,
    view_type: String,
    extension_id: String,
    options: TransientOptions,
    proxy: Rc<dyn ExtHostNotebookShape>,
    log_service: Rc<dyn LogService>,
}

impl INotebookSerializer for NotebookSerializerBridge {
    fn options(&self) -> TransientOptions {
        self.options.clone()
    }

    fn data_to_notebook(&self, data: VSBuffer) -> Promise<NotebookData> {
        let sw = StopWatch::new(None);
        let (handle, proxy, log_service) = (self.handle, self.proxy.clone(), self.log_service.clone());
        let (view_type, extension_id) = (self.view_type.clone(), self.extension_id.clone());
        async_fn(async move {
            let result = if data.byte_length() == 0 && view_type == "interactive" {
                // we don't want any starting cells for an empty interactive window.
                from_notebook_data_dto(&NotebookDataDto { cells: Vec::new(), metadata: Dynamic::object(Vec::<(&str, Dynamic)>::new()) })
            } else {
                let dto = proxy.rpc_data_to_notebook(handle, data, CancellationToken::None).await?;
                from_notebook_data_dto(&dto.value)
            };
            log_service.trace_args(&format!("[NotebookSerializer] dataToNotebook DONE after {}ms", sw.elapsed()), &serializer_log_args(&view_type, &extension_id));
            Ok(result)
        })
    }

    fn notebook_to_data(&self, data: NotebookData) -> Promise<VSBuffer> {
        let sw = StopWatch::new(None);
        let result = self.proxy.rpc_notebook_to_data(self.handle, SerializableObjectWithBuffers::new(to_notebook_data_dto(&data)), CancellationToken::None);
        self.log_service.trace_args(&format!("[NotebookSerializer] notebookToData DONE after {}", sw.elapsed()), &serializer_log_args(&self.view_type, &self.extension_id));
        result
    }

    fn save(&self, uri: &URI, version_id: i64, options: WriteFileOptions, token: CancellationToken) -> Promise<FileStatWithMetadata> {
        let uri = uri.clone();
        let saved_uri = uri.clone();
        self.proxy.rpc_save_notebook(self.handle, &uri, version_id, options, token).then_result(move |result| match result? {
            NotebookSaveResult::Error(error) => Err(FileOperationError::into_error(error)),
            NotebookSaveResult::Stats(stat) => Ok(FileStatWithMetadata::new(FileStat {
                resource: saved_uri,
                name: stat.name,
                size: Some(stat.size),
                mtime: Some(stat.mtime),
                ctime: Some(stat.ctime),
                etag: Some(stat.etag),
                readonly: Some(stat.readonly),
                locked: Some(stat.locked),
                executable: Some(stat.executable),
                is_file: stat.is_file,
                is_directory: stat.is_directory,
                is_symbolic_link: stat.is_symbolic_link,
                children: None,
            })),
        })
    }
}

/// O `Dto<INotebookCellStatusBarItem>` que o host devolve, como o item do modelo.
fn status_bar_item_from_dto(dto: INotebookCellStatusBarEntryDto) -> INotebookCellStatusBarItem {
    INotebookCellStatusBarItem {
        alignment: dto.alignment,
        priority: dto.priority,
        text: dto.text,
        color: None,
        background_color: None,
        tooltip: dto.tooltip.map(StringOrMarkdown::String),
        command: dto.command.map(|command| StringOrCommand::Command(Command { id: command.id, title: command.title, tooltip: command.tooltip, arguments: command.arguments })),
        accessibility_information: dto.accessibility_information,
        opacity: None,
        only_show_when_active: None,
    }
}

/// O `INotebookCellStatusBarItemProvider` literal de `$registerNotebookCellStatusBarItemProvider`.
struct CellStatusBarItemProvider {
    handle: u32,
    view_type: String,
    proxy: Rc<dyn ExtHostNotebookShape>,
    on_did_change_status_bar_items: Option<Event<()>>,
}

impl INotebookCellStatusBarItemProvider for CellStatusBarItemProvider {
    fn view_type(&self) -> String {
        self.view_type.clone()
    }

    fn on_did_change_status_bar_items(&self) -> Option<Event<()>> {
        self.on_did_change_status_bar_items.clone()
    }

    fn provide_cell_status_bar_items(&self, uri: &URI, index: usize, token: &CancellationToken) -> Promise<Option<INotebookCellStatusBarItemList>> {
        let proxy = self.proxy.clone();
        self.proxy.rpc_provide_notebook_cell_status_bar_items(self.handle, uri, index, token.clone()).then(move |result| {
            let cache_id = result.as_ref().map(|result| result.cache_id);
            let items = result.map(|result| result.items).unwrap_or_default().into_iter().map(status_bar_item_from_dto).collect();
            Some(INotebookCellStatusBarItemList {
                items,
                dispose: Some(Rc::new(move || {
                    if let Some(cache_id) = cache_id {
                        proxy.rpc_release_notebook_cell_status_bar_items(cache_id);
                    }
                })),
            })
        })
    }
}

/// Um valor de `_notebookCellStatusBarRegistrations`: o `Emitter` do evento de itens ou o registro do provedor.
enum StatusBarRegistration {
    Emitter(Rc<Emitter<()>>),
    Provider(DisposableRef),
}

impl StatusBarRegistration {
    fn dispose(&self) {
        match self {
            StatusBarRegistration::Emitter(emitter) => emitter.dispose(),
            StatusBarRegistration::Provider(provider) => provider.dispose(),
        }
    }
}

/// `MainThreadNotebooks`.
pub struct MainThreadNotebooks {
    disposables: Rc<DisposableStore>,
    proxy: Rc<dyn ExtHostNotebookShape>,
    notebook_serializer: RefCell<HashMap<u32, Rc<DisposableStore>>>,
    notebook_cell_status_bar_registrations: RefCell<HashMap<u32, StatusBarRegistration>>,
    notebook_service: Rc<dyn INotebookService>,
    cell_status_bar_service: Rc<dyn INotebookCellStatusBarService>,
    log_service: Rc<dyn LogService>,
}

impl MainThreadNotebooks {
    pub fn new(
        ext_host_context: &Rc<dyn IMainContext>,
        notebook_service: Rc<dyn INotebookService>,
        cell_status_bar_service: Rc<dyn INotebookCellStatusBarService>,
        log_service: Rc<dyn LogService>,
    ) -> Rc<Self> {
        Rc::new(Self {
            disposables: DisposableStore::new(),
            proxy: ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_NOTEBOOK),
            notebook_serializer: RefCell::new(HashMap::new()),
            notebook_cell_status_bar_registrations: RefCell::new(HashMap::new()),
            notebook_service,
            cell_status_bar_service,
            log_service,
        })
    }

    /// `unregisterThing(handle)` de `$unregisterNotebookCellStatusBarItemProvider`.
    fn unregister_status_bar_registration(&self, handle: u32) {
        let entry = self.notebook_cell_status_bar_registrations.borrow_mut().remove(&handle);
        if let Some(entry) = entry {
            entry.dispose();
        }
    }
}

impl IDisposable for MainThreadNotebooks {
    fn dispose(&self) {
        self.disposables.dispose();
        let serializers: Vec<Rc<DisposableStore>> = self.notebook_serializer.borrow().values().cloned().collect();
        for serializer in serializers {
            serializer.dispose();
        }
        let registrations: Vec<StatusBarRegistration> = self.notebook_cell_status_bar_registrations.borrow_mut().drain().map(|(_, registration)| registration).collect();
        for registration in registrations {
            registration.dispose();
        }
    }
}

impl MainThreadNotebookShape for MainThreadNotebooks {
    fn rpc_register_notebook_serializer(&self, handle: u32, extension: NotebookExtensionDescription, view_type: &str, options: TransientOptions, data: Option<INotebookContributionData>) {
        let disposables = DisposableStore::new();
        let extension_id = extension.id.value.clone();

        let serializer = Rc::new(NotebookSerializerBridge {
            handle,
            view_type: view_type.to_string(),
            extension_id: extension_id.clone(),
            options,
            proxy: self.proxy.clone(),
            log_service: self.log_service.clone(),
        });
        disposables.add(self.notebook_service.register_notebook_serializer(view_type, extension, serializer));

        if let Some(data) = data {
            disposables.add(self.notebook_service.register_contributed_notebook_type(view_type, data));
        }
        self.notebook_serializer.borrow_mut().insert(handle, disposables);

        self.log_service.trace_args("[NotebookSerializer] registered notebook serializer", &serializer_log_args(view_type, &extension_id));
    }

    fn rpc_unregister_notebook_serializer(&self, handle: u32) {
        let serializer = self.notebook_serializer.borrow_mut().remove(&handle);
        if let Some(serializer) = serializer {
            serializer.dispose();
        }
    }

    fn rpc_emit_cell_status_bar_event(&self, event_handle: u32) {
        let emitter = match self.notebook_cell_status_bar_registrations.borrow().get(&event_handle) {
            Some(StatusBarRegistration::Emitter(emitter)) => Some(emitter.clone()),
            _ => None,
        };
        if let Some(emitter) = emitter {
            emitter.fire(());
        }
    }

    fn rpc_register_notebook_cell_status_bar_item_provider(&self, handle: u32, event_handle: Option<u32>, view_type: &str) -> Promise<()> {
        let mut on_did_change_status_bar_items = None;
        if let Some(event_handle) = event_handle {
            let emitter = Emitter::<()>::new();
            on_did_change_status_bar_items = Some(emitter.event());
            self.notebook_cell_status_bar_registrations.borrow_mut().insert(event_handle, StatusBarRegistration::Emitter(emitter));
        }

        let provider = Rc::new(CellStatusBarItemProvider { handle, view_type: view_type.to_string(), proxy: self.proxy.clone(), on_did_change_status_bar_items });

        let disposable = self.cell_status_bar_service.register_cell_status_bar_item_provider(provider);
        self.notebook_cell_status_bar_registrations.borrow_mut().insert(handle, StatusBarRegistration::Provider(disposable));
        Promise::resolved(())
    }

    fn rpc_unregister_notebook_cell_status_bar_item_provider(&self, handle: u32, event_handle: Option<u32>) -> Promise<()> {
        self.unregister_status_bar_registration(handle);
        if let Some(event_handle) = event_handle {
            self.unregister_status_bar_registration(event_handle);
        }
        Promise::resolved(())
    }
}

/// `CommandsRegistry.registerCommand('_executeDataToNotebook', ...)` e `'_executeNotebookToData'`.
pub fn register_notebook_execute_commands() -> DisposableRef {
    let data_to_notebook: CommandHandler = Rc::new(|accessor, args| {
        let notebook_type = args.first().cloned().unwrap_or(Dynamic::Undefined);
        let Dynamic::String(notebook_type) = notebook_type else {
            assert_type(&false, Some("string"));
            return Dynamic::Undefined;
        };
        let bytes = args.get(1).and_then(buffer_from_dynamic);
        assert_type(&bytes.is_some(), Some("VSBuffer"));
        let bytes = bytes.expect("o assert_type acima garante o VSBuffer");

        let notebook_service = accessor.get(I_NOTEBOOK_SERVICE);
        let notebook_type = notebook_type.to_string_lossy();
        promise_to_dynamic(async_fn(async move {
            let info = notebook_service.with_notebook_data_provider(&notebook_type).await?;
            let dto = info.serializer.data_to_notebook((*bytes).clone()).await?;
            Ok(notebook_data_dto_to_dynamic(SerializableObjectWithBuffers::new(to_notebook_data_dto(&dto))))
        }))
    });

    let notebook_to_data: CommandHandler = Rc::new(|accessor, args| {
        let notebook_type = args.first().cloned().unwrap_or(Dynamic::Undefined);
        let Dynamic::String(notebook_type) = notebook_type else {
            assert_type(&false, Some("string"));
            return Dynamic::Undefined;
        };
        let dto = args.get(1).and_then(notebook_data_dto_from_dynamic);

        let notebook_service = accessor.get(I_NOTEBOOK_SERVICE);
        let notebook_type = notebook_type.to_string_lossy();
        promise_to_dynamic(async_fn(async move {
            let info = notebook_service.with_notebook_data_provider(&notebook_type).await?;
            let dto = dto.ok_or_else(|| je_base::errors::Error::new("Cannot read properties of undefined (reading 'value')"))?;
            let data = from_notebook_data_dto(&dto.value);
            let bytes = info.serializer.notebook_to_data(data).await?;
            Ok(buffer_to_dynamic(bytes))
        }))
    });

    combined_disposable(vec![
        commands_registry().register_command_handler("_executeDataToNotebook", Some(data_to_notebook)),
        commands_registry().register_command_handler("_executeNotebookToData", Some(notebook_to_data)),
    ])
}
