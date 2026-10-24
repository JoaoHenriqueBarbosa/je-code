//! A API `vscode` que as extensões enxergam, montada em Rust sobre os serviços do host de extensões (`ExtHost*`).
//!
//! Cada namespace é um [`JsData::Object`](zjsc::api::embed_node::JsData) de funções nativas (`make_function`) e cada classe
//! é uma [`ClassSpec`](zjsc::api::embed_node::ClassSpec); o objeto inteiro é o `exports` de `require('vscode')`. Nada de
//! JavaScript no produto: as funções chamam os mesmos `ExtHost*` portados do `extHost*.ts`.

pub mod commands;
pub mod disposable;
pub mod l10n;
