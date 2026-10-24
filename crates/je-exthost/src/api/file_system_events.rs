//! `vscode.workspace.createFileSystemWatcher` e os eventos de operação de arquivo `onDidCreateFiles`, `onDidDeleteFiles`,
//! `onDidRenameFiles`, `onWillCreateFiles`, `onWillDeleteFiles` e `onWillRenameFiles`, ligados ao `ExtHostFileSystemEventService`
//! portado de `extHostFileSystemEventService.ts` (`ext_host_file_system_event_service`).
//!
//! O `FileSystemWatcher` que a extensão segura é um objeto com `ignoreCreateEvents`, `ignoreChangeEvents`, `ignoreDeleteEvents`,
//! os eventos `onDidCreate`, `onDidChange` e `onDidDelete` (funções `(listener, thisArgs?, disposables?) => Disposable`) e
//! `dispose()`. Os eventos de operação entregam `{ files }` (`{ oldUri, newUri }` em `onDidRenameFiles`); os `onWill*Files`
//! entregam também `token` e `waitUntil(thenable)`, e a `WorkspaceEdit` que a promessa devolve entra na operação (qualquer outro
//! valor é ignorado, como no original).
//!
//! Falta: nada do que o `extHost.api.impl.ts` entrega para esses membros. O `createFileSystemWatcher` só existe quando o host tem os
//! clientes de workspace e configuração (ele precisa das pastas do workspace e de `files.watcherExclude`); os eventos existem
//! sempre que o serviço está ligado.
//!
//! Divergências:
//! - O `extension` do `createFileSystemWatcher` e dos `onWill*Files` é o `je.exthost` (o `vscode` é um só para todas as extensões).
//!   Ele só aparece no pedido de observação enviado à thread principal e no aviso de participante lento.
//! - O objeto do `FileSystemWatcher` é copiado para o motor: as flags `ignore*Events` são os valores do instante da criação (elas
//!   não mudam depois, como no original) e `dispose` é uma função própria do objeto, não do protótipo.
//! - `waitUntil` só vale durante o despacho do listener: as promessas que ele recebe são entregues ao evento no epílogo, logo
//!   depois do retorno do listener (o `WaitUntilEvent` do Rust não sobrevive dentro do objeto copiado). Chamar `waitUntil` depois
//!   disso lança "waitUntil can NOT be called asynchronous", como o original.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Event, WaitUntilEvent};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_workbench::api::common::ext_host_configuration::ExtHostConfiguration;
use je_workbench::api::common::ext_host_file_system_event_service::{
    wait_until_edit, ExtHostFileSystemEventService, FileCreateEvent, FileDeleteEvent, FileRename, FileRenameEvent, FileSystemWatcher, FileSystemWatcherCreateOptions,
    FileWatcherWorkspace, FileWillEvent,
};
use je_workbench::api::common::ext_host_file_system_info::ExtHostFileSystemInfo;
use je_workbench::api::common::ext_host_types::WorkspaceEdit;
use je_workbench::api::common::ext_host_workspace::ExtHostWorkspace;
use zjsc::api::embed_node::{JsData, MethodCallback, NodeRunner};

use crate::api::disposable::DisposableTable;
use crate::api::events::{event_function, event_function_with_epilogue, EventTables};
use crate::api::languages::host_extension;
use crate::api::workspace::glob_of;
use crate::api::workspace_edit::WorkspaceEditObjects;
use crate::convert::{js_to_error, uri_to_js};

/// O que o `createFileSystemWatcher` lê do workspace: as pastas, `files.watcherExclude` e as capacidades dos provedores.
pub struct WatcherServices {
    pub workspace: Rc<ExtHostWorkspace>,
    pub configuration: Rc<ExtHostConfiguration>,
    pub file_system_info: Rc<ExtHostFileSystemInfo>,
}

/// Os serviços do host que os membros de arquivo de `vscode.workspace` usam.
#[derive(Clone)]
pub struct FileSystemEventsApi {
    pub service: Rc<ExtHostFileSystemEventService>,
    /// Sem os clientes de workspace e configuração não há `createFileSystemWatcher`.
    pub watcher: Option<Rc<WatcherServices>>,
    /// Onde moram as `WorkspaceEdit` que `waitUntil` recebe.
    pub edits: Rc<WorkspaceEditObjects>,
    pub events: Rc<EventTables>,
}

// ---- a leitura dos argumentos

/// `Boolean(value)` do JavaScript sobre um argumento da API.
fn truthy(value: Option<&JsData>) -> bool {
    match value {
        None | Some(JsData::Undefined) | Some(JsData::Null) => false,
        Some(JsData::Bool(flag)) => *flag,
        Some(JsData::Num(number)) => *number != 0.0 && !number.is_nan(),
        Some(JsData::Str(text)) => !text.is_empty(),
        Some(_) => true,
    }
}

// ---- o FileSystemWatcher

fn watcher_to_js(runner: &NodeRunner, table: &Rc<DisposableTable>, watcher: Rc<FileSystemWatcher>) -> JsData {
    let uri_event = |name: &str, event: Event<URI>| (name.to_owned(), event_function(runner, table, name, event, uri_to_js));
    let disposed = watcher.clone();
    let dispose = runner.make_function(
        "dispose",
        0,
        Rc::new(move |_| {
            disposed.dispose();
            Ok(JsData::Undefined)
        }),
    );
    JsData::Object(vec![
        ("ignoreCreateEvents".to_owned(), JsData::Bool(watcher.ignore_create_events())),
        ("ignoreChangeEvents".to_owned(), JsData::Bool(watcher.ignore_change_events())),
        ("ignoreDeleteEvents".to_owned(), JsData::Bool(watcher.ignore_delete_events())),
        uri_event("onDidCreate", watcher.on_did_create()),
        uri_event("onDidChange", watcher.on_did_change()),
        uri_event("onDidDelete", watcher.on_did_delete()),
        ("dispose".to_owned(), JsData::Handle(Rc::new(dispose))),
    ])
}

fn create_watcher(runner: &NodeRunner, table: &Rc<DisposableTable>, api: &FileSystemEventsApi, args: &[JsData]) -> Result<JsData, String> {
    let services = api.watcher.as_ref().ok_or("createFileSystemWatcher: o host não tem os clientes de workspace e configuração")?;
    let pattern = glob_of(args.first().ok_or("createFileSystemWatcher: falta o padrão")?)?;
    let provider = services.configuration.initialized_provider().ok_or("createFileSystemWatcher: a configuração ainda não foi inicializada")?;
    let workspace: Rc<dyn FileWatcherWorkspace> = services.workspace.clone();
    let options = FileSystemWatcherCreateOptions {
        ignore_create_events: Some(truthy(args.get(1))),
        ignore_change_events: Some(truthy(args.get(2))),
        ignore_delete_events: Some(truthy(args.get(3))),
    };
    let watcher = api.service.create_file_system_watcher(workspace, &*provider, &services.file_system_info, &host_extension(), &pattern, options);
    Ok(watcher_to_js(runner, table, watcher))
}

// ---- os eventos de operação de arquivo

// `&Vec` e não `&[_]`: a função é passada como `fn(&F)` de `will_event`, em que `F` é o `Vec` do evento.
#[allow(clippy::ptr_arg)]
fn uris_to_js(files: &Vec<URI>) -> JsData {
    JsData::Array(files.iter().map(uri_to_js).collect())
}

#[allow(clippy::ptr_arg)]
fn renames_to_js(files: &Vec<FileRename>) -> JsData {
    JsData::Array(
        files.iter().map(|file| JsData::Object(vec![("oldUri".to_owned(), uri_to_js(&file.old_uri)), ("newUri".to_owned(), uri_to_js(&file.new_uri))])).collect(),
    )
}

fn files_event(files: JsData) -> JsData {
    JsData::Object(vec![("files".to_owned(), files)])
}

/// O valor de uma promessa de `waitUntil` como a `WorkspaceEdit` que ela devolve (`None` para qualquer outro valor, que o original
/// ignora; uma promessa rejeitada rejeita o participante).
fn thenable_edit(runner: &NodeRunner, edits: &Rc<WorkspaceEditObjects>, value: JsData) -> Promise<Option<WorkspaceEdit>> {
    let edits = edits.clone();
    let JsData::Handle(thenable) = &value else { return Promise::resolved(edits.resolved_edit(&value)) };
    let (promise, resolver) = Promise::<JsData>::new();
    let on_fulfilled = {
        let resolver = resolver.clone();
        runner.make_function(
            "fulfilled",
            1,
            Rc::new(move |args| {
                resolver.resolve(args.first().cloned().unwrap_or(JsData::Undefined));
                Ok(JsData::Undefined)
            }),
        )
    };
    let on_rejected = runner.make_function(
        "rejected",
        1,
        Rc::new(move |args| {
            resolver.reject(js_to_error(args.first().unwrap_or(&JsData::Undefined)));
            Ok(JsData::Undefined)
        }),
    );
    match runner.call_method(thenable, "then", &[JsData::Handle(Rc::new(on_fulfilled)), JsData::Handle(Rc::new(on_rejected))]) {
        Ok(_) => promise.then(move |resolved| edits.resolved_edit(&resolved)),
        // Sem `then` chamável: é um valor comum, que não é uma `WorkspaceEdit`.
        Err(_) => Promise::resolved(None),
    }
}

/// A função de evento de um `onWill*Files`: o listener recebe `{ token, files, waitUntil }`.
fn will_event<F: Clone + 'static>(
    runner: &NodeRunner,
    table: &Rc<DisposableTable>,
    api: &FileSystemEventsApi,
    name: &str,
    event: Event<WaitUntilEvent<FileWillEvent<F>>>,
    files_to_js: fn(&F) -> JsData,
) -> JsData {
    let (runner_for_event, api) = (runner.clone(), api.clone());
    event_function_with_epilogue(runner, table, name, event, move |event: &WaitUntilEvent<FileWillEvent<F>>| {
        // `None` marca o fim do despacho: depois dele `waitUntil` não vale mais.
        let thenables: Rc<RefCell<Option<Vec<JsData>>>> = Rc::new(RefCell::new(Some(Vec::new())));
        let collected = thenables.clone();
        let wait_until = runner_for_event.make_function(
            "waitUntil",
            1,
            Rc::new(move |args| {
                let mut slot = collected.borrow_mut();
                let list = slot.as_mut().ok_or("waitUntil can NOT be called asynchronous")?;
                list.push(args.first().cloned().unwrap_or(JsData::Undefined));
                Ok(JsData::Undefined)
            }),
        );
        let payload = JsData::Object(vec![
            ("token".to_owned(), api.events.token_to_js(event.token.clone())),
            ("files".to_owned(), files_to_js(&event.data.files)),
            ("waitUntil".to_owned(), JsData::Handle(Rc::new(wait_until))),
        ]);
        let (runner, edits) = (runner_for_event.clone(), api.edits.clone());
        let epilogue = move |event: &WaitUntilEvent<FileWillEvent<F>>| {
            for thenable in thenables.borrow_mut().take().unwrap_or_default() {
                wait_until_edit(event, thenable_edit(&runner, &edits, thenable));
            }
        };
        (payload, epilogue)
    })
}

// ---- os membros de workspace

/// Os membros de `vscode.workspace` que o `ExtHostFileSystemEventService` entrega: `createFileSystemWatcher` (se houver os clientes
/// de workspace e configuração) e os seis eventos de operação de arquivo.
pub fn workspace_members(runner: &NodeRunner, api: &FileSystemEventsApi, table: &Rc<DisposableTable>) -> (Vec<(String, u32, MethodCallback)>, Vec<(String, MethodCallback)>) {
    let mut methods: Vec<(String, u32, MethodCallback)> = Vec::new();
    if api.watcher.is_some() {
        let (runner, table, api) = (runner.clone(), table.clone(), api.clone());
        methods.push(("createFileSystemWatcher".to_owned(), 4, Rc::new(move |_, args| create_watcher(&runner, &table, &api, args))));
    }

    let mut getters: Vec<(String, MethodCallback)> = Vec::new();
    let mut getter = |name: &str, make: Box<dyn Fn() -> JsData>| getters.push((name.to_owned(), Rc::new(move |_, _| Ok(make()))));
    let service = api.service.clone();
    {
        let (runner, table, service) = (runner.clone(), table.clone(), service.clone());
        getter(
            "onDidCreateFiles",
            Box::new(move || event_function(&runner, &table, "onDidCreateFiles", service.on_did_create_file(), |event: &FileCreateEvent| files_event(uris_to_js(&event.files)))),
        );
    }
    {
        let (runner, table, service) = (runner.clone(), table.clone(), service.clone());
        getter(
            "onDidDeleteFiles",
            Box::new(move || event_function(&runner, &table, "onDidDeleteFiles", service.on_did_delete_file(), |event: &FileDeleteEvent| files_event(uris_to_js(&event.files)))),
        );
    }
    {
        let (runner, table, service) = (runner.clone(), table.clone(), service.clone());
        getter(
            "onDidRenameFiles",
            Box::new(move || event_function(&runner, &table, "onDidRenameFiles", service.on_did_rename_file(), |event: &FileRenameEvent| files_event(renames_to_js(&event.files)))),
        );
    }
    {
        let (runner, table, api) = (runner.clone(), table.clone(), api.clone());
        getter(
            "onWillCreateFiles",
            Box::new(move || {
                will_event(&runner, &table, &api, "onWillCreateFiles", api.service.get_on_will_create_file_event(&host_extension()), uris_to_js)
            }),
        );
    }
    {
        let (runner, table, api) = (runner.clone(), table.clone(), api.clone());
        getter(
            "onWillDeleteFiles",
            Box::new(move || {
                will_event(&runner, &table, &api, "onWillDeleteFiles", api.service.get_on_will_delete_file_event(&host_extension()), uris_to_js)
            }),
        );
    }
    {
        let (runner, table, api) = (runner.clone(), table.clone(), api.clone());
        getter(
            "onWillRenameFiles",
            Box::new(move || {
                will_event(&runner, &table, &api, "onWillRenameFiles", api.service.get_on_will_rename_file_event(&host_extension()), renames_to_js)
            }),
        );
    }
    (methods, getters)
}
