//! Teste de ponta a ponta do `ExtensionContext` (não é porte do oráculo): uma extensão de exemplo em JavaScript, ativada pelo host local
//! (`AbstractExtensionService` + `LocalExtensionHost` + `RPCProtocol`), lê e escreve `globalState` e `workspaceState` (que persistem
//! pelo `MainThreadStorage` e voltam na leitura seguinte), chama `setKeysForSync`, lê os URIs e caminhos de extensão, armazenamento e log,
//! usa `secrets` e a `environmentVariableCollection`.
//!
//! O lado do ExtHost é o REAL (`_loadExtensionContext`, `ExtensionMemento`, `ExtensionSecrets`, `ExtHostStorage`, `ExtHostSecretState` e
//! o `ExtHostTerminalService`), ligado pelo host em processo. Os clientes do workbench são dublês: o `MainThreadStorage` guarda o texto
//! JSON de cada documento (como o armazenamento do workbench), o `MainThreadSecretState` guarda os segredos e o
//! `MainThreadTerminalService` só anota a sincronização da coleção de variáveis de ambiente.

#[path = "support/extension_host_api_harness.rs"]
mod harness;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use harness::{run_extension, Workbench};
use je_base::dynamic::{json_stringify, Dynamic};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_exthost::extension_storage_paths::ExtensionStorageEnvironment;
use je_exthost::in_process::IMainThreadBinder;
use je_platform::terminal::environment_variable::{ISerializableEnvironmentDescriptionMap, ISerializableEnvironmentVariableCollection};
use je_platform::terminal::terminal::{IProcessProperty, IProcessReadyWindowsPty};
use je_workbench::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostSecretStateShape, ExtHostStorageShape, ExtHostTerminalIdentifier, IExtensionIdWithVersion, IMainContext, MainContext, MainThreadSecretStateShape,
    MainThreadStorageShape, MainThreadTerminalServiceShape, TerminalLaunchConfig,
};

/// O JavaScript da extensão de exemplo.
const EXTENSION_SOURCE: &str = r#"
const vscode = require('vscode');
exports.activate = (context) => {
  globalThis.__activated = true;
  globalThis.__events = [];
  const log = (text) => globalThis.__events.push(text);
  const command = (id, handler) => context.subscriptions.push(vscode.commands.registerCommand(id, handler));

  command('t.write', () => {
    globalThis.__updates = Promise.all([
      context.globalState.update('count', 3),
      context.globalState.update('config', { a: [1, 2], b: 'x' }),
      context.workspaceState.update('wsKey', 'ws'),
    ]).then(() => { globalThis.__written = true; });
    context.globalState.setKeysForSync(['count']);
    return JSON.stringify({
      globalKeys: context.globalState.keys(),
      count: context.globalState.get('count'),
      missing: context.globalState.get('nope', 'padrao'),
      config: context.globalState.get('config'),
      workspaceKeys: context.workspaceState.keys(),
      workspaceMissing: String(context.workspaceState.get('nope')),
    });
  });

  command('t.read', () => JSON.stringify({
    globalKeys: context.globalState.keys(),
    seed: context.globalState.get('seed'),
    n: context.globalState.get('n'),
    workspaceKeys: context.workspaceState.keys(),
    ws: context.workspaceState.get('wsSeed'),
  }));

  command('t.removeKey', () => {
    context.globalState.update('count', undefined);
    return JSON.stringify({ keys: context.globalState.keys(), count: String(context.globalState.get('count')) });
  });

  command('t.paths', () => JSON.stringify({
    extensionUri: context.extensionUri.toString(),
    extensionPathMatches: context.extensionPath === context.extensionUri.fsPath,
    absolute: context.asAbsolutePath('media/a.png') === context.extensionPath + '/media/a.png',
    logUri: context.logUri.toString(),
    logPathEnds: context.logPath.endsWith('je-test.api'),
    globalStorageUri: context.globalStorageUri.toString(),
    globalStoragePathEnds: context.globalStoragePath.endsWith('je-test.api'),
    storageUri: context.storageUri.toString(),
    storagePathEnds: context.storagePath.endsWith('je-test.api'),
    extensionId: context.extension.id,
    mode: context.extensionMode,
  }));

  command('t.secrets', () => {
    context.secrets.onDidChange((event) => log('secret:' + event.key));
    globalThis.__secretRun = (async () => {
      await context.secrets.store('token', 'abc');
      await context.secrets.store('other', 'def');
      const got = await context.secrets.get('token');
      await context.secrets.delete('other');
      const keys = await context.secrets.keys();
      globalThis.__secret = JSON.stringify({ got, missing: String(await context.secrets.get('nope')), keys });
    })();
    return 'started';
  });

  command('t.env', () => {
    const collection = context.environmentVariableCollection;
    collection.replace('JE_A', 'a');
    collection.append('JE_B', 'b', { applyAtProcessCreation: true });
    collection.prepend('JE_C', 'c');
    collection.description = 'Variáveis do teste';
    const seen = [];
    collection.forEach((variable, mutator) => seen.push(variable + ':' + mutator.value + ':' + mutator.type));
    const iterated = [];
    for (const [variable, mutator] of collection) iterated.push(variable + '/' + mutator.variable);
    const a = collection.get('JE_A');
    const b = collection.get('JE_B');
    collection.delete('JE_C');
    return JSON.stringify({
      persistent: collection.persistent,
      a: a.value + ':' + a.type,
      bOptions: b.options.applyAtProcessCreation + '/' + String(b.options.applyAtShellIntegration),
      seen,
      iterated,
      removed: String(collection.get('JE_C')),
      description: collection.description,
    });
  });

  command('t.envClear', () => {
    context.environmentVariableCollection.persistent = false;
    context.environmentVariableCollection.clear();
    return String(context.environmentVariableCollection.persistent);
  });
};
"#;

/// O `MainThreadStorage`: o texto JSON de cada documento (`shared`, extensão) e o que a extensão pediu.
struct Storage {
    documents: RefCell<HashMap<(bool, String), String>>,
    calls: RefCell<Vec<String>>,
}

impl IDisposable for Storage {
    fn dispose(&self) {}
}

impl MainThreadStorageShape for Storage {
    fn rpc_initialize_extension_storage(&self, shared: bool, extension_id: &str) -> Promise<Option<String>> {
        self.calls.borrow_mut().push(format!("init {shared} {extension_id}"));
        Promise::resolved(self.documents.borrow().get(&(shared, extension_id.to_owned())).cloned())
    }

    fn rpc_set_value(&self, shared: bool, extension_id: &str, value: Dynamic) -> Promise<()> {
        let text = json_stringify(&value).expect("o valor do memento serializa");
        self.calls.borrow_mut().push(format!("set {shared} {extension_id} {text}"));
        self.documents.borrow_mut().insert((shared, extension_id.to_owned()), text);
        Promise::resolved(())
    }

    fn rpc_register_extension_storage_keys_to_sync(&self, extension: IExtensionIdWithVersion, keys: Vec<String>) {
        self.calls.borrow_mut().push(format!("sync {}@{} {keys:?}", extension.id, extension.version));
    }
}

/// O `MainThreadSecretState`: os segredos por (extensão, chave).
struct Secrets {
    values: RefCell<Vec<((String, String), String)>>,
}

impl IDisposable for Secrets {
    fn dispose(&self) {}
}

impl MainThreadSecretStateShape for Secrets {
    fn rpc_get_password(&self, extension_id: &str, key: &str) -> Promise<Option<String>> {
        let found = self.values.borrow().iter().find(|((id, name), _)| id == extension_id && name == key).map(|(_, value)| value.clone());
        Promise::resolved(found)
    }

    fn rpc_set_password(&self, extension_id: &str, key: &str, value: &str) -> Promise<()> {
        let mut values = self.values.borrow_mut();
        values.retain(|((id, name), _)| !(id == extension_id && name == key));
        values.push(((extension_id.to_owned(), key.to_owned()), value.to_owned()));
        Promise::resolved(())
    }

    fn rpc_delete_password(&self, extension_id: &str, key: &str) -> Promise<()> {
        self.values.borrow_mut().retain(|((id, name), _)| !(id == extension_id && name == key));
        Promise::resolved(())
    }

    fn rpc_get_keys(&self, extension_id: &str) -> Promise<Vec<String>> {
        Promise::resolved(self.values.borrow().iter().filter(|((id, _), _)| id == extension_id).map(|((_, name), _)| name.clone()).collect())
    }
}

/// O `MainThreadTerminalService`: só a sincronização da coleção de variáveis de ambiente importa.
struct Terminal {
    syncs: RefCell<Vec<(String, bool, Option<Vec<String>>)>>,
}

impl IDisposable for Terminal {
    fn dispose(&self) {}
}

impl MainThreadTerminalServiceShape for Terminal {
    fn rpc_create_terminal(&self, _ext_host_terminal_id: &str, _config: TerminalLaunchConfig) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_dispose(&self, _id: &ExtHostTerminalIdentifier) {}

    fn rpc_hide(&self, _id: &ExtHostTerminalIdentifier) {}

    fn rpc_send_text(&self, _id: &ExtHostTerminalIdentifier, _text: &str, _should_execute: bool) {}

    fn rpc_show(&self, _id: &ExtHostTerminalIdentifier, _preserve_focus: bool) {}

    fn rpc_register_process_support(&self, _is_supported: bool) {}

    fn rpc_register_profile_provider(&self, _id: &str, _extension_identifier: &str) {}

    fn rpc_unregister_profile_provider(&self, _id: &str) {}

    fn rpc_register_completion_provider(&self, _id: &str, _extension_identifier: &str, _trigger_characters: &[String]) {}

    fn rpc_unregister_completion_provider(&self, _id: &str) {}

    fn rpc_register_quick_fix_provider(&self, _id: &str, _extension_identifier: &str) {}

    fn rpc_unregister_quick_fix_provider(&self, _id: &str) {}

    fn rpc_set_environment_variable_collection(
        &self,
        extension_identifier: &str,
        persistent: bool,
        collection: Option<ISerializableEnvironmentVariableCollection>,
        _description_map: ISerializableEnvironmentDescriptionMap,
    ) {
        let described = collection.map(|collection| collection.iter().map(|(name, mutator)| format!("{name}={}:{}", mutator.value, mutator.mutator_type as i32)).collect());
        self.syncs.borrow_mut().push((extension_identifier.to_owned(), persistent, described));
    }

    fn rpc_start_sending_data_events(&self) {}

    fn rpc_stop_sending_data_events(&self) {}

    fn rpc_start_sending_command_events(&self) {}

    fn rpc_stop_sending_command_events(&self) {}

    fn rpc_start_link_provider(&self) {}

    fn rpc_stop_link_provider(&self) {}

    fn rpc_send_process_data(&self, _terminal_id: u32, _data: &str) {}

    fn rpc_send_process_ready(&self, _terminal_id: u32, _pid: i32, _cwd: &str, _windows_pty: Option<IProcessReadyWindowsPty>) {}

    fn rpc_send_process_property(&self, _terminal_id: u32, _property: IProcessProperty) {}

    fn rpc_send_process_exit(&self, _terminal_id: u32, _exit_code: Option<i32>) {}
}

/// O que o teste observa do workbench.
struct Panel {
    storage: Rc<Storage>,
    secrets: Rc<Secrets>,
    terminal: Rc<Terminal>,
    ext_host_storage: RefCell<Option<Rc<dyn ExtHostStorageShape>>>,
    ext_host_secrets: RefCell<Option<Rc<dyn ExtHostSecretStateShape>>>,
}

/// Liga os três dublês ao host e entrega os diretórios de log e de armazenamento.
struct Binder {
    panel: Rc<Panel>,
}

fn directory(path: &str) -> URI {
    URI::file(path.to_owned(), je_platform::resources::host_platform())
}

impl IMainThreadBinder for Binder {
    fn bind(&self, context: Rc<dyn IMainContext>) -> Vec<Rc<dyn IDisposable>> {
        *self.panel.ext_host_storage.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_STORAGE));
        *self.panel.ext_host_secrets.borrow_mut() = Some(context.get_proxy(&ExtHostContext::EXT_HOST_SECRET_STATE));
        let storage: Rc<dyn MainThreadStorageShape> = self.panel.storage.clone();
        context.set(&MainContext::MAIN_THREAD_STORAGE, storage);
        let secrets: Rc<dyn MainThreadSecretStateShape> = self.panel.secrets.clone();
        context.set(&MainContext::MAIN_THREAD_SECRET_STATE, secrets);
        let terminal: Rc<dyn MainThreadTerminalServiceShape> = self.panel.terminal.clone();
        context.set(&MainContext::MAIN_THREAD_TERMINAL_SERVICE, terminal);
        Vec::new()
    }

    fn extension_storage_environment(&self) -> Option<ExtensionStorageEnvironment> {
        Some(ExtensionStorageEnvironment {
            logs_location: directory("/je/logs"),
            global_storage_home: directory("/je/global"),
            workspace_storage: Some(directory("/je/workspace/ws1")),
        })
    }
}

fn setup_with(documents: &[(bool, &str, &str)]) -> impl FnOnce(&Workbench) -> (Rc<dyn IMainThreadBinder>, Rc<Panel>) {
    let seeded: HashMap<(bool, String), String> = documents.iter().map(|(shared, id, text)| ((*shared, (*id).to_owned()), (*text).to_owned())).collect();
    move |_workbench| {
        let panel = Rc::new(Panel {
            storage: Rc::new(Storage { documents: RefCell::new(seeded), calls: RefCell::new(Vec::new()) }),
            secrets: Rc::new(Secrets { values: RefCell::new(Vec::new()) }),
            terminal: Rc::new(Terminal { syncs: RefCell::new(Vec::new()) }),
            ext_host_storage: RefCell::new(None),
            ext_host_secrets: RefCell::new(None),
        });
        (Rc::new(Binder { panel: panel.clone() }), panel)
    }
}

fn run(workbench: &Workbench, command: &str) -> String {
    match workbench.execute(command, Vec::new()) {
        Dynamic::String(text) => text.to_string_lossy(),
        other => panic!("o comando {command} devolveu {other:?}"),
    }
}

/// Deixa o `RunOnceScheduler` de 0 ms do memento gravar e esvazia o laço.
fn settle(workbench: &Workbench) {
    std::thread::sleep(Duration::from_millis(50));
    workbench.pump();
}

#[test]
fn memento_updates_persist_through_the_storage_and_read_back() {
    run_extension(EXTENSION_SOURCE, setup_with(&[]), |workbench, panel| {
        assert_eq!(
            run(workbench, "t.write"),
            r#"{"globalKeys":["count","config"],"count":3,"missing":"padrao","config":{"a":[1,2],"b":"x"},"workspaceKeys":["wsKey"],"workspaceMissing":"undefined"}"#,
            "as leituras logo depois do update já enxergam o valor novo (sem esperar a gravação)"
        );
        settle(workbench);

        assert_eq!(workbench.probe("String(globalThis.__written)"), "true", "as promessas de update assentam depois da gravação");
        let documents = panel.storage.documents.borrow();
        assert_eq!(documents[&(true, "je-test.api".to_owned())], r#"{"count":3,"config":{"a":[1,2],"b":"x"}}"#);
        assert_eq!(documents[&(false, "je-test.api".to_owned())], r#"{"wsKey":"ws"}"#);

        let calls = panel.storage.calls.borrow().clone();
        assert!(calls.contains(&"init true je-test.api".to_owned()) && calls.contains(&"init false je-test.api".to_owned()), "os dois mementos inicializam: {calls:?}");
        assert!(calls.contains(&r#"sync je-test.api@1.0.0 ["count"]"#.to_owned()), "setKeysForSync chega ao armazenamento: {calls:?}");
        assert_eq!(calls.iter().filter(|call| call.starts_with("set true")).count(), 1, "as atualizações da mesma rodada viram uma só gravação: {calls:?}");
    });
}

#[test]
fn memento_starts_from_the_stored_value_follows_the_main_thread_and_drops_undefined_keys() {
    let seeded = setup_with(&[(true, "je-test.api", r#"{"seed":"s","n":1}"#), (false, "je-test.api", r#"{"wsSeed":"w"}"#)]);
    run_extension(EXTENSION_SOURCE, seeded, |workbench, panel| {
        assert_eq!(run(workbench, "t.read"), r#"{"globalKeys":["seed","n"],"seed":"s","n":1,"workspaceKeys":["wsSeed"],"ws":"w"}"#);

        panel.ext_host_storage.borrow().clone().expect("o host ligou o proxy").rpc_accept_value(true, "je-test.api", r#"{"seed":"novo"}"#);
        assert_eq!(run(workbench, "t.read"), r#"{"globalKeys":["seed"],"seed":"novo","workspaceKeys":["wsSeed"],"ws":"w"}"#);

        run(workbench, "t.write");
        assert_eq!(run(workbench, "t.removeKey"), r#"{"keys":["seed","config"],"count":"undefined"}"#, "um valor undefined some de keys() e de get()");
    });
}

#[test]
fn context_exposes_the_extension_storage_and_log_locations() {
    run_extension(EXTENSION_SOURCE, setup_with(&[]), |workbench, _panel| {
        let uri_text = |path: &str| directory(path).to_string(false).to_string_lossy();
        let paths = run(workbench, "t.paths");
        let parsed: Vec<(&str, String)> = vec![
            ("logUri", uri_text("/je/logs/je-test.api")),
            ("globalStorageUri", uri_text("/je/global/je-test.api")),
            ("storageUri", uri_text("/je/workspace/ws1/je-test.api")),
        ];
        for (name, expected) in parsed {
            assert!(paths.contains(&format!(r#""{name}":"{expected}""#)), "{name} esperado {expected} em {paths}");
        }
        for fragment in [
            r#""extensionPathMatches":true"#,
            r#""absolute":true"#,
            r#""logPathEnds":true"#,
            r#""globalStoragePathEnds":true"#,
            r#""storagePathEnds":true"#,
            r#""extensionId":"je-test.api""#,
            r#""mode":1"#,
        ] {
            assert!(paths.contains(fragment), "faltou {fragment} em {paths}");
        }
    });
}

#[test]
fn secrets_forward_to_the_main_thread_and_relay_changes_to_the_owner_only() {
    run_extension(EXTENSION_SOURCE, setup_with(&[]), |workbench, panel| {
        assert_eq!(run(workbench, "t.secrets"), "started");
        settle(workbench);
        assert_eq!(workbench.probe("globalThis.__secret"), r#"{"got":"abc","missing":"undefined","keys":["token"]}"#);

        let secrets = panel.ext_host_secrets.borrow().clone().expect("o host ligou o proxy");
        drop(secrets.rpc_on_did_change_password("je-test.api", "token"));
        drop(secrets.rpc_on_did_change_password("outra.extensao", "token"));
        workbench.pump();
        assert_eq!(workbench.probe("JSON.stringify(globalThis.__events)"), r#"["secret:token"]"#, "só a mudança da própria extensão chega ao onDidChange");
    });
}

#[test]
fn environment_variable_collection_mutates_iterates_and_syncs_with_the_terminal_service() {
    run_extension(EXTENSION_SOURCE, setup_with(&[]), |workbench, panel| {
        assert_eq!(
            run(workbench, "t.env"),
            r#"{"persistent":true,"a":"a:1","bOptions":"true/undefined","seen":["JE_A:a:1","JE_B:b:2","JE_C:c:3"],"iterated":["JE_A/JE_A","JE_B/JE_B","JE_C/JE_C"],"removed":"undefined","description":"Variáveis do teste"}"#
        );
        settle(workbench);
        let syncs = panel.terminal.syncs.borrow().clone();
        let (extension, persistent, collection) = syncs.last().cloned().expect("a coleção sincronizou com o serviço de terminais");
        assert_eq!(extension, "je-test.api");
        assert!(persistent);
        assert_eq!(collection, Some(vec!["JE_A=a:1".to_owned(), "JE_B=b:2".to_owned()]), "o JE_C foi removido antes da última sincronização");

        assert_eq!(run(workbench, "t.envClear"), "false");
        settle(workbench);
        let (_, persistent, collection) = panel.terminal.syncs.borrow().last().cloned().expect("sincronizou de novo");
        assert!(!persistent);
        assert_eq!(collection, None, "a coleção vazia sincroniza como ausente");
    });
}
