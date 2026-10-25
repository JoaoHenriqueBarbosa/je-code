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
pub mod query_builder;

pub mod file_search_manager;
pub mod ripgrep_file_search;
pub mod search_ext_conversion_types;
pub mod text_search_manager;

pub mod file_walker;

pub mod raw_search_service;
pub mod ripgrep_disk_path;
pub mod ripgrep_search_provider;
pub mod text_search_adapter;
