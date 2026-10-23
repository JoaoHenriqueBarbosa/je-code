//! Porte de `src/vs/platform/terminal` do VS Code.
//!
//! Portado até agora: as capacidades do terminal ([`capabilities`]) e as variáveis de ambiente que as
//! extensões contribuem ([`environment_variable`], [`environment_variable_collection`],
//! [`environment_variable_shared`]).

pub mod capabilities;
pub mod environment_variable;
pub mod environment_variable_collection;
pub mod environment_variable_shared;

pub mod request_store;
pub mod terminal;
pub mod terminal_environment;
pub mod terminal_profiles;
