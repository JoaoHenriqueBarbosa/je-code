//! Porte de `src/vs/workbench/api/test/browser/extHostConfiguration.test.ts` (suíte `ExtHostConfiguration`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` só vigia o `store` do caso `configuration change event`, que recebe
//!   a assinatura do evento: o caso a descarta no fim.
//! - `new TestRPCProtocol()` e os `mock<...>()` são os dublês de `support::ext_host_workspace_support` e o
//!   [`MockMainThreadConfiguration`]: um método sem resposta falha com `panic!`, como o método ausente do objeto
//!   vazio do original lançaria `TypeError`. `RecordingShape` é o mesmo dublê com `$updateConfigurationOption`
//!   gravando `[target, key, value]` ([`recording_shape`]).
//! - O `WorkspaceConfiguration` do original é um objeto: `config['chave']` é `property("chave")`
//!   ([`WorkspaceConfigurationProperty::Value`] com o valor), `config['get']` é
//!   `WorkspaceConfigurationProperty::Method("get")` e a escrita `config['get'] = x` é `set_property`, que sempre falha.
//!   `new ConfigurationModel(contents, keys, [], raw)` é o `ConfigurationModelData` (o dado do modelo que o
//!   `IConfigurationData` leva), e `toConfigurationModel(obj)` é o parser de `JSON.stringify(obj)`.
//! - `actual[...] = valor` num valor devolvido por `get` é a escrita direta no objeto ([`set`]): o objeto devolvido é
//!   uma cópia, e uma escrita num objeto congelado (a configuração somente leitura) lança, o que `errors::catch` captura.
//! - `assert.deepStrictEqual` é [`assert_deep_strict_equal`] e o `JSON.stringify` é [`json_stringify`].
//! - Os literais de objeto são JSON com aspas simples ([`j`]); `undefined` é `Dynamic::Undefined`.
//! - O caso `update/error-state not OK` confere o resultado da promessa já rejeitada (o proxy devolve uma promessa
//!   rejeitada, que `update` repassa), em vez de encadear `.then`.
//! - `configuration change event` (com `done`) confere por um sinalizador que o ouvinte rodou.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{self, Error};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::configuration::models::{ConfigurationModelParser, Raw};
use je_platform::configuration::{ConfigurationChange, ConfigurationData, ConfigurationModelData, ConfigurationOverrides, ConfigurationTarget};
use je_platform::log::NullLogService;
use je_platform::resources::uri_file;
use je_workbench::api::common::ext_host_configuration::{
    ConfigurationInspect, ConfigurationScopeArg, ConfigurationTargetArg, ExtHostConfigProvider, WorkspaceConfiguration, WorkspaceConfigurationProperty,
};
use je_workbench::api::common::ext_host_protocol::{ExtHostWorkspaceShape, IConfigurationInitData, IWorkspaceData, MainThreadConfigurationShape, WorkspaceFolderDataDto};
use je_workbench::api::common::ext_host_workspace::ExtHostWorkspace;
use support::ext_host_workspace_support::{TestMainContext, assert_deep_strict_equal, j, new_ext_host_workspace};

type UpdateFn = Box<dyn Fn(Option<ConfigurationTarget>, &str, Dynamic) -> Promise<()>>;

/// `new class extends mock<MainThreadConfigurationShape>() { }`, com o `$updateConfigurationOption` opcional.
#[derive(Default)]
struct MockMainThreadConfiguration {
    update_configuration_option: Option<UpdateFn>,
}

impl IDisposable for MockMainThreadConfiguration {
    fn dispose(&self) {}
}

impl MainThreadConfigurationShape for MockMainThreadConfiguration {
    fn rpc_update_configuration_option(
        &self,
        target: Option<ConfigurationTarget>,
        key: &str,
        value: Dynamic,
        _overrides: Option<ConfigurationOverrides>,
        _scope_to_language: Option<bool>,
    ) -> Promise<()> {
        match &self.update_configuration_option {
            Some(update) => update(target, key, value),
            None => panic!("$updateConfigurationOption is not a function"),
        }
    }

    fn rpc_remove_configuration_option(
        &self,
        _target: Option<ConfigurationTarget>,
        _key: &str,
        _overrides: Option<ConfigurationOverrides>,
        _scope_to_language: Option<bool>,
    ) -> Promise<()> {
        panic!("$removeConfigurationOption is not a function")
    }
}

type LastArgs = Rc<RefCell<Option<(Option<ConfigurationTarget>, String, Dynamic)>>>;

/// `RecordingShape`: guarda `[target, key, value]` do último `$updateConfigurationOption`.
fn recording_shape() -> (Rc<dyn MainThreadConfigurationShape>, LastArgs) {
    let last_args: LastArgs = Rc::default();
    let recorded = last_args.clone();
    let shape = MockMainThreadConfiguration {
        update_configuration_option: Some(Box::new(move |target, key, value| {
            *recorded.borrow_mut() = Some((target, key.to_owned(), value));
            Promise::resolved(())
        })),
    };
    (Rc::new(shape), last_args)
}

fn create_ext_host_workspace() -> Rc<ExtHostWorkspace> {
    new_ext_host_workspace(&TestMainContext::new(), None)
}

fn keys(names: &[&str]) -> Vec<JsString> {
    names.iter().map(|name| JsString::from(*name)).collect()
}

/// `new ConfigurationModel(contents, keys, [], raw, log)`.
fn model(contents: Dynamic, names: &[&str], raw: Option<Dynamic>) -> ConfigurationModelData {
    ConfigurationModelData { contents, keys: keys(names), overrides: Vec::new(), raw: raw.map(Raw::Dictionary) }
}

/// `ConfigurationModel.createEmptyModel(log)`.
fn empty_model() -> ConfigurationModelData {
    model(j("{}"), &[], None)
}

fn create_configuration_data(contents: Dynamic) -> IConfigurationInitData {
    IConfigurationInitData {
        data: ConfigurationData {
            defaults: model(contents.clone(), &[], None),
            policy: empty_model(),
            application: empty_model(),
            user_local: model(contents, &[], None),
            user_remote: empty_model(),
            workspace: empty_model(),
            folders: Vec::new(),
        },
        configuration_scopes: Vec::new(),
    }
}

fn create_ext_host_configuration(contents: Dynamic, shape: Option<Rc<dyn MainThreadConfigurationShape>>) -> Rc<ExtHostConfigProvider> {
    let shape = shape.unwrap_or_else(|| Rc::new(MockMainThreadConfiguration::default()));
    ExtHostConfigProvider::new(shape, create_ext_host_workspace(), create_configuration_data(contents), NullLogService::shared())
}

/// `new ExtHostConfigProvider(shape, workspace, data, log)` com os seis modelos dados.
#[allow(clippy::too_many_arguments)]
fn create_provider(
    ext_host_workspace: Rc<ExtHostWorkspace>,
    defaults: ConfigurationModelData,
    application: ConfigurationModelData,
    user_local: ConfigurationModelData,
    user_remote: ConfigurationModelData,
    workspace: ConfigurationModelData,
    folders: Vec<(URI, ConfigurationModelData)>,
) -> Rc<ExtHostConfigProvider> {
    ExtHostConfigProvider::new(
        Rc::new(MockMainThreadConfiguration::default()),
        ext_host_workspace,
        IConfigurationInitData {
            data: ConfigurationData { defaults, policy: empty_model(), application, user_local, user_remote, workspace, folders },
            configuration_scopes: Vec::new(),
        },
        NullLogService::shared(),
    )
}

/// `toConfigurationModel(obj)`: `parser.parse(JSON.stringify(obj))` e o dado do modelo.
fn to_configuration_model(json: &str) -> ConfigurationModelData {
    let mut parser = ConfigurationModelParser::new("test", NullLogService::shared());
    parser.parse(Some(&json.replace('\'', "\"")), None);
    parser.configuration_model().to_json()
}

/// `aWorkspaceFolder(uri, index)`: o `name` padrão é `''`.
fn a_workspace_folder(uri: &URI, index: usize) -> WorkspaceFolderDataDto {
    WorkspaceFolderDataDto { uri: uri.clone(), name: String::new(), index }
}

fn s(text: &str) -> Dynamic {
    Dynamic::string(text)
}

fn n(value: f64) -> Dynamic {
    Dynamic::Number(value)
}

fn b(value: bool) -> Dynamic {
    Dynamic::Bool(value)
}

fn undefined() -> Dynamic {
    Dynamic::Undefined
}

/// `config['nome']` de um valor que não é um dos membros.
fn property(config: &WorkspaceConfiguration, name: &str) -> Dynamic {
    match config.property(name) {
        WorkspaceConfigurationProperty::Value(value) => value,
        WorkspaceConfigurationProperty::Method(method) => panic!("`{name}` é o membro `{method}`"),
    }
}

/// `target[key] = value` num objeto (que pode estar congelado).
fn set(target: &Dynamic, key: &str, value: Dynamic) {
    match target {
        Dynamic::Object(object) => object.set(&JsString::from(key), value),
        other => panic!("não é um objeto: {other:?}"),
    }
}

/// `{ ...(actual || {}), ...extra }`.
fn spread(actual: &Dynamic, extra: Vec<(&str, Dynamic)>) -> Dynamic {
    let mut entries: Vec<(JsString, Dynamic)> = if matches!(actual, Dynamic::Object(_)) { actual.own_entries() } else { Vec::new() };
    entries.extend(extra.into_iter().map(|(key, value)| (JsString::from(key), value)));
    Dynamic::object(entries)
}

#[track_caller]
fn assert_stringify(expected: &str, actual: &Dynamic) {
    assert_eq!(json_stringify(&j(expected)).expect("o esperado é um objeto"), json_stringify(actual).expect("o valor tem JSON"));
}

/// As seis primeiras asserções de cada `inspect`: `defaultValue`, `globalLocalValue`, `globalRemoteValue`,
/// `globalValue`, `workspaceValue` e `workspaceFolderValue`.
#[track_caller]
fn assert_values(actual: &ConfigurationInspect, expected: [Dynamic; 6]) {
    let [default_value, global_local_value, global_remote_value, global_value, workspace_value, workspace_folder_value] = expected;
    assert_eq!(actual.default_value, default_value, "defaultValue");
    assert_eq!(actual.global_local_value, global_local_value, "globalLocalValue");
    assert_eq!(actual.global_remote_value, global_remote_value, "globalRemoteValue");
    assert_eq!(actual.global_value, global_value, "globalValue");
    assert_eq!(actual.workspace_value, workspace_value, "workspaceValue");
    assert_eq!(actual.workspace_folder_value, workspace_folder_value, "workspaceFolderValue");
}

#[test]
fn get_configuration_fails_regression_test_1_7_1_to_1_8_15552() {
    let ext_host_config = create_ext_host_configuration(j("{'search': {'exclude': {'**/node_modules': true}}}"), None);

    assert_eq!(property(&ext_host_config.get_configuration(Some("search.exclude"), None, None), "**/node_modules"), b(true));
    assert_eq!(ext_host_config.get_configuration(Some("search.exclude"), None, None).get("**/node_modules", None), b(true));
    assert_eq!(ext_host_config.get_configuration(Some("search"), None, None).get("exclude", None).get_str("**/node_modules"), b(true));

    assert!(ext_host_config.get_configuration(Some("search.exclude"), None, None).has("**/node_modules"));
    assert!(ext_host_config.get_configuration(Some("search"), None, None).has("exclude.**/node_modules"));
}

#[test]
fn has_get() {
    let all = create_ext_host_configuration(
        j("{'farboo': {'config0': true, 'nested': {'config1': 42, 'config2': 'Das Pferd frisst kein Reis.'}, 'config4': ''}}"),
        None,
    );

    let config = all.get_configuration(Some("farboo"), None, None);

    assert!(config.has("config0"));
    assert_eq!(config.get("config0", None), b(true));
    assert_eq!(config.get("config4", None), s(""));
    assert_eq!(property(&config, "config0"), b(true));
    assert_eq!(property(&config, "config4"), s(""));

    assert!(config.has("nested.config1"));
    assert_eq!(config.get("nested.config1", None), n(42.0));
    assert!(config.has("nested.config2"));
    assert_eq!(config.get("nested.config2", None), s("Das Pferd frisst kein Reis."));

    assert!(config.has("nested"));
    assert_deep_strict_equal(&config.get("nested", None), &j("{'config1': 42, 'config2': 'Das Pferd frisst kein Reis.'}"));
}

#[test]
fn get_nested_config() {
    let all = create_ext_host_configuration(
        j("{'farboo': {'config0': true, 'nested': {'config1': 42, 'config2': 'Das Pferd frisst kein Reis.'}, 'config4': ''}}"),
        None,
    );

    assert_deep_strict_equal(&all.get_configuration(Some("farboo.nested"), None, None).get("config1", None), &n(42.0));
    assert_deep_strict_equal(&all.get_configuration(Some("farboo.nested"), None, None).get("config2", None), &s("Das Pferd frisst kein Reis."));
    assert_deep_strict_equal(&property(&all.get_configuration(Some("farboo.nested"), None, None), "config1"), &n(42.0));
    assert_deep_strict_equal(&property(&all.get_configuration(Some("farboo.nested"), None, None), "config2"), &s("Das Pferd frisst kein Reis."));
    assert_deep_strict_equal(&all.get_configuration(Some("farboo.nested1"), None, None).get("config1", None), &undefined());
    assert_deep_strict_equal(&all.get_configuration(Some("farboo.nested1"), None, None).get("config2", None), &undefined());
    assert_deep_strict_equal(&all.get_configuration(Some("farboo.config0.config1"), None, None).get("a", None), &undefined());
    assert_deep_strict_equal(&property(&all.get_configuration(Some("farboo.config0.config1"), None, None), "a"), &undefined());
}

#[test]
fn can_modify_the_returned_configuration() {
    let all = create_ext_host_configuration(
        j("{'farboo': {'config0': true, 'nested': {'config1': 42, 'config2': 'Das Pferd frisst kein Reis.'}, 'config4': ''}, 'workbench': {'colorCustomizations': {'statusBar.foreground': 'somevalue'}}}"),
        None,
    );

    let mut test_object = all.get_configuration(None, None, None);
    let mut actual = test_object.get("farboo", None);
    set(&actual.get_str("nested"), "config1", n(41.0));
    assert_eq!(n(41.0), actual.get_str("nested").get_str("config1"));
    set(&actual, "farboo1", s("newValue"));
    assert_eq!(s("newValue"), actual.get_str("farboo1"));

    test_object = all.get_configuration(None, None, None);
    actual = test_object.get("farboo", None);
    assert_eq!(actual.get_str("nested").get_str("config1"), n(42.0));
    assert_eq!(actual.get_str("farboo1"), undefined());

    test_object = all.get_configuration(None, None, None);
    actual = test_object.get("farboo", None);
    assert_eq!(actual.get_str("config0"), b(true));
    set(&actual, "config0", b(false));
    assert_eq!(actual.get_str("config0"), b(false));

    test_object = all.get_configuration(None, None, None);
    actual = test_object.get("farboo", None);
    assert_eq!(actual.get_str("config0"), b(true));

    test_object = all.get_configuration(None, None, None);
    actual = test_object.inspect("farboo").expect("inspect devolve o objeto").to_dynamic();
    set(&actual, "value", s("effectiveValue"));
    assert_eq!(s("effectiveValue"), actual.get_str("value"));

    test_object = all.get_configuration(Some("workbench"), None, None);
    actual = test_object.get("colorCustomizations", None);
    set(&actual, "statusBar.foreground", undefined());
    assert_eq!(actual.get_str("statusBar.foreground"), undefined());
    test_object = all.get_configuration(Some("workbench"), None, None);
    actual = test_object.get("colorCustomizations", None);
    assert_eq!(actual.get_str("statusBar.foreground"), s("somevalue"));
}

#[test]
fn stringify_returned_configuration() {
    let all = create_ext_host_configuration(
        j("{'farboo': {'config0': true, 'nested': {'config1': 42, 'config2': 'Das Pferd frisst kein Reis.'}, 'config4': ''}, 'workbench': {'colorCustomizations': {'statusBar.foreground': 'somevalue'}, 'emptyobjectkey': {}}}"),
        None,
    );

    let test_object = all.get_configuration(None, None, None);
    let mut actual = test_object.get("farboo", None);
    assert_stringify("{'config0': true, 'nested': {'config1': 42, 'config2': 'Das Pferd frisst kein Reis.'}, 'config4': ''}", &actual);

    // `JSON.stringify(testObject.get('unknownkey'))` é `undefined`
    assert_eq!(None, json_stringify(&test_object.get("unknownkey", None)));

    actual = test_object.get("farboo", None);
    set(&actual, "config0", b(false));
    assert_stringify("{'config0': false, 'nested': {'config1': 42, 'config2': 'Das Pferd frisst kein Reis.'}, 'config4': ''}", &actual);

    actual = test_object.get("workbench", None).get_str("colorCustomizations");
    set(&actual, "statusBar.background", s("anothervalue"));
    assert_stringify("{'statusBar.foreground': 'somevalue', 'statusBar.background': 'anothervalue'}", &actual);

    actual = test_object.get("workbench", None);
    set(&actual, "unknownkey", s("somevalue"));
    assert_stringify(
        "{'colorCustomizations': {'statusBar.foreground': 'somevalue'}, 'emptyobjectkey': {}, 'unknownkey': 'somevalue'}",
        &actual,
    );

    actual = all.get_configuration(Some("workbench"), None, None).get("emptyobjectkey", None);
    actual = spread(&actual, vec![("statusBar.background", s("#0ff")), ("statusBar.foreground", s("#ff0"))]);
    assert_stringify("{'statusBar.background': '#0ff', 'statusBar.foreground': '#ff0'}", &actual);

    actual = all.get_configuration(Some("workbench"), None, None).get("unknownkey", None);
    actual = spread(&actual, vec![("statusBar.background", s("#0ff")), ("statusBar.foreground", s("#ff0"))]);
    assert_stringify("{'statusBar.background': '#0ff', 'statusBar.foreground': '#ff0'}", &actual);
}

#[test]
fn cannot_modify_returned_configuration() {
    let all = create_ext_host_configuration(
        j("{'farboo': {'config0': true, 'nested': {'config1': 42, 'config2': 'Das Pferd frisst kein Reis.'}, 'config4': ''}}"),
        None,
    );

    let test_object = all.get_configuration(None, None, None);

    // `testObject['get'] = null`
    assert!(test_object.set_property("get", Dynamic::Null).is_err(), "This should be readonly");

    // `testObject['farboo']['config0'] = false`
    let farboo = property(&test_object, "farboo");
    assert!(errors::catch(|| set(&farboo, "config0", b(false))).is_err(), "This should be readonly");

    // `testObject['farboo']['farboo1'] = 'hello'`
    assert!(errors::catch(|| set(&farboo, "farboo1", s("hello"))).is_err(), "This should be readonly");
}

#[test]
fn inspect_in_no_workspace_context() {
    let test_object = create_provider(
        create_ext_host_workspace(),
        model(j("{'editor': {'wordWrap': 'off', 'lineNumbers': 'on', 'fontSize': '12px'}}"), &["editor.wordWrap"], None),
        empty_model(),
        model(j("{'editor': {'wordWrap': 'on', 'lineNumbers': 'off'}}"), &["editor.wordWrap", "editor.lineNumbers"], None),
        model(
            j("{'editor': {'lineNumbers': 'relative'}}"),
            &["editor.lineNumbers"],
            Some(j("{'editor': {'lineNumbers': 'relative', 'fontSize': '14px'}}")),
        ),
        model(j("{}"), &[], None),
        Vec::new(),
    );

    let mut actual = test_object.get_configuration(None, None, None).inspect("editor.wordWrap").unwrap();
    assert_values(&actual, [s("off"), s("on"), undefined(), s("on"), undefined(), undefined()]);

    actual = test_object.get_configuration(Some("editor"), None, None).inspect("wordWrap").unwrap();
    assert_values(&actual, [s("off"), s("on"), undefined(), s("on"), undefined(), undefined()]);

    actual = test_object.get_configuration(Some("editor"), None, None).inspect("lineNumbers").unwrap();
    assert_values(&actual, [s("on"), s("off"), s("relative"), s("relative"), undefined(), undefined()]);

    assert_eq!(test_object.get_configuration(Some("editor"), None, None).get("fontSize", None), s("12px"));

    actual = test_object.get_configuration(Some("editor"), None, None).inspect("fontSize").unwrap();
    assert_values(&actual, [s("12px"), undefined(), s("14px"), undefined(), undefined(), undefined()]);
}

#[test]
fn inspect_in_single_root_context() {
    let workspace_uri = uri_file("foo");
    let workspace = model(j("{'editor': {'wordWrap': 'bounded'}}"), &["editor.wordWrap"], None);
    let folders = vec![(workspace_uri.clone(), workspace.clone())];
    let ext_host_workspace = create_ext_host_workspace();
    ext_host_workspace.rpc_initialize_workspace(
        Some(IWorkspaceData { id: "foo".to_owned(), name: "foo".to_owned(), folders: vec![a_workspace_folder(&uri_file("foo"), 0)], ..IWorkspaceData::default() }),
        true,
    );
    let test_object = create_provider(
        ext_host_workspace,
        model(j("{'editor': {'wordWrap': 'off'}}"), &["editor.wordWrap"], None),
        empty_model(),
        model(j("{'editor': {'wordWrap': 'on'}}"), &["editor.wordWrap"], None),
        empty_model(),
        workspace,
        folders,
    );

    let mut actual1 = test_object.get_configuration(None, None, None).inspect("editor.wordWrap").unwrap();
    assert_values(&actual1, [s("off"), s("on"), undefined(), s("on"), s("bounded"), undefined()]);

    actual1 = test_object.get_configuration(Some("editor"), None, None).inspect("wordWrap").unwrap();
    assert_values(&actual1, [s("off"), s("on"), undefined(), s("on"), s("bounded"), undefined()]);

    let scope = ConfigurationScopeArg::Uri(workspace_uri);
    let mut actual2 = test_object.get_configuration(None, Some(&scope), None).inspect("editor.wordWrap").unwrap();
    assert_values(&actual2, [s("off"), s("on"), undefined(), s("on"), s("bounded"), s("bounded")]);

    actual2 = test_object.get_configuration(Some("editor"), Some(&scope), None).inspect("wordWrap").unwrap();
    assert_values(&actual2, [s("off"), s("on"), undefined(), s("on"), s("bounded"), s("bounded")]);
}

#[test]
fn inspect_in_multi_root_context() {
    let workspace = model(j("{'editor': {'wordWrap': 'bounded'}}"), &["editor.wordWrap"], None);

    let first_root = uri_file("foo1");
    let second_root = uri_file("foo2");
    let third_root = uri_file("foo3");
    let folders = vec![
        (first_root.clone(), model(j("{'editor': {'wordWrap': 'off', 'lineNumbers': 'relative'}}"), &["editor.wordWrap"], None)),
        (second_root.clone(), model(j("{'editor': {'wordWrap': 'on'}}"), &["editor.wordWrap"], None)),
        (third_root.clone(), model(j("{}"), &[], None)),
    ];

    let ext_host_workspace = create_ext_host_workspace();
    ext_host_workspace.rpc_initialize_workspace(
        Some(IWorkspaceData {
            id: "foo".to_owned(),
            name: "foo".to_owned(),
            folders: vec![a_workspace_folder(&first_root, 0), a_workspace_folder(&second_root, 1)],
            ..IWorkspaceData::default()
        }),
        true,
    );
    let test_object = create_provider(
        ext_host_workspace,
        model(j("{'editor': {'wordWrap': 'off', 'lineNumbers': 'on'}}"), &["editor.wordWrap"], None),
        empty_model(),
        model(j("{'editor': {'wordWrap': 'on'}}"), &["editor.wordWrap"], None),
        empty_model(),
        workspace,
        folders,
    );

    let mut actual1 = test_object.get_configuration(None, None, None).inspect("editor.wordWrap").unwrap();
    assert_values(&actual1, [s("off"), s("on"), undefined(), s("on"), s("bounded"), undefined()]);

    actual1 = test_object.get_configuration(Some("editor"), None, None).inspect("wordWrap").unwrap();
    assert_values(&actual1, [s("off"), s("on"), undefined(), s("on"), s("bounded"), undefined()]);

    actual1 = test_object.get_configuration(Some("editor"), None, None).inspect("lineNumbers").unwrap();
    assert_values(&actual1, [s("on"), undefined(), undefined(), undefined(), undefined(), undefined()]);

    let first = ConfigurationScopeArg::Uri(first_root);
    let second = ConfigurationScopeArg::Uri(second_root);
    let third = ConfigurationScopeArg::Uri(third_root);

    let mut actual2 = test_object.get_configuration(None, Some(&first), None).inspect("editor.wordWrap").unwrap();
    assert_values(&actual2, [s("off"), s("on"), undefined(), s("on"), s("bounded"), s("off")]);

    actual2 = test_object.get_configuration(Some("editor"), Some(&first), None).inspect("wordWrap").unwrap();
    assert_values(&actual2, [s("off"), s("on"), undefined(), s("on"), s("bounded"), s("off")]);

    actual2 = test_object.get_configuration(Some("editor"), Some(&first), None).inspect("lineNumbers").unwrap();
    assert_values(&actual2, [s("on"), undefined(), undefined(), undefined(), undefined(), s("relative")]);

    actual2 = test_object.get_configuration(None, Some(&second), None).inspect("editor.wordWrap").unwrap();
    assert_values(&actual2, [s("off"), s("on"), undefined(), s("on"), s("bounded"), s("on")]);

    actual2 = test_object.get_configuration(Some("editor"), Some(&second), None).inspect("wordWrap").unwrap();
    assert_values(&actual2, [s("off"), s("on"), undefined(), s("on"), s("bounded"), s("on")]);

    actual2 = test_object.get_configuration(None, Some(&third), None).inspect("editor.wordWrap").unwrap();
    assert_values(&actual2, [s("off"), s("on"), undefined(), s("on"), s("bounded"), undefined()]);
    assert!(actual2.to_dynamic().own_keys().contains(&JsString::from("workspaceFolderValue")));

    actual2 = test_object.get_configuration(Some("editor"), Some(&third), None).inspect("wordWrap").unwrap();
    assert_values(&actual2, [s("off"), s("on"), undefined(), s("on"), s("bounded"), undefined()]);
    assert!(actual2.to_dynamic().own_keys().contains(&JsString::from("workspaceFolderValue")));
}

#[test]
fn inspect_with_language_overrides() {
    let first_root = uri_file("foo1");
    let second_root = uri_file("foo2");
    let folders = vec![
        (first_root.clone(), to_configuration_model("{'editor.wordWrap': 'bounded', '[typescript]': {'editor.wordWrap': 'unbounded'}}")),
        (second_root.clone(), to_configuration_model("{}")),
    ];

    let ext_host_workspace = create_ext_host_workspace();
    ext_host_workspace.rpc_initialize_workspace(
        Some(IWorkspaceData {
            id: "foo".to_owned(),
            name: "foo".to_owned(),
            folders: vec![a_workspace_folder(&first_root, 0), a_workspace_folder(&second_root, 1)],
            ..IWorkspaceData::default()
        }),
        true,
    );
    let test_object = create_provider(
        ext_host_workspace,
        to_configuration_model("{'editor.wordWrap': 'off', '[markdown]': {'editor.wordWrap': 'bounded'}}"),
        empty_model(),
        to_configuration_model("{'editor.wordWrap': 'bounded', '[typescript]': {'editor.lineNumbers': 'off'}}"),
        empty_model(),
        to_configuration_model("{'[typescript]': {'editor.wordWrap': 'unbounded', 'editor.lineNumbers': 'off'}}"),
        folders,
    );

    let scope = ConfigurationScopeArg::ResourceLanguage { uri: first_root, language_id: "typescript".to_owned() };
    let mut actual = test_object.get_configuration(None, Some(&scope), None).inspect("editor.wordWrap").unwrap();
    assert_eq!(actual.default_value, s("off"));
    assert_eq!(actual.global_value, s("bounded"));
    assert_eq!(actual.global_local_value, s("bounded"));
    assert_eq!(actual.global_remote_value, undefined());
    assert_eq!(actual.workspace_value, undefined());
    assert_eq!(actual.workspace_folder_value, s("bounded"));
    assert_eq!(actual.default_language_value, undefined());
    assert_eq!(actual.global_language_value, undefined());
    assert_eq!(actual.workspace_language_value, s("unbounded"));
    assert_eq!(actual.workspace_folder_language_value, s("unbounded"));
    assert_eq!(actual.language_ids, Some(vec!["markdown".to_owned(), "typescript".to_owned()]));

    let scope = ConfigurationScopeArg::ResourceLanguage { uri: second_root, language_id: "typescript".to_owned() };
    actual = test_object.get_configuration(None, Some(&scope), None).inspect("editor.wordWrap").unwrap();
    assert_eq!(actual.default_value, s("off"));
    assert_eq!(actual.global_value, s("bounded"));
    assert_eq!(actual.global_local_value, s("bounded"));
    assert_eq!(actual.global_remote_value, undefined());
    assert_eq!(actual.workspace_value, undefined());
    assert_eq!(actual.workspace_folder_value, undefined());
    assert_eq!(actual.default_language_value, undefined());
    assert_eq!(actual.global_language_value, undefined());
    assert_eq!(actual.workspace_language_value, s("unbounded"));
    assert_eq!(actual.workspace_folder_language_value, undefined());
    assert_eq!(actual.language_ids, Some(vec!["markdown".to_owned(), "typescript".to_owned()]));
}

#[test]
fn application_is_not_set_in_inspect() {
    let test_object = create_provider(
        create_ext_host_workspace(),
        model(j("{'editor': {'wordWrap': 'off', 'lineNumbers': 'on', 'fontSize': '12px'}}"), &["editor.wordWrap"], None),
        model(j("{'editor': {'wordWrap': 'on'}}"), &["editor.wordWrap"], None),
        model(j("{'editor': {'wordWrap': 'auto', 'lineNumbers': 'off'}}"), &["editor.wordWrap"], None),
        empty_model(),
        model(j("{}"), &[], None),
        Vec::new(),
    );

    let mut actual = test_object.get_configuration(None, None, None).inspect("editor.wordWrap").unwrap();
    assert_values(&actual, [s("off"), s("auto"), undefined(), s("auto"), undefined(), undefined()]);
    assert_eq!(test_object.get_configuration(None, None, None).get("editor.wordWrap", None), s("auto"));

    actual = test_object.get_configuration(None, None, None).inspect("editor.lineNumbers").unwrap();
    assert_values(&actual, [s("on"), s("off"), undefined(), s("off"), undefined(), undefined()]);
    assert_eq!(test_object.get_configuration(None, None, None).get("editor.lineNumbers", None), s("off"));

    actual = test_object.get_configuration(None, None, None).inspect("editor.fontSize").unwrap();
    assert_values(&actual, [s("12px"), undefined(), undefined(), undefined(), undefined(), undefined()]);
    assert_eq!(test_object.get_configuration(None, None, None).get("editor.fontSize", None), s("12px"));
}

#[test]
fn get_configuration_vs_get() {
    let all = create_ext_host_configuration(j("{'farboo': {'config0': true, 'config4': 38}}"), None);

    let mut config = all.get_configuration(Some("farboo.config0"), None, None);
    assert_eq!(config.get("", None), undefined());
    assert!(!config.has(""));

    config = all.get_configuration(Some("farboo"), None, None);
    assert_eq!(config.get("config0", None), b(true));
    assert!(config.has("config0"));
}

#[test]
fn name_vs_property() {
    let all = create_ext_host_configuration(j("{'farboo': {'get': 'get-prop'}}"), None);
    let config = all.get_configuration(Some("farboo"), None, None);

    assert!(config.has("get"));
    assert_eq!(config.get("get", None), s("get-prop"));
    // `assert.deepStrictEqual(config['get'], config.get)`: o membro vence a propriedade
    assert!(matches!(config.property("get"), WorkspaceConfigurationProperty::Method("get")));
    assert!(config.set_property("get", s("get-prop")).is_err());
}

#[test]
fn update_no_target_passes_null() {
    let (shape, last_args) = recording_shape();
    let all_config = create_ext_host_configuration(j("{'foo': {'bar': 1, 'far': 1}}"), Some(shape));

    let config = all_config.get_configuration(Some("foo"), None, None);
    drop(config.update("bar", n(42.0), None, None));

    assert_eq!(last_args.borrow().as_ref().unwrap().0, None);
}

#[test]
fn update_section_to_key() {
    let (shape, last_args) = recording_shape();
    let all_config = create_ext_host_configuration(j("{'foo': {'bar': 1, 'far': 1}}"), Some(shape));

    let mut config = all_config.get_configuration(Some("foo"), None, None);
    drop(config.update("bar", n(42.0), Some(ConfigurationTargetArg::Bool(true)), None));

    {
        let last = last_args.borrow();
        let last = last.as_ref().unwrap();
        assert_eq!(last.0, Some(ConfigurationTarget::User));
        assert_eq!(last.1, "foo.bar");
        assert_eq!(last.2, n(42.0));
    }

    config = all_config.get_configuration(Some(""), None, None);
    drop(config.update("bar", n(42.0), Some(ConfigurationTargetArg::Bool(true)), None));
    assert_eq!(last_args.borrow().as_ref().unwrap().1, "bar");

    drop(config.update("foo.bar", n(42.0), Some(ConfigurationTargetArg::Bool(true)), None));
    assert_eq!(last_args.borrow().as_ref().unwrap().1, "foo.bar");
}

#[test]
fn update_what_is_15834() {
    let (shape, last_args) = recording_shape();
    let all_config = create_ext_host_configuration(j("{'editor': {'formatOnSave': true}}"), Some(shape));

    drop(all_config.get_configuration(Some("editor"), None, None).update("formatOnSave", j("{'extensions': ['ts']}"), None, None));
    let last = last_args.borrow();
    let last = last.as_ref().unwrap();
    assert_eq!(last.1, "editor.formatOnSave");
    assert_deep_strict_equal(&last.2, &j("{'extensions': ['ts']}"));
}

#[test]
fn update_error_state_not_ok() {
    let shape = MockMainThreadConfiguration {
        update_configuration_option: Some(Box::new(|_target, _key, _value| Promise::rejected(Error::new("Unknown Key")))), // something !== OK
    };

    let result = create_ext_host_configuration(j("{}"), Some(Rc::new(shape))).get_configuration(Some(""), None, None).update(
        "",
        Dynamic::Bool(true),
        Some(ConfigurationTargetArg::Bool(false)),
        None,
    );
    // expecting rejection
    assert!(matches!(result.result(), Some(Err(_))));
}

#[test]
fn configuration_change_event() {
    let workspace_folder = a_workspace_folder(&uri_file("folder1"), 0);
    let ext_host_workspace = create_ext_host_workspace();
    ext_host_workspace.rpc_initialize_workspace(
        Some(IWorkspaceData { id: "foo".to_owned(), name: "foo".to_owned(), folders: vec![workspace_folder.clone()], ..IWorkspaceData::default() }),
        true,
    );
    let test_object = ExtHostConfigProvider::new(
        Rc::new(MockMainThreadConfiguration::default()),
        ext_host_workspace,
        create_configuration_data(j("{'farboo': {'config': false, 'updatedConfig': false}}")),
        NullLogService::shared(),
    );

    let new_config_data = create_configuration_data(j("{'farboo': {'config': false, 'updatedConfig': true, 'newConfig': true}}"));
    let config_event_data = ConfigurationChange { keys: keys(&["farboo.updatedConfig", "farboo.newConfig"]), overrides: Vec::new() };
    let done = Rc::new(Cell::new(false));
    let (observed, flag) = (Rc::downgrade(&test_object), done.clone());
    let any = ConfigurationScopeArg::Uri(uri_file("any"));
    let folder = ConfigurationScopeArg::Uri(workspace_folder.uri.clone());
    let store = test_object.on_did_change_configuration().listen(move |e| {
        let test_object = observed.upgrade().expect("o provedor está vivo");
        assert_deep_strict_equal(
            &test_object.get_configuration(None, None, None).get("farboo", None),
            &j("{'config': false, 'updatedConfig': true, 'newConfig': true}"),
        );

        assert!(e.affects_configuration("farboo", None));
        assert!(e.affects_configuration("farboo", Some(&folder)));
        assert!(e.affects_configuration("farboo", Some(&any)));

        assert!(e.affects_configuration("farboo.updatedConfig", None));
        assert!(e.affects_configuration("farboo.updatedConfig", Some(&folder)));
        assert!(e.affects_configuration("farboo.updatedConfig", Some(&any)));

        assert!(e.affects_configuration("farboo.newConfig", None));
        assert!(e.affects_configuration("farboo.newConfig", Some(&folder)));
        assert!(e.affects_configuration("farboo.newConfig", Some(&any)));

        assert!(!e.affects_configuration("farboo.config", None));
        assert!(!e.affects_configuration("farboo.config", Some(&folder)));
        assert!(!e.affects_configuration("farboo.config", Some(&any)));
        flag.set(true);
    });

    test_object.accept_configuration_changed(new_config_data, config_event_data);
    assert!(done.get(), "o ouvinte não rodou");
    store.dispose();
}

#[test]
fn get_return_instance_of_array_value() {
    let test_object = create_ext_host_configuration(j("{'far': {'boo': []}}"), None);

    let value = test_object.get_configuration(None, None, None).get("far.boo", Some(Dynamic::array([])));
    match &value {
        Dynamic::Array(array) => array.push(s("a")),
        other => panic!("não é um array: {other:?}"),
    }

    let actual = test_object.get_configuration(None, None, None).get("far.boo", Some(Dynamic::array([])));
    assert_deep_strict_equal(&actual, &j("[]"));
}
