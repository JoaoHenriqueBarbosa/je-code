//! Porte de `src/vs/platform` do VS Code.
//!
//! Portado até agora: `configuration` (modelos, registro, camadas e o `ConfigurationService`), `files`,
//! `log`, `environment` (argv, ambiente nativo e caminhos de dados do usuário), `policy`, `state` e `storage`
//! (sobre o `je-storage`), mais `product` e `resources` (o que eles pedem do produto e do processo
//! hospedeiro). Os valores dinâmicos do original
//! (`unknown`, `IStringDictionary<unknown>`) são [`je_base::dynamic::Dynamic`]; o texto que é chave
//! de configuração é [`je_base::JsString`] (UTF-16, como no JavaScript).

pub mod action;
pub mod actions;
pub mod commands;
pub mod backup;
pub mod configuration;
pub mod dialogs;
pub mod contextkey;
pub mod environment;
pub mod files;
pub mod js;
pub mod keyboard_layout;
pub mod keybinding;
pub mod log;
pub mod markers;
pub mod notification;
pub mod policy;
pub mod request;
pub mod product;
pub mod resources;
pub mod state;
pub mod telemetry;
pub mod url;
pub mod storage;
pub mod user_data;
pub mod user_data_profile;
pub mod workspace;
pub mod workspace_trust;
pub mod workspaces;

pub mod window;

#[doc(hidden)]
pub use je_base::event as __je_event;
#[doc(hidden)]
pub use je_base::lifecycle as __je_lifecycle;

pub mod user_data_sync;
pub mod update;

pub mod remote;

pub mod opener;

pub mod terminal;

pub mod sandbox;
pub mod network_filter;
pub mod browser_view;
pub mod quickinput;
pub mod remote_tunnel;
pub mod otel;
pub mod web_content_extractor;

pub mod agent_host;
pub mod external_services;

pub mod encryption;
pub mod external_terminal;
pub mod registry;
pub mod secrets;

pub mod label;
pub mod path;
pub mod uri_identity;
pub mod virtual_workspace;

pub mod diagnostics;
