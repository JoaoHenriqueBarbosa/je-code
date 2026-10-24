//! Porte de `src/vs/workbench/contrib/terminalContrib/quickFix/browser`: os quick fixes do terminal (o contrato, os quick fixes
//! internos, o serviço das extensões e o addon que os resolve). A decoração e o menu do addon ficam de fora (ver
//! `quick_fix_addon`).

pub mod quick_fix;

pub mod terminal_quick_fix_builtin_actions;

pub mod terminal_quick_fix_service;

pub mod quick_fix_addon;
