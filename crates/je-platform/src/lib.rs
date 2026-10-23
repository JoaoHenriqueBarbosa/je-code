//! Porte de `src/vs/platform` do VS Code.
//!
//! Portado até agora: `configuration` (modelos, registro, camadas e o `ConfigurationService`), `files`,
//! `log`, `environment` (argv, ambiente nativo e caminhos de dados do usuário), `policy`, `state` e `storage`
//! (sobre o `je-storage`), mais `product` e `resources` (o que eles pedem do produto e do processo
//! hospedeiro). Os valores dinâmicos do original
//! (`unknown`, `IStringDictionary<unknown>`) são [`je_base::dynamic::Dynamic`]; o texto que é chave
//! de configuração é [`je_base::JsString`] (UTF-16, como no JavaScript).

pub mod configuration;
pub mod environment;
pub mod files;
pub mod js;
pub mod log;
pub mod policy;
pub mod product;
pub mod resources;
pub mod state;
pub mod storage;
pub mod workspace;

#[doc(hidden)]
pub use je_base::event as __je_event;
#[doc(hidden)]
pub use je_base::lifecycle as __je_lifecycle;
