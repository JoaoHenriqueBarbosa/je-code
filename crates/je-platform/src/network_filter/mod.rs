//! Rede e filtros de domínio de `src/vs/platform/networkFilter` do VS Code: o `domainMatcher` (normalização e
//! casamento de padrões de domínio), o serviço de filtro de rede do agente e os identificadores das configurações.

pub mod domain_matcher;

pub mod network_filter_service;
pub mod settings;
