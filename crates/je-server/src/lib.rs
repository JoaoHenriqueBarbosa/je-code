//! Porte de `src/vs/server` do VS Code: o servidor remoto.
//!
//! `cookie` é o pacote `cookie` do npm que o servidor usa para ler o cabeçalho `Cookie`;
//! `server_connection_token` é `serverConnectionToken.ts`, `server_environment_service` o que o porte já tem de
//! `serverEnvironmentService.ts` e `server_lifetime_service` é `serverLifetimeService.ts`.

pub mod cookie;
pub mod server_connection_token;
pub mod server_environment_service;
pub mod server_lifetime_service;
