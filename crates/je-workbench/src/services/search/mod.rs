//! Porte de `src/vs/workbench/services/search`: `common/*.ts` vira módulos diretos desta pasta.

pub mod ignore_file;
pub mod replace;
pub mod ripgrep_search_utils;
pub mod search;
pub mod search_ext_types;

pub mod folder_query_search_tree;
pub mod ripgrep_text_search_engine;
pub mod search_ext_types_internal;
pub mod search_helpers;
