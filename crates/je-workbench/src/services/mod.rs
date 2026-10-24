//! Porte de `src/vs/workbench/services`: um módulo por serviço, na árvore do original.

pub mod keybinding;
pub mod search;
pub mod editor;

pub mod suggest;
pub mod policies;

pub mod preferences;
pub mod text_mate;
pub mod user_activity;
pub mod authentication;
pub mod user_attention;
pub mod extensions;
pub mod statusbar;
pub mod layout;
pub mod accounts;

pub mod extension_management;

// Contratos de serviço que os dublês de `test::common::workbench_test_services` implementam.
pub mod lifecycle;
pub mod user_data_profile;
pub mod working_copy;
pub mod files_configuration;

// `LabelService`, `AbstractPathService` e os contratos de ambiente e de agente remoto de que eles dependem.
pub mod environment;
pub mod label;
pub mod path;
pub mod remote;
// Serviço de arquivos de texto: codificação (`textfile/common/encoding.ts`).
pub mod textfile;

// `ILanguageDetectionService` e o `ITextEditorModel`/`ITextModelService` que os modelos de texto do workbench leem.
pub mod language_detection;
pub mod textmodel_resolver;

// `IEditorService` reduzido (o que o `findViewStateForEditor` e o `SideBySideEditorInput` leem).
// Configuração do workbench: `WorkspaceService`, edição e as camadas por workspace e por pasta.
pub mod configuration;

// Resolvedor de variáveis de configuração (`${workspaceFolder}`, `${env:X}`, `${config:X}`, `${command:X}`, `${input:X}`).
pub mod configuration_resolver;

// Estrutura de tópicos (`IOutlineService`) e rótulo das pastas do workspace, que as migalhas leem.
pub mod outline;
pub mod workspaces;

// URIs de célula, de metadados e de saída dos notebooks (`notebookDocumentService.ts`).
pub mod notebook;
