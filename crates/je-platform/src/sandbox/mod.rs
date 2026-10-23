//! Porte de `src/vs/platform/sandbox`: o verificador das dependências do sandbox (`bubblewrap`, `socat`),
//! a resolução das configurações locais contra o piso gerenciado e as configurações do sandbox do agente.

pub mod node_support;
pub mod sandbox_helper;
pub mod sandbox_helper_service;
pub mod sandbox_settings_resolution_helper;
pub mod settings;
