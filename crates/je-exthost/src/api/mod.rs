//! A API `vscode` que as extensões enxergam, montada em Rust sobre os serviços do host de extensões (`ExtHost*`).
//!
//! Cada namespace é um [`JsData::Object`](zjsc::api::embed_node::JsData) de funções nativas (`make_function`) e cada classe
//! é uma [`ClassSpec`](zjsc::api::embed_node::ClassSpec); o objeto inteiro é o `exports` de `require('vscode')`. Nada de
//! JavaScript no produto: as funções chamam os mesmos `ExtHost*` portados do `extHost*.ts`.
//!
//! O que existe, namespace a namespace, e o que falta de cada um está no cabeçalho do módulo dele (`commands`, `workspace`,
//! `window`, `languages`, `l10n`) e na lista medida de `wip/notes/exthost-api-usage.md`. Os namespaces que dependem de um cliente
//! `MainThread*` do workbench (`workspace`, `window`, `languages`) só entram no objeto `vscode` quando o host é montado com um
//! [`IMainThreadBinder`](crate::in_process::IMainThreadBinder) que os fornece.

use std::rc::Rc;

use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner, CLASS_KEY};

use crate::convert::field;

pub mod authentication;
pub mod commands;
pub mod comments;
pub mod debug;
pub mod disposable;
pub mod enums;
pub mod env;
pub mod events;
pub mod extensions;
pub mod file_system;
pub mod file_system_events;
pub mod l10n;
pub mod js_object;
pub mod language_provider_types;
pub mod language_providers;
pub mod language_types;
pub mod languages;
pub mod notebooks;
pub mod output_channel;
pub mod progress;
pub mod quick_input;
pub mod quick_session;
pub mod scm;
pub mod tabs;
pub mod terminal;
pub mod text_editors;
pub mod tree_views;
pub mod types;
pub mod uri;
pub mod window;
pub mod workspace;
pub mod workspace_edit;

/// O identificador da extensão chamadora: o `vscode` é um só para todas as extensões, então o host não sabe qual delas chama e usa
/// este para o que o original lê da `IExtensionDescription` (o log de traço do `findFiles`, o canal de saída, o progresso).
pub const HOST_EXTENSION: &str = "je.exthost";

/// O objeto que a extensão segura de uma instância da classe nativa `class`: só a marca `$$class` e o `_id` (o handle do host).
pub(crate) fn handle_object(class: &str, handle: u32) -> JsData {
    JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str(class.to_owned())), ("_id".to_owned(), JsData::Num(f64::from(handle)))])
}

/// O handle de um objeto criado por [`handle_object`]; `Err` com o nome da classe se `this` não for uma instância dela.
pub(crate) fn handle_field(this: &JsData, class: &str) -> Result<u32, String> {
    match field(this, "_id") {
        Some(JsData::Num(id)) => Ok(*id as u32),
        _ => Err(format!("{class}: this inválido")),
    }
}

/// O acessor de leitura de uma classe de objetos vivos: acha o objeto de `this` na tabela `objects` com `lookup` e lê a propriedade com
/// `read`.
pub(crate) fn getter<O: 'static, T: 'static>(
    objects: &Rc<O>,
    name: &str,
    lookup: impl Fn(&O, &JsData) -> Result<Rc<T>, String> + 'static,
    read: impl Fn(&O, &Rc<T>) -> JsData + 'static,
) -> (String, MethodCallback) {
    let objects = objects.clone();
    (name.to_owned(), Rc::new(move |this, _| Ok(read(&objects, &lookup(&objects, this)?))))
}

/// O acessor de escrita correspondente a [`getter`]: o valor atribuído (ou `undefined`) vai para `write`.
pub(crate) fn setter<O: 'static, T: 'static>(
    objects: &Rc<O>,
    name: &str,
    lookup: impl Fn(&O, &JsData) -> Result<Rc<T>, String> + 'static,
    write: impl Fn(&O, &Rc<T>, &JsData) -> Result<(), String> + 'static,
) -> (String, MethodCallback) {
    let objects = objects.clone();
    (
        name.to_owned(),
        Rc::new(move |this, args| {
            write(&objects, &lookup(&objects, this)?, args.first().unwrap_or(&JsData::Undefined))?;
            Ok(JsData::Undefined)
        }),
    )
}

/// Um namespace com acessores (`vscode.workspace.name`, `vscode.workspace.onDidChangeConfiguration`): o motor copia o objeto comum
/// que atravessa a fronteira, então os acessores moram no protótipo de uma classe nativa `$<nome>` e o namespace é a instância
/// dela (um [`JsData::Object`] só com a marca `$$class`).
pub fn accessor_namespace(runner: &NodeRunner, name: &str, methods: Vec<(String, u32, MethodCallback)>, getters: Vec<(String, MethodCallback)>) -> JsData {
    let class = format!("${name}");
    runner.define_class(ClassSpec {
        name: class.clone(),
        length: 0,
        construct: Rc::new(|_| Err("um namespace da API não se constrói".to_owned())),
        methods,
        getters,
        statics: vec![],
        parent: None,
    });
    JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str(class))])
}
