//! Teste de ponta a ponta de `vscode.workspace.fs` e `vscode.workspace.registerFileSystemProvider` (não é porte do oráculo): uma extensão
//! de exemplo em JavaScript, ativada pelo host local (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), registra um
//! provedor de arquivos em memória (`memfs`), grava, lê, lista, renomeia, copia e apaga pelo `workspace.fs`, e observa os erros
//! (`FileSystemError`) tanto do provedor da extensão quanto da thread principal.
//!
//! O lado do ExtHost é o `ExtHostConsumerFileSystem` e o `ExtHostFileSystem` REAIS (portados de `extHostFileSystemConsumer.ts` e
//! `extHostFileSystem.ts`), ligados pelo host em processo. O lado do workbench é um dublê do `MainThreadFileSystemShape` ([`Panel`]) que
//! anota cada chamada (`$registerFileSystemProvider`, `$unregisterProvider`, `$onFileSystemChange`, `$rename`, `$copy`...), faz o papel
//! da thread principal para `rename` e `copy` (repassa ao provedor pelo proxy `ExtHostFileSystemShape`) e responde a um esquema sem
//! provedor (`other`). O binder junta o dublê aos clientes de documentos e linguagens do suporte de `languages`, porque o registro do
//! provedor também registra um provedor de links de documento (`ExtHostLanguageFeatures`).

#[allow(dead_code)]
#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/extension_host_api_harness.rs"]
mod harness;
#[allow(dead_code, unused_imports)]
#[path = "support/main_thread_support.rs"]
mod main_thread_support;

/// O suporte de editor do `je-editor/tests/support` que o `TestCodeEditorService` precisa, no mesmo módulo pai (os arquivos se referem uns
/// aos outros por `super::`).
#[allow(dead_code, unused_imports)]
#[path = "../../je-editor/tests/support"]
mod editor_support {
    pub mod editor_test_services;
    pub mod test_clipboard_service;
    pub mod test_code_editor;
    pub mod test_code_editor_service;
    pub mod test_color_theme_service;
    pub mod test_configuration;
    pub mod test_instantiation_service;
    pub mod test_language_configuration_service;
    pub mod test_notification_service;
    pub mod test_text_model;
    pub mod test_theme_service;
}
#[path = "support/extension_host_api_languages_support.rs"]
mod languages_support;

use std::cell::RefCell;
use std::rc::Rc;

use harness::{run_extension, settled_ok, Workbench};
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::html_content::IMarkdownString;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_exthost::in_process::IMainThreadBinder;
use je_platform::files::files::{
    FileChangeType as ProtocolFileChangeType, FileDeleteOptions, FileOverwriteOptions, FileSystemProviderCapabilities, FileType, Stat, WatchOptions,
};
use je_workbench::api::common::ext_host_protocol::{ExtHostContext, ExtHostFileSystemShape, FileChangeDto, IMainContext, MainContext, MainThreadFileSystemShape};
use je_workbench::api::common::ext_host_types::{FileSystemError, MessageOrUri};
use languages_support::{languages_setup, Observed};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__events = [];
  const log = (text) => globalThis.__events.push(text);
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));

  const files = new Map();
  const dirs = new Set(['/']);
  const emitter = new vscode.EventEmitter();
  globalThis.__emitter = emitter;
  const parentOf = (path) => path.substring(0, path.lastIndexOf('/')) || '/';
  const nameOf = (path) => path.substring(path.lastIndexOf('/') + 1);
  const notFound = (uri) => vscode.FileSystemError.FileNotFound(uri);

  const provider = {
    onDidChangeFile: emitter.event,
    watch: (uri, options) => {
      log('watch:' + uri.path + ':' + options.recursive);
      return new vscode.Disposable(() => log('unwatch'));
    },
    stat: (uri) => {
      if (dirs.has(uri.path)) return { type: vscode.FileType.Directory, ctime: 1, mtime: 2, size: 0 };
      if (files.has(uri.path)) return { type: vscode.FileType.File, ctime: 1, mtime: 3, size: files.get(uri.path).length };
      throw notFound(uri);
    },
    readDirectory: (uri) => {
      const entries = [];
      for (const path of files.keys()) if (parentOf(path) === uri.path) entries.push([nameOf(path), vscode.FileType.File]);
      for (const path of dirs) if (path !== '/' && parentOf(path) === uri.path) entries.push([nameOf(path), vscode.FileType.Directory]);
      return entries.sort((a, b) => (a[0] < b[0] ? -1 : 1));
    },
    createDirectory: (uri) => {
      log('mkdir:' + uri.path);
      dirs.add(uri.path);
    },
    readFile: async (uri) => {
      if (!files.has(uri.path)) throw notFound(uri);
      return files.get(uri.path);
    },
    writeFile: (uri, content, options) => {
      log('write:' + uri.path + ':' + options.create + ':' + options.overwrite);
      files.set(uri.path, Array.from(content));
    },
    delete: (uri, options) => {
      log('delete:' + uri.path + ':' + options.recursive);
      files.delete(uri.path);
      dirs.delete(uri.path);
    },
    rename: (from, to, options) => {
      log('rename:' + from.path + '>' + to.path + ':' + options.overwrite);
      files.set(to.path, files.get(from.path));
      files.delete(from.path);
    },
    copy: (from, to, options) => {
      log('copy:' + from.path + '>' + to.path + ':' + options.overwrite);
      files.set(to.path, files.get(from.path).slice());
    },
  };

  command('t.fsRegister', () => {
    globalThis.__registration = vscode.workspace.registerFileSystemProvider('memfs', provider, { isCaseSensitive: true });
    return 'ok';
  });

  command('t.fsRegisterTwice', () => {
    try {
      vscode.workspace.registerFileSystemProvider('memfs', provider);
      return 'registrou';
    } catch (error) {
      return error.message;
    }
  });

  command('t.fsWrite', async () => {
    const fs = vscode.workspace.fs;
    const uri = vscode.Uri.parse('memfs:/docs/a.txt');
    await fs.writeFile(uri, [104, 105]);
    const bytes = await fs.readFile(uri);
    const stat = await fs.stat(uri);
    const listing = await fs.readDirectory(vscode.Uri.parse('memfs:/docs'));
    const root = await fs.readDirectory(vscode.Uri.parse('memfs:/'));
    return JSON.stringify({ bytes, size: stat.size, type: stat.type, listing, root, writable: String(fs.isWritableFileSystem('memfs')) });
  });

  command('t.fsMove', async () => {
    const fs = vscode.workspace.fs;
    const a = vscode.Uri.parse('memfs:/docs/a.txt');
    const b = vscode.Uri.parse('memfs:/docs/b.txt');
    const c = vscode.Uri.parse('memfs:/docs/c.txt');
    await fs.rename(a, b, { overwrite: true });
    await fs.copy(b, c);
    await fs.delete(b);
    return JSON.stringify(await fs.readDirectory(vscode.Uri.parse('memfs:/docs')));
  });

  command('t.fsErrors', async () => {
    const fs = vscode.workspace.fs;
    const out = {};
    try { await fs.readFile(vscode.Uri.parse('memfs:/nope.txt')); } catch (error) { out.provider = error.name + '|' + error.message; }
    try { await fs.stat(vscode.Uri.parse('other:/missing')); } catch (error) { out.main = error.name + '|' + error.message; }
    const stat = await fs.stat(vscode.Uri.parse('other:/file'));
    out.other = [stat.type, stat.size, stat.permissions];
    out.otherBytes = await fs.readFile(vscode.Uri.parse('other:/file'));
    out.otherListing = await fs.readDirectory(vscode.Uri.parse('other:/'));
    const exists = vscode.FileSystemError.FileExists('x');
    out.factory = exists.name + '|' + exists.code + '|' + exists.message;
    return JSON.stringify(out);
  });

  command('t.fsFire', () => {
    globalThis.__emitter.fire([
      { type: vscode.FileChangeType.Created, uri: vscode.Uri.parse('memfs:/docs/n.txt') },
      { type: vscode.FileChangeType.Deleted, uri: vscode.Uri.parse('other:/skip') },
      { type: vscode.FileChangeType.Changed, uri: vscode.Uri.parse('memfs:/docs/a.txt') },
    ]);
    return 'ok';
  });

  command('t.fsDispose', () => {
    globalThis.__registration.dispose();
    return 'ok';
  });
};
"#;

fn text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

fn protocol_change(change: ProtocolFileChangeType) -> &'static str {
    match change {
        ProtocolFileChangeType::Updated => "Updated",
        ProtocolFileChangeType::Added => "Added",
        ProtocolFileChangeType::Deleted => "Deleted",
    }
}

/// O papel da thread principal: o `MainThreadFileSystemShape` que anota as chamadas.
struct Panel {
    ext_host: RefCell<Option<Rc<dyn ExtHostFileSystemShape>>>,
    log: RefCell<Vec<String>>,
    /// Os pares (esquema, `handle`) dos provedores registrados.
    handles: RefCell<Vec<(String, u32)>>,
}

impl Panel {
    fn new() -> Rc<Self> {
        Rc::new(Self { ext_host: RefCell::new(None), log: RefCell::new(Vec::new()), handles: RefCell::new(Vec::new()) })
    }

    fn ext_host(&self) -> Rc<dyn ExtHostFileSystemShape> {
        self.ext_host.borrow().clone().expect("o host já subiu e ligou o dublê")
    }

    fn note(&self, line: String) {
        self.log.borrow_mut().push(line);
    }

    fn lines(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn handle_of(&self, scheme: &str) -> u32 {
        self.handles.borrow().iter().find(|(known, _)| known == scheme).map(|(_, handle)| *handle).expect("o esquema foi registrado")
    }
}

impl IDisposable for Panel {
    fn dispose(&self) {}
}

impl MainThreadFileSystemShape for Panel {
    fn rpc_register_file_system_provider(&self, handle: u32, scheme: &str, capabilities: FileSystemProviderCapabilities, readonly_message: Option<IMarkdownString>) -> Promise<()> {
        self.handles.borrow_mut().push((scheme.to_owned(), handle));
        self.note(format!("register:{scheme}:{}:{}", capabilities.bits(), readonly_message.map_or_else(|| "writable".to_owned(), |message| message.value.to_string_lossy())));
        Promise::resolved(())
    }

    fn rpc_unregister_provider(&self, handle: u32) {
        self.note(format!("unregister:{handle}"));
    }

    fn rpc_on_file_system_change(&self, handle: u32, resource: Vec<FileChangeDto>) {
        let changes: Vec<String> = resource.iter().map(|change| format!("{}:{}", protocol_change(change.change_type), text(&change.resource))).collect();
        self.note(format!("change:{handle}:{}", changes.join(",")));
    }

    fn rpc_stat(&self, resource: &URI) -> Promise<Stat> {
        match resource.path().to_string_lossy().as_str() {
            "/missing" => Promise::rejected(FileSystemError::file_not_found(Some(MessageOrUri::Message("missing".to_owned()))).error().clone()),
            _ => Promise::resolved(Stat { file_type: FileType::FILE, mtime: 5, ctime: 4, size: 7, permissions: None }),
        }
    }

    fn rpc_readdir(&self, _resource: &URI) -> Promise<Vec<(String, FileType)>> {
        Promise::resolved(vec![("x".to_owned(), FileType::FILE)])
    }

    fn rpc_read_file(&self, _resource: &URI) -> Promise<VSBuffer> {
        Promise::resolved(VSBuffer::wrap(vec![1, 2, 3]))
    }

    fn rpc_write_file(&self, resource: &URI, _content: VSBuffer) -> Promise<()> {
        Promise::rejected(Error::new(format!("o teste não grava em {}", text(resource))))
    }

    fn rpc_rename(&self, resource: &URI, target: &URI, opts: FileOverwriteOptions) -> Promise<()> {
        self.note(format!("rename:{}>{}:{}", text(resource), text(target), opts.overwrite));
        self.ext_host().rpc_rename(self.handle_of(&resource.scheme().to_string_lossy()), resource, target, opts)
    }

    fn rpc_copy(&self, resource: &URI, target: &URI, opts: FileOverwriteOptions) -> Promise<()> {
        self.note(format!("copy:{}>{}:{}", text(resource), text(target), opts.overwrite));
        self.ext_host().rpc_copy(self.handle_of(&resource.scheme().to_string_lossy()), resource, target, opts)
    }

    fn rpc_mkdir(&self, resource: &URI) -> Promise<()> {
        Promise::rejected(Error::new(format!("o teste não cria {}", text(resource))))
    }

    fn rpc_delete(&self, resource: &URI, _opts: FileDeleteOptions) -> Promise<()> {
        Promise::rejected(Error::new(format!("o teste não apaga {}", text(resource))))
    }

    fn rpc_ensure_activation(&self, _scheme: &str) -> Promise<()> {
        Promise::resolved(())
    }
}

/// Liga o dublê da thread principal ao host, junto dos clientes de documentos e linguagens do suporte de `languages`.
struct Binder {
    documents: Rc<dyn IMainThreadBinder>,
    panel: Rc<Panel>,
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        let disposables = self.documents.bind(context.clone());
        *self.panel.ext_host.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_FILE_SYSTEM));
        let panel: Rc<dyn MainThreadFileSystemShape> = self.panel.clone();
        context.set(&MainContext::MAIN_THREAD_FILE_SYSTEM, panel);
        disposables
    }
}

fn setup(_workbench: &Workbench) -> (Rc<dyn IMainThreadBinder>, (Observed, Rc<Panel>)) {
    let (documents, observed) = languages_setup();
    let panel = Panel::new();
    (Rc::new(Binder { documents, panel: panel.clone() }), (observed, panel))
}

fn run(workbench: &Workbench, command: &str) -> Dynamic {
    workbench.execute(command, Vec::new())
}

fn events(workbench: &Workbench) -> String {
    workbench.probe("JSON.stringify(globalThis.__events)")
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn memfs_uri(path: &str) -> URI {
    URI::parse(format!("memfs:{path}"), false)
}

#[test]
fn register_file_system_provider_announces_the_scheme_with_its_capabilities_and_rejects_a_second_one() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        assert_eq!(run(workbench, "t.fsRegister"), Dynamic::string("ok"));
        let capabilities = FileSystemProviderCapabilities::FILE_READ_WRITE | FileSystemProviderCapabilities::PATH_CASE_SENSITIVE | FileSystemProviderCapabilities::FILE_FOLDER_COPY;
        assert_eq!(panel.lines(), vec![format!("register:memfs:{}:writable", capabilities.bits())]);

        assert_eq!(run(workbench, "t.fsRegisterTwice"), Dynamic::string("a provider for the scheme 'memfs' is already registered"));
    });
}

#[test]
fn workspace_fs_writes_reads_stats_and_lists_through_the_registered_provider() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, _panel)| {
        run(workbench, "t.fsRegister");
        assert_eq!(
            run(workbench, "t.fsWrite"),
            Dynamic::string(r#"{"bytes":[104,105],"size":2,"type":1,"listing":[["a.txt",1]],"root":[["docs",2]],"writable":"undefined"}"#),
            "o conteúdo atravessa como lista de bytes; o mkdirp criou a pasta antes da gravação"
        );
        assert_eq!(events(workbench), r#"["mkdir:/docs","write:/docs/a.txt:true:true"]"#);
    });
}

#[test]
fn workspace_fs_rename_and_copy_go_through_the_main_thread_and_delete_takes_the_shortcut() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.fsRegister");
        run(workbench, "t.fsWrite");
        assert_eq!(run(workbench, "t.fsMove"), Dynamic::string(r#"[["c.txt",1]]"#));

        let lines = panel.lines();
        assert_eq!(
            lines[1..].to_vec(),
            strings(&["rename:memfs:/docs/a.txt>memfs:/docs/b.txt:true", "copy:memfs:/docs/b.txt>memfs:/docs/c.txt:false"]),
            "rename e copy não têm atalho: passam pela thread principal"
        );
        assert_eq!(
            events(workbench),
            r#"["mkdir:/docs","write:/docs/a.txt:true:true","rename:/docs/a.txt>/docs/b.txt:true","copy:/docs/b.txt>/docs/c.txt:false","delete:/docs/b.txt:false"]"#
        );
    });
}

#[test]
fn workspace_fs_reports_file_system_errors_and_serves_schemes_without_a_provider_from_the_main_thread() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, _panel)| {
        run(workbench, "t.fsRegister");
        assert_eq!(
            run(workbench, "t.fsErrors"),
            Dynamic::string(
                r#"{"provider":"EntryNotFound (FileSystemError)|memfs:/nope.txt","main":"EntryNotFound (FileSystemError)|missing","other":[1,7,null],"otherBytes":[1,2,3],"otherListing":[["x",1]],"factory":"EntryExists (FileSystemError)|FileExists|x"}"#
            )
        );
    });
}

#[test]
fn the_main_thread_calls_the_provider_and_receives_change_events_and_the_unregistration() {
    run_extension(EXTENSION_SOURCE, setup, |workbench, (_observed, panel)| {
        run(workbench, "t.fsRegister");
        run(workbench, "t.fsWrite");
        let handle = panel.handle_of("memfs");

        let stat = panel.ext_host().rpc_stat(handle, &memfs_uri("/docs/a.txt"));
        let content = panel.ext_host().rpc_read_file(handle, &memfs_uri("/docs/a.txt"));
        workbench.pump();
        let stat = settled_ok(&stat, "$stat");
        assert_eq!((stat.file_type, stat.size, stat.mtime, stat.ctime), (FileType::FILE, 2, 3, 1));
        assert_eq!(settled_ok(&content, "$readFile").to_vec(), vec![104, 105]);

        panel.ext_host().rpc_watch(handle, 7, &memfs_uri("/docs"), WatchOptions { recursive: true, ..WatchOptions::default() });
        panel.ext_host().rpc_unwatch(handle, 7);
        assert_eq!(events(workbench), r#"["mkdir:/docs","write:/docs/a.txt:true:true","watch:/docs:true","unwatch"]"#);

        run(workbench, "t.fsFire");
        assert_eq!(
            panel.lines().last().cloned(),
            Some(format!("change:{handle}:Added:memfs:/docs/n.txt,Updated:memfs:/docs/a.txt")),
            "o evento de outro esquema é descartado"
        );

        run(workbench, "t.fsDispose");
        assert_eq!(panel.lines().last().cloned(), Some(format!("unregister:{handle}")));
    });
}
