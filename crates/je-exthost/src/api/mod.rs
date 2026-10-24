//! A API `vscode` que as extensões enxergam, montada em Rust sobre os serviços do host de extensões (`ExtHost*`).
//!
//! Cada namespace é um [`JsData::Object`](zjsc::api::embed_node::JsData) de funções nativas (`make_function`) e cada classe
//! é uma [`ClassSpec`](zjsc::api::embed_node::ClassSpec); o objeto inteiro é o `exports` de `require('vscode')`. Nada de
//! JavaScript no produto: as funções chamam os mesmos `ExtHost*` portados do `extHost*.ts`.
//!
//! O que existe, namespace a namespace, e o que falta de cada um está no cabeçalho do módulo dele (`commands`, `workspace`,
//! `window`, `l10n`) e na lista medida de `wip/notes/exthost-api-usage.md`. Os namespaces que dependem de um cliente
//! `MainThread*` do workbench (`workspace`, `window`) só entram no objeto `vscode` quando o host é montado com um
//! [`IMainThreadBinder`](crate::in_process::IMainThreadBinder) que os fornece.

use std::rc::Rc;

use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NodeRunner, CLASS_KEY};

pub mod commands;
pub mod disposable;
pub mod enums;
pub mod events;
pub mod l10n;
pub mod types;
pub mod uri;
pub mod window;
pub mod workspace;

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
