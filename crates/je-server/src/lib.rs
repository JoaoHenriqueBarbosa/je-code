//! Porte de `src/vs/server` do VS Code: o servidor remoto.
//!
//! `cookie` é o pacote `cookie` do npm que o servidor usa para ler o cabeçalho `Cookie`;
//! `server_connection_token` é `serverConnectionToken.ts` e `server_environment_service` o que o porte já tem de
//! `serverEnvironmentService.ts`.

pub mod cookie;
pub mod server_connection_token;
pub mod server_environment_service;
