//! `ExtensionContext.environmentVariableCollection`: a `vscode.GlobalEnvironmentVariableCollection` de cada extensão
//! (`extHostTerminalService.ts`: `UnifiedEnvironmentVariableCollection` e `ScopedEnvironmentVariableCollection`), sobre o
//! `ExtHostTerminalService` portado.
//!
//! Divergências:
//! - A coleção é uma instância de classe nativa que carrega só o identificador da extensão (`_extension`) e o escopo (`_scope`, do
//!   `getScoped`); cada chamada lê a coleção do serviço de terminais (`getEnvironmentVariableCollection`), que é quem guarda o estado.
//! - O `variable` do mutator e o `[Symbol.iterator]` (os pares `[variable, mutator]`) seguem o original; o iterador é instalado no
//!   protótipo por uma função auxiliar sobre `_entries()`.
//! - `forEach(callback, thisArg)` chama `callback.call(thisArg, variable, mutator, collection)`.

use std::rc::Rc;

use je_platform::terminal::environment_variable::{EnvironmentVariableScope, IEnvironmentVariableMutatorOptions};
use je_platform::workspace::WorkspaceFolder;
use je_workbench::services::extensions::extension_description_registry::IReadOnlyExtensionDescriptionRegistry;
use je_workbench::api::common::ext_host_terminal_service::{EnvironmentVariableMutator, ScopedEnvironmentVariableCollection, WorkerExtHostTerminalService};
use zjsc::api::embed_node::{ClassSpec, JsData, JsHandle, MethodCallback, NodeRunner, CLASS_KEY};

use crate::api::disposable::DisposableTable;
use crate::api::events::event_function;
use crate::api::extensions::ExtensionServiceSlot;
use crate::api::language_types::documentation_from_js;
use crate::convert::{field, opt_bool, opt_number, opt_string, uri_from_js, uri_to_js};

const CLASS_NAME: &str = "EnvironmentVariableCollection";

/// O objeto `vscode.GlobalEnvironmentVariableCollection` da extensão `extension_id` (o escopo é o global).
pub fn instance(extension_id: &str) -> JsData {
    JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str(CLASS_NAME.to_owned())), ("_extension".to_owned(), JsData::Str(extension_id.to_owned()))])
}

/// O escopo (`{ workspaceFolder?: { uri, name, index } }`) como dado do motor.
fn scope_to_js(scope: &EnvironmentVariableScope) -> JsData {
    let folder = scope.workspace_folder.as_ref().map_or(JsData::Undefined, |folder| {
        JsData::Object(vec![
            ("uri".to_owned(), uri_to_js(&folder.uri)),
            ("name".to_owned(), JsData::Str(folder.name.clone())),
            ("index".to_owned(), JsData::Num(folder.index as f64)),
        ])
    });
    JsData::Object(vec![("workspaceFolder".to_owned(), folder)])
}

/// O escopo que o motor entregou (`getScoped(scope)`).
fn scope_from_js(value: &JsData) -> Result<EnvironmentVariableScope, String> {
    let Some(folder) = field(value, "workspaceFolder").filter(|folder| matches!(folder, JsData::Object(_))) else {
        return Ok(EnvironmentVariableScope { workspace_folder: None });
    };
    let uri = uri_from_js(field(folder, "uri").ok_or("getScoped: workspaceFolder.uri ausente")?)?;
    let name = opt_string(field(folder, "name")).unwrap_or_default();
    let index = opt_number(field(folder, "index")).unwrap_or(0.0) as usize;
    Ok(EnvironmentVariableScope { workspace_folder: Some(WorkspaceFolder::new(uri, name, index, None)) })
}

fn options_from_js(value: Option<&JsData>) -> Option<IEnvironmentVariableMutatorOptions> {
    let options = value.filter(|options| matches!(options, JsData::Object(_)))?;
    Some(IEnvironmentVariableMutatorOptions {
        apply_at_process_creation: opt_bool(field(options, "applyAtProcessCreation")),
        apply_at_shell_integration: opt_bool(field(options, "applyAtShellIntegration")),
    })
}

/// `convertMutator`: `{ variable, value, type, options }`.
fn mutator_to_js(mutator: &EnvironmentVariableMutator) -> JsData {
    let mut fields = vec![
        ("variable".to_owned(), JsData::Str(mutator.variable.clone())),
        ("value".to_owned(), JsData::Str(mutator.value.clone())),
        ("type".to_owned(), JsData::Num(f64::from(mutator.mutator_type as i32))),
    ];
    if let Some(options) = mutator.options {
        let flag = |value: Option<bool>| value.map_or(JsData::Undefined, JsData::Bool);
        fields.push((
            "options".to_owned(),
            JsData::Object(vec![
                ("applyAtProcessCreation".to_owned(), flag(options.apply_at_process_creation)),
                ("applyAtShellIntegration".to_owned(), flag(options.apply_at_shell_integration)),
            ]),
        ));
    }
    JsData::Object(fields)
}

/// O corpo de um método ou acessor como `MethodCallback` (a assinatura fixa os tipos dos parâmetros do fechamento).
fn method(body: impl Fn(&JsData, &[JsData]) -> Result<JsData, String> + 'static) -> MethodCallback {
    Rc::new(body)
}

/// O que cada método precisa para achar a coleção.
struct Lookup {
    terminal: Rc<WorkerExtHostTerminalService>,
    slot: Rc<ExtensionServiceSlot>,
}

impl Lookup {
    fn collection(&self, this: &JsData) -> Result<Rc<ScopedEnvironmentVariableCollection>, String> {
        let Some(JsData::Str(id)) = field(this, "_extension") else { return Err("EnvironmentVariableCollection: this inválido".to_owned()) };
        let service = self.slot.service()?;
        let description = service.my_registry().get_extension_description(id).ok_or_else(|| format!("a extensão '{id}' não está mais registrada"))?;
        let global = self.terminal.get_environment_variable_collection(&description);
        match field(this, "_scope").filter(|scope| matches!(scope, JsData::Object(_))) {
            Some(scope) => Ok(global.get_scoped(Some(&scope_from_js(scope)?))),
            None => Ok(global),
        }
    }
}

/// Define a classe `EnvironmentVariableCollection` (não exportada em `vscode`: é uma interface no `vscode.d.ts`; o protótipo existe
/// para os objetos que o contexto entrega).
pub fn define_class(runner: &NodeRunner, table: &Rc<DisposableTable>, terminal: Rc<WorkerExtHostTerminalService>, slot: Rc<ExtensionServiceSlot>) {
    let lookup = Rc::new(Lookup { terminal, slot });

    let mutation = |name: &'static str, apply: fn(&ScopedEnvironmentVariableCollection, &str, &str, Option<IEnvironmentVariableMutatorOptions>) -> Result<(), je_base::errors::Error>| {
        let lookup = lookup.clone();
        let callback: MethodCallback = method(move |this, args| {
            let (Some(JsData::Str(variable)), Some(JsData::Str(value))) = (args.first(), args.get(1)) else {
                return Err(format!("{name}: a variável e o valor precisam ser texto"));
            };
            apply(&*lookup.collection(this)?, variable, value, options_from_js(args.get(2))).map_err(|error| error.message)?;
            Ok(JsData::Undefined)
        });
        (name.to_owned(), 3u32, callback)
    };

    let get_lookup = lookup.clone();
    let for_each_lookup = lookup.clone();
    let for_each_runner = runner.clone();
    let delete_lookup = lookup.clone();
    let clear_lookup = lookup.clone();
    let scoped_lookup = lookup.clone();
    let entries_lookup = lookup.clone();
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        mutation("replace", |collection, variable, value, options| collection.replace(variable, value, options)),
        mutation("append", |collection, variable, value, options| collection.append(variable, value, options)),
        mutation("prepend", |collection, variable, value, options| collection.prepend(variable, value, options)),
        (
            "get".to_owned(),
            1,
            method(move |this,args| {
                let Some(JsData::Str(variable)) = args.first() else { return Err("get: a variável precisa ser texto".to_owned()) };
                Ok(get_lookup.collection(this)?.get(variable).map_or(JsData::Undefined, |mutator| mutator_to_js(&mutator)))
            }),
        ),
        (
            "forEach".to_owned(),
            2,
            method(move |this,args| {
                let Some(JsData::Handle(callback)) = args.first().cloned() else { return Err("forEach: o callback precisa ser função".to_owned()) };
                let this_arg = args.get(1).cloned().unwrap_or(JsData::Undefined);
                let collection = for_each_lookup.collection(this)?;
                for (variable, mutator) in collection.entries() {
                    let call_args = [this_arg.clone(), JsData::Str(variable), mutator_to_js(&mutator), this.clone()];
                    for_each_runner.call_method(&callback, "call", &call_args)?;
                }
                Ok(JsData::Undefined)
            }),
        ),
        (
            "delete".to_owned(),
            1,
            method(move |this,args| {
                let Some(JsData::Str(variable)) = args.first() else { return Err("delete: a variável precisa ser texto".to_owned()) };
                delete_lookup.collection(this)?.delete(variable);
                Ok(JsData::Undefined)
            }),
        ),
        (
            "clear".to_owned(),
            0,
            method(move |this,_| {
                clear_lookup.collection(this)?.clear();
                Ok(JsData::Undefined)
            }),
        ),
        (
            "getScoped".to_owned(),
            1,
            method(move |this,args| {
                let scope = scope_from_js(args.first().ok_or("getScoped: o escopo é obrigatório")?)?;
                // Valida a extensão e cria o escopo no serviço (o `getScoped` do original o registra).
                scoped_lookup.collection(this)?.get_scoped(Some(&scope));
                let JsData::Object(mut fields) = this.clone() else { return Err("EnvironmentVariableCollection: this inválido".to_owned()) };
                fields.retain(|(key, _)| key != "_scope");
                fields.push(("_scope".to_owned(), scope_to_js(&scope)));
                Ok(JsData::Object(fields))
            }),
        ),
        (
            "_entries".to_owned(),
            0,
            method(move |this,_| {
                let entries = entries_lookup.collection(this)?.entries();
                Ok(JsData::Array(entries.iter().map(|(variable, mutator)| JsData::Array(vec![JsData::Str(variable.clone()), mutator_to_js(mutator)])).collect()))
            }),
        ),
    ];

    let (event_runner, event_table, event_lookup) = (runner.clone(), table.clone(), lookup.clone());
    let getters: Vec<(String, MethodCallback)> = vec![
        ("persistent".to_owned(), {
            let lookup = lookup.clone();
            method(move |this,_| Ok(JsData::Bool(lookup.collection(this)?.persistent())))
        }),
        ("description".to_owned(), {
            let lookup = lookup.clone();
            method(move |this,_| Ok(lookup.collection(this)?.description().map_or(JsData::Undefined, JsData::Str)))
        }),
        (
            "onDidChangeCollection".to_owned(),
            method(move |this,_| {
                let event = event_lookup.collection(this)?.on_did_change_collection();
                Ok(event_function(&event_runner, &event_table, "onDidChangeCollection", event, |()| JsData::Undefined))
            }),
        ),
    ];
    let setters: Vec<(String, MethodCallback)> = vec![
        ("persistent".to_owned(), {
            let lookup = lookup.clone();
            method(move |this,args| {
                let Some(JsData::Bool(value)) = args.first() else { return Err("persistent precisa ser booleano".to_owned()) };
                lookup.collection(this)?.set_persistent(*value);
                Ok(JsData::Undefined)
            })
        }),
        ("description".to_owned(), {
            let lookup = lookup.clone();
            method(move |this,args| {
                let description = documentation_from_js(args.first()).map_err(|error| error.message)?;
                lookup.collection(this)?.set_description(description.as_ref());
                Ok(JsData::Undefined)
            })
        }),
    ];

    let constructor = runner.define_accessor_class(
        ClassSpec {
            name: CLASS_NAME.to_owned(),
            length: 0,
            construct: Rc::new(|_| Err("EnvironmentVariableCollection não tem construtor público: use ExtensionContext.environmentVariableCollection".to_owned())),
            methods,
            getters,
            statics: vec![],
            parent: None,
        },
        setters,
    );
    install_iterator(runner, constructor);
}

/// `[Symbol.iterator]()` no protótipo: itera os pares de `_entries()`.
fn install_iterator(runner: &NodeRunner, constructor: JsHandle) {
    let install = runner
        .eval("(constructor) => { constructor.prototype[Symbol.iterator] = function () { return this._entries()[Symbol.iterator](); }; }", "je-exthost/environment-variable-iterator.js")
        .expect("a função auxiliar do iterador avalia");
    runner
        .call_data_throwing(&install, &[JsData::Handle(Rc::new(constructor))])
        .unwrap_or_else(|_| panic!("instalar o iterador de EnvironmentVariableCollection não lança"));
}
