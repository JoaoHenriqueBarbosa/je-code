//! Porte de `src/vs/workbench/common`: os tipos e os modelos de dados do workbench, sem interface.

pub mod comments;

// O registro das contribuições do workbench (`common/contributions.ts`).
pub mod contributions;

pub mod contextkeys;

pub mod notification_telemetry;
pub mod notifications;

pub mod editor;

pub mod memento;

pub mod resources;

// As cores `NOTIFICATIONS_*` de `common/theme.ts`.
pub mod theme;

pub mod views;

// `ITreeView`, `ITreeViewDataProvider` e `ITreeViewDragAndDropController` (`common/views.ts`).
pub mod tree_view_contracts;

// Registro de migrações de configuração (`common/configuration.ts`).
pub mod configuration;
