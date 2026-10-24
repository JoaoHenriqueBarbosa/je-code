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
pub mod host;

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
// Contrato do serviço de decorações de recursos (`decorations/common/decorations.ts`), que o `TextFileService` lê.
pub mod decorations;

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

// Editores de texto sem título (`untitledTextEditorModel.ts`, `untitledTextEditorService.ts`, `untitledTextEditorInput.ts` e
// `untitledTextEditorHandler.ts`).
pub mod untitled;

// Propriedades comuns da telemetria do workbench (`telemetry/browser/workbenchCommonProperties.ts`).
pub mod telemetry;
// Filtros do contexto de atribuição de experimentos (`assignmentContextFilter.ts` e o contrato `IAssignmentFilter`).
pub mod assignment;

// Contrato de `files/common/elevatedFileService.ts` (a escrita de arquivo com privilégios elevados).
pub mod files;
// Serviço de views (`views/common/viewContainerModel.ts` e `views/browser/viewDescriptorService.ts`).
pub mod views;
