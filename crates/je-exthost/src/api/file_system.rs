//! `vscode.workspace.fs`, `vscode.workspace.registerFileSystemProvider`, `vscode.FileSystemError` e os enums `FileType`,
//! `FileChangeType` e `FilePermission`: a ponte para o `ExtHostConsumerFileSystem` (`extHostFileSystemConsumer.ts`) e o
//! `ExtHostFileSystem` (`extHostFileSystem.ts`) portados.
//!
//! O `fs` é um objeto com `stat`, `readDirectory`, `createDirectory`, `readFile`, `writeFile`, `delete`, `rename`, `copy` e
//! `isWritableFileSystem`, todos sobre o [`ApiFileSystem`]; cada operação devolve a promessa do consumidor, que vai direto ao
//! provedor registrado pela própria extensão host (quando há um para o esquema) ou à thread principal. O
//! `registerFileSystemProvider(scheme, provider, options?)` recebe o provedor POR REFERÊNCIA (com o `this` e os métodos de classe) e o
//! registra no `ExtHostFileSystem`, que avisa a thread principal e responde aos `$stat`, `$readdir`, `$readFile`... dela. O provedor
//! implementa `stat`, `readDirectory`, `createDirectory`, `readFile`, `writeFile`, `delete`, `rename`, `watch`, `onDidChangeFile` e,
//! opcional, `copy`; cada método pode devolver o valor ou uma promessa.
//!
//! Falta: o quarteto `open`/`close`/`read`/`write` do provedor (a proposta `fsChunks`, que o host nunca habilita: um provedor que o
//! declara é rejeitado no registro, como o original rejeitaria para uma extensão sem a proposta) e o `Uint8Array` (ver abaixo).
//!
//! Divergências:
//! - O `Uint8Array` não atravessa o motor (o mesmo recorte de `NotebookCellOutputItem#data` em `notebooks`): `readFile` e o `readFile`
//!   do provedor entregam e recebem a lista de bytes (números de 0 a 255), e `writeFile` aceita a lista de bytes, um texto (os bytes
//!   UTF-8) ou um `Uint8Array` copiado como objeto de índices. O que o provedor devolve em `readFile` passa pela mesma conversão.
//! - O `vscode` é um só para todas as extensões: o provedor é registrado em nome da extensão `je.exthost`.
//! - `registerFileSystemProvider` pede o `ExtHostFileSystem`, que por sua vez pede o `ExtHostLanguageFeatures` (o provedor de links de
//!   documento do esquema); sem os clientes de linguagem do host a função lança. O `fs` só pede o cliente `MainThreadFileSystem`.
//! - `FileSystemError` não estende `Error` (o motor não deixa uma classe nativa herdar do `Error` embutido): a instância tem `name`
//!   (`FileNotFound (FileSystemError)`), `message` e `code`, e `instanceof Error` é falso. Uma promessa de `fs` que rejeita entrega um
//!   objeto de erro com `name` e `message` (o `name` tem o código do erro), e um provedor que lança um `FileSystemError` mantém o
//!   `name`, que é por onde o `ExtHostConsumerFileSystem` reconhece `FileNotFound` e `FileExists` no `mkdirp`.
//! - O `fs` é recriado a cada leitura de `workspace.fs` (um objeto de funções, não o objeto congelado único): a identidade não é
//!   observável. `isWritableFileSystem` devolve `undefined` enquanto a thread principal não informou as capacidades do esquema.
//! - `FileStat#permissions` e `FileChangeEvent#type` chegam do provedor como números (`FilePermission.Readonly`, `FileChangeType`).
//! - `watch(uri, options)` recebe `{ recursive, excludes }`; o `Disposable` que o provedor devolve é descartado pelo `$unwatch`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{on_unexpected_error, throw, Error};
use je_base::event::{Event, Listener};
use je_base::lifecycle::{combined_disposable, plain_disposable, to_disposable, DisposableRef};
use je_base::promise::MaybePromise;
use je_base::uri::URI;
use je_platform::files::files::{FileDeleteOptions, FileOverwriteOptions, FileSystemProviderErrorCode, FileType, FileWriteOptions, WatchOptions};
use je_workbench::api::common::ext_host_file_system::ExtHostFileSystem;
use je_workbench::api::common::ext_host_file_system_consumer::{
    ApiDeleteOptions, ApiFileChangeEvent, ApiFilePermission, ApiFileStat, ApiFileSystem, ApiFileSystemProvider, ExtHostConsumerFileSystem, FileSystemProviderOptions,
    FileSystemReadonly,
};
use je_workbench::api::common::ext_host_types::{FileChangeType, FileSystemError, MessageOrUri};
use zjsc::api::embed_node::{ClassSpec, JsData, JsHandle, MethodCallback, NativeCallback, NodeRunner, CLASS_KEY};

use crate::api::commands::{convert_maybe, promise_to_js};
use crate::api::disposable::DisposableTable;
use crate::api::js_object::{JsObjects, JsResult};
use crate::api::language_types::markdown_from_js;
use crate::api::languages::host_extension;
use crate::convert::{bytes_of, bytes_to_js, exported, field, is_nullish, opt_bool, opt_number, string_arg, uri_from_js, uri_to_js};

/// Onde o host guarda o `ExtHostFileSystem`, que só nasce depois do `ExtHostLanguageFeatures` (o namespace `workspace` é montado antes).
pub type ProviderSlot = Rc<RefCell<Option<Rc<ExtHostFileSystem>>>>;

/// Os serviços do host que os membros de arquivo de `vscode.workspace` usam.
#[derive(Clone)]
pub struct FileSystemApi {
    pub consumer: Rc<ExtHostConsumerFileSystem>,
    /// Vazio enquanto o host não tem os clientes de linguagem: sem ele não há `registerFileSystemProvider`.
    pub provider: ProviderSlot,
    pub js: JsObjects,
    pub table: Rc<DisposableTable>,
}

/// O que o namespace `workspace` junta: os acessores (`fs`) e as funções por referência (`registerFileSystemProvider`).
pub struct FileSystemMembers {
    pub getters: Vec<(String, MethodCallback)>,
    pub references: Vec<(String, JsData)>,
}

// ---- os valores de arquivo

fn file_type_of(value: Option<&JsData>) -> Result<FileType, Error> {
    match opt_number(value) {
        Some(number) if number.fract() == 0.0 && number >= 0.0 => Ok(FileType(number as u32)),
        _ => Err(Error::new("FileType inválido")),
    }
}

/// `vscode.FileStat` que o provedor devolveu.
fn stat_of(value: &JsData) -> Result<ApiFileStat, Error> {
    let number = |name: &str| opt_number(field(value, name)).ok_or_else(|| Error::new(format!("FileStat.{name} precisa ser um número")));
    Ok(ApiFileStat {
        file_type: file_type_of(field(value, "type"))?,
        ctime: number("ctime")? as i64,
        mtime: number("mtime")? as i64,
        size: number("size")? as u64,
        permissions: (opt_number(field(value, "permissions")) == Some(f64::from(ApiFilePermission::Readonly as i32))).then_some(ApiFilePermission::Readonly),
    })
}

fn stat_to_js(stat: &ApiFileStat) -> JsData {
    let mut fields = vec![
        ("type".to_owned(), JsData::Num(f64::from(stat.file_type.bits()))),
        ("ctime".to_owned(), JsData::Num(stat.ctime as f64)),
        ("mtime".to_owned(), JsData::Num(stat.mtime as f64)),
        ("size".to_owned(), JsData::Num(stat.size as f64)),
    ];
    if let Some(permissions) = stat.permissions {
        fields.push(("permissions".to_owned(), JsData::Num(f64::from(permissions as i32))));
    }
    JsData::Object(fields)
}

/// `[string, FileType][]` que o provedor devolveu.
fn directory_of(value: &JsData) -> Result<Vec<(String, FileType)>, Error> {
    let JsData::Array(entries) = value else { return Err(Error::new("readDirectory precisa devolver [string, FileType][]")) };
    entries
        .iter()
        .map(|entry| match entry {
            JsData::Array(pair) => match pair.as_slice() {
                [JsData::Str(name), kind] => Ok((name.clone(), file_type_of(Some(kind))?)),
                _ => Err(Error::new("readDirectory precisa devolver [string, FileType][]")),
            },
            _ => Err(Error::new("readDirectory precisa devolver [string, FileType][]")),
        })
        .collect()
}

#[allow(clippy::ptr_arg)]
fn directory_to_js(entries: &Vec<(String, FileType)>) -> JsData {
    JsData::Array(entries.iter().map(|(name, kind)| JsData::Array(vec![JsData::Str(name.clone()), JsData::Num(f64::from(kind.bits()))])).collect())
}

fn uri_arg(args: &[JsData], index: usize, what: &str) -> Result<URI, String> {
    uri_from_js(args.get(index).filter(|uri| !is_nullish(Some(*uri))).ok_or_else(|| format!("{what} precisa ser uma Uri"))?)
}

/// O argumento opcional `options`: `None` se falta, é `undefined` ou `null`.
fn options_arg(args: &[JsData], index: usize) -> Option<&JsData> {
    args.get(index).filter(|options| !is_nullish(Some(*options)))
}

fn overwrite_options(args: &[JsData], index: usize) -> FileOverwriteOptions {
    FileOverwriteOptions { overwrite: options_arg(args, index).and_then(|options| opt_bool(field(options, "overwrite"))).unwrap_or(false) }
}

fn flag_object(flags: &[(&str, bool)]) -> JsData {
    JsData::Object(flags.iter().map(|(name, flag)| ((*name).to_owned(), JsData::Bool(*flag))).collect())
}

// ---- vscode.FileSystemError

fn message_or_uri_of(value: Option<&JsData>) -> Result<Option<MessageOrUri>, String> {
    match value {
        None | Some(JsData::Undefined) | Some(JsData::Null) => Ok(None),
        Some(JsData::Str(message)) => Ok(Some(MessageOrUri::Message(message.clone()))),
        Some(uri @ JsData::Object(_)) => Ok(Some(MessageOrUri::Uri(uri_from_js(uri)?))),
        Some(_) => Err("FileSystemError: o argumento precisa ser uma mensagem ou uma Uri".to_owned()),
    }
}

/// Os campos próprios de uma instância de `FileSystemError`.
fn error_fields(error: &FileSystemError) -> Vec<(String, JsData)> {
    vec![
        ("name".to_owned(), JsData::Str(error.error().name.clone())),
        ("message".to_owned(), JsData::Str(error.error().message.clone())),
        ("code".to_owned(), JsData::Str(error.code.clone())),
    ]
}

/// A classe `FileSystemError` e as fábricas estáticas (`FileNotFound(uriOrMessage?)`...).
pub fn classes(runner: &NodeRunner) -> Vec<(String, JsData)> {
    let factories: [(&str, fn(Option<MessageOrUri>) -> FileSystemError); 6] = [
        ("FileExists", FileSystemError::file_exists),
        ("FileNotFound", FileSystemError::file_not_found),
        ("FileNotADirectory", FileSystemError::file_not_a_directory),
        ("FileIsADirectory", FileSystemError::file_is_a_directory),
        ("NoPermissions", FileSystemError::no_permissions),
        ("Unavailable", FileSystemError::unavailable),
    ];
    let statics: Vec<(String, u32, NativeCallback)> = factories
        .into_iter()
        .map(|(name, factory)| {
            let body: NativeCallback = Rc::new(move |args| {
                let mut fields = vec![(CLASS_KEY.to_owned(), JsData::Str("FileSystemError".to_owned()))];
                fields.extend(error_fields(&factory(message_or_uri_of(args.first())?)));
                Ok(JsData::Object(fields))
            });
            (name.to_owned(), 1, body)
        })
        .collect();
    let constructor = runner.define_class(ClassSpec {
        name: "FileSystemError".to_owned(),
        length: 1,
        construct: Rc::new(|args| Ok(JsData::Object(error_fields(&FileSystemError::new(message_or_uri_of(args.first())?, FileSystemProviderErrorCode::Unknown, None))))),
        methods: vec![],
        getters: vec![],
        statics,
        parent: None,
    });
    vec![("FileSystemError".to_owned(), JsData::Handle(Rc::new(constructor)))]
}

// ---- vscode.workspace.fs

/// Uma função do `fs`: `(name, Handle)` para o objeto.
fn function(runner: &NodeRunner, name: &str, length: u32, body: impl Fn(&[JsData]) -> Result<JsData, String> + 'static) -> (String, JsData) {
    (name.to_owned(), JsData::Handle(Rc::new(runner.make_function(name, length, Rc::new(body)))))
}

/// O objeto `vscode.FileSystem` sobre o [`ApiFileSystem`].
fn file_system_to_js(runner: &NodeRunner, fs: ApiFileSystem) -> JsData {
    let settle = |fs: &ApiFileSystem| (fs.clone(), runner.clone());
    let mut fields: Vec<(String, JsData)> = Vec::new();

    let (this, promise_runner) = settle(&fs);
    fields.push(function(runner, "stat", 1, move |args| {
        let uri = uri_arg(args, 0, "stat: a uri")?;
        Ok(promise_to_js(&promise_runner, this.stat(&uri), stat_to_js))
    }));
    let (this, promise_runner) = settle(&fs);
    fields.push(function(runner, "readDirectory", 1, move |args| {
        let uri = uri_arg(args, 0, "readDirectory: a uri")?;
        Ok(promise_to_js(&promise_runner, this.read_directory(&uri), directory_to_js))
    }));
    let (this, promise_runner) = settle(&fs);
    fields.push(function(runner, "createDirectory", 1, move |args| {
        let uri = uri_arg(args, 0, "createDirectory: a uri")?;
        Ok(promise_to_js(&promise_runner, this.create_directory(&uri), |()| JsData::Undefined))
    }));
    let (this, promise_runner) = settle(&fs);
    fields.push(function(runner, "readFile", 1, move |args| {
        let uri = uri_arg(args, 0, "readFile: a uri")?;
        Ok(promise_to_js(&promise_runner, this.read_file(&uri), |bytes: &Vec<u8>| bytes_to_js(bytes)))
    }));
    let (this, promise_runner) = settle(&fs);
    fields.push(function(runner, "writeFile", 2, move |args| {
        let uri = uri_arg(args, 0, "writeFile: a uri")?;
        let content = bytes_of(args.get(1))?;
        Ok(promise_to_js(&promise_runner, this.write_file(&uri, &content), |()| JsData::Undefined))
    }));
    let (this, promise_runner) = settle(&fs);
    fields.push(function(runner, "delete", 2, move |args| {
        let uri = uri_arg(args, 0, "delete: a uri")?;
        let options = options_arg(args, 1).map(|options| ApiDeleteOptions {
            recursive: opt_bool(field(options, "recursive")),
            use_trash: opt_bool(field(options, "useTrash")),
        });
        Ok(promise_to_js(&promise_runner, this.delete(&uri, options), |()| JsData::Undefined))
    }));
    let (this, promise_runner) = settle(&fs);
    fields.push(function(runner, "rename", 3, move |args| {
        let (source, target) = (uri_arg(args, 0, "rename: a origem")?, uri_arg(args, 1, "rename: o destino")?);
        Ok(promise_to_js(&promise_runner, this.rename(&source, &target, Some(overwrite_options(args, 2))), |()| JsData::Undefined))
    }));
    let (this, promise_runner) = settle(&fs);
    fields.push(function(runner, "copy", 3, move |args| {
        let (source, target) = (uri_arg(args, 0, "copy: a origem")?, uri_arg(args, 1, "copy: o destino")?);
        Ok(promise_to_js(&promise_runner, this.copy(&source, &target, Some(overwrite_options(args, 2))), |()| JsData::Undefined))
    }));
    fields.push(function(runner, "isWritableFileSystem", 1, move |args| {
        let scheme = string_arg(args, 0, "isWritableFileSystem: o esquema")?;
        Ok(fs.is_writable_file_system(&scheme).map_or(JsData::Undefined, JsData::Bool))
    }));
    JsData::Object(fields)
}

// ---- o provedor da extensão

/// O resultado de um método do provedor como dado: os objetos por referência são copiados (os itens de um array, um a um).
fn result_to_js(runner: &NodeRunner, result: JsResult) -> JsData {
    match result {
        JsResult::Nullish => JsData::Undefined,
        JsResult::Single(value) => exported(runner, &value),
        JsResult::Many(items) => JsData::Array(items.iter().map(|item| exported(runner, item)).collect()),
    }
}

/// `vscode.FileChangeEvent` que o provedor disparou: `{ type, uri }`, por referência.
fn change_of(js: &JsObjects, item: &JsData) -> Result<ApiFileChangeEvent, Error> {
    let change_type = opt_number(Some(&js.property(item, "type"))).and_then(|number| FileChangeType::from_value(number as i32)).ok_or_else(|| Error::new("FileChangeEvent.type inválido"))?;
    let uri = uri_from_js(&exported(js.runner(), &js.property(item, "uri"))).map_err(Error::new)?;
    Ok(ApiFileChangeEvent { change_type, uri })
}

fn changes_of(js: &JsObjects, result: JsResult) -> Result<Vec<ApiFileChangeEvent>, Error> {
    let items = match result {
        JsResult::Nullish => Vec::new(),
        JsResult::Single(item) => vec![item],
        JsResult::Many(items) => items,
    };
    items.iter().map(|item| change_of(js, item)).collect()
}

/// O `vscode.FileSystemProvider` da extensão (por referência, com o `this` e os métodos da classe).
struct JsFileSystemProvider {
    js: JsObjects,
    provider: Rc<JsHandle>,
    table: Rc<DisposableTable>,
}

impl JsFileSystemProvider {
    fn target(&self) -> JsData {
        JsData::Handle(self.provider.clone())
    }

    /// `provider[method](...args)`: o resultado convertido por `convert`. Uma exceção do provedor é uma promessa rejeitada.
    fn call<T: Clone + 'static>(&self, method: &str, args: Vec<JsData>, convert: impl Fn(JsData) -> Result<T, Error> + 'static) -> MaybePromise<T> {
        let runner = self.js.runner().clone();
        convert_maybe(self.js.invoke(&self.target(), method, args), move |result| convert(result_to_js(&runner, result)))
    }

    fn call_void(&self, method: &str, args: Vec<JsData>) -> MaybePromise<()> {
        self.call(method, args, |_| Ok(()))
    }

    fn has_function(&self, name: &str) -> bool {
        self.js.is_function(&self.target(), name)
    }
}

impl ApiFileSystemProvider for JsFileSystemProvider {
    fn on_did_change_file(&self) -> Event<Vec<ApiFileChangeEvent>> {
        let target = self.target();
        let JsData::Handle(event) = self.js.property(&target, "onDidChangeFile") else { return Event::none() };
        if !self.has_function("onDidChangeFile") {
            return Event::none();
        }
        let (js, table) = (self.js.clone(), self.table.clone());
        Event::new(move |listener: Listener<Vec<ApiFileChangeEvent>>| -> DisposableRef {
            let converter = js.clone();
            let subscribed = js.listen(&event, move |result| match changes_of(&converter, result) {
                Ok(changes) => listener(&changes),
                Err(error) => on_unexpected_error(&error),
            });
            match subscribed {
                Ok(Some(id)) => {
                    let table = table.clone();
                    plain_disposable(move || table.dispose(id))
                }
                Ok(None) => plain_disposable(|| {}),
                Err(message) => {
                    on_unexpected_error(&Error::new(message));
                    plain_disposable(|| {})
                }
            }
        })
    }

    fn watch(&self, uri: &URI, options: &WatchOptions) -> DisposableRef {
        let excludes = JsData::Array(options.excludes.iter().cloned().map(JsData::Str).collect());
        let options = JsData::Object(vec![("recursive".to_owned(), JsData::Bool(options.recursive)), ("excludes".to_owned(), excludes)]);
        match self.js.invoke(&self.target(), "watch", vec![uri_to_js(uri), options]) {
            MaybePromise::Value(JsResult::Single(JsData::Handle(disposable))) => {
                let runner = self.js.runner().clone();
                to_disposable(move || {
                    let _ = runner.call_method(&disposable, "dispose", &[]);
                })
            }
            // Uma exceção do `watch` do provedor sobe até o `$watch`, como no original.
            MaybePromise::Promise(promise) => match promise.result() {
                Some(Err(error)) => throw(error),
                _ => to_disposable(|| {}),
            },
            MaybePromise::Value(_) => to_disposable(|| {}),
        }
    }

    fn stat(&self, uri: &URI) -> MaybePromise<ApiFileStat> {
        self.call("stat", vec![uri_to_js(uri)], |data| stat_of(&data))
    }

    fn read_directory(&self, uri: &URI) -> MaybePromise<Vec<(String, FileType)>> {
        self.call("readDirectory", vec![uri_to_js(uri)], |data| directory_of(&data))
    }

    fn create_directory(&self, uri: &URI) -> MaybePromise<()> {
        self.call_void("createDirectory", vec![uri_to_js(uri)])
    }

    fn read_file(&self, uri: &URI) -> MaybePromise<Vec<u8>> {
        self.call("readFile", vec![uri_to_js(uri)], |data| bytes_of(Some(&data)).map_err(Error::new))
    }

    fn write_file(&self, uri: &URI, content: &[u8], options: &FileWriteOptions) -> MaybePromise<()> {
        self.call_void("writeFile", vec![uri_to_js(uri), bytes_to_js(content), flag_object(&[("create", options.create), ("overwrite", options.overwrite)])])
    }

    fn delete(&self, uri: &URI, options: &FileDeleteOptions) -> MaybePromise<()> {
        self.call_void("delete", vec![uri_to_js(uri), flag_object(&[("recursive", options.recursive)])])
    }

    fn rename(&self, old_uri: &URI, new_uri: &URI, options: &FileOverwriteOptions) -> MaybePromise<()> {
        self.call_void("rename", vec![uri_to_js(old_uri), uri_to_js(new_uri), flag_object(&[("overwrite", options.overwrite)])])
    }

    fn has_copy(&self) -> bool {
        self.has_function("copy")
    }

    fn copy(&self, source: &URI, destination: &URI, options: &FileOverwriteOptions) -> MaybePromise<()> {
        self.call_void("copy", vec![uri_to_js(source), uri_to_js(destination), flag_object(&[("overwrite", options.overwrite)])])
    }

    fn has_open_close_read_write(&self) -> bool {
        ["open", "close", "read", "write"].iter().all(|name| self.has_function(name))
    }
}

/// `{ isCaseSensitive?, isReadonly? }` de `registerFileSystemProvider`.
fn provider_options_of(runner: &NodeRunner, value: Option<&JsData>) -> Result<FileSystemProviderOptions, String> {
    let Some(value) = value.filter(|options| !is_nullish(Some(*options))) else { return Ok(FileSystemProviderOptions::default()) };
    let options = exported(runner, value);
    let is_readonly = match field(&options, "isReadonly") {
        Some(JsData::Bool(flag)) => Some(FileSystemReadonly::Flag(*flag)),
        Some(markdown @ JsData::Object(_)) => Some(FileSystemReadonly::Message(markdown_from_js(markdown).map_err(|error| error.message)?)),
        _ => None,
    };
    Ok(FileSystemProviderOptions { is_case_sensitive: opt_bool(field(&options, "isCaseSensitive")).unwrap_or(false), is_readonly })
}

fn register_provider(runner: &NodeRunner, api: &FileSystemApi, args: &[JsData]) -> Result<JsData, String> {
    let scheme = string_arg(args, 0, "registerFileSystemProvider: o esquema")?;
    let Some(JsData::Handle(provider)) = args.get(1) else { return Err("registerFileSystemProvider: o provedor precisa ser um objeto".to_owned()) };
    let options = provider_options_of(runner, args.get(2))?;
    let service = api
        .provider
        .borrow()
        .clone()
        .ok_or("registerFileSystemProvider: o host não tem os clientes de linguagem (MainThreadLanguageFeatures e MainThreadDiagnostics)")?;
    let adapter: Rc<dyn ApiFileSystemProvider> = Rc::new(JsFileSystemProvider { js: api.js.clone(), provider: provider.clone(), table: api.table.clone() });
    // Como no `extHost.api.impl.ts`: o registro no main thread e o atalho do consumidor (`workspace.fs` fala com o provedor local
    // sem atravessar o protocolo) saem juntos, num só `Disposable`.
    let consumer = api.consumer.add_file_system_provider(&scheme, adapter.clone(), Some(&options));
    let registration = service.register_file_system_provider(&host_extension(), &scheme, adapter, options).map_err(|error| error.message)?;
    Ok(api.table.to_js(combined_disposable(vec![registration, consumer])))
}

/// Os membros de `vscode.workspace` do sistema de arquivos: `fs` e `registerFileSystemProvider`.
pub fn workspace_members(runner: &NodeRunner, api: &FileSystemApi) -> FileSystemMembers {
    let (fs_runner, consumer) = (runner.clone(), api.consumer.clone());
    let getters: Vec<(String, MethodCallback)> = vec![("fs".to_owned(), Rc::new(move |_, _| Ok(file_system_to_js(&fs_runner, consumer.value()))))];

    let (register_runner, register_api) = (runner.clone(), api.clone());
    let register = runner.make_function_by_reference("registerFileSystemProvider", 3, Rc::new(move |args| register_provider(&register_runner, &register_api, args)));
    FileSystemMembers { getters, references: vec![("registerFileSystemProvider".to_owned(), JsData::Handle(Rc::new(register)))] }
}
