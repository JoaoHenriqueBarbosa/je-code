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

pub mod commands;
pub mod debug;
pub mod disposable;
pub mod enums;
pub mod env;
pub mod events;
pub mod extensions;
pub mod file_system_events;
pub mod l10n;
pub mod js_object;
pub mod language_types;
pub mod languages;
pub mod output_channel;
pub mod progress;
pub mod quick_input;
pub mod quick_session;
pub mod scm;
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
