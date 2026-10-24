//! Porte de `src/vs/workbench/services/textfile`: `common/encoding.ts` em [`encoding`], com o
//! `iconv-lite` que ele usa em [`iconv_lite`] e as tabelas das páginas de código DOS em [`code_pages`];
//! `common/textfiles.ts` em [`textfiles`], `common/textFileEditorModel.ts` em [`text_file_editor_model`],
//! `common/textFileEditorModelManager.ts` em [`text_file_editor_model_manager`] e
//! `common/textFileSaveParticipant.ts` em [`text_file_save_participant`].
//!
//! [`collaborators`] traz os contratos reduzidos dos serviços que esses arquivos leem e que ainda não foram portados
//! (progresso, backup de working copy, detecção de linguagem, acessibilidade e operações de arquivo de working copy), e
//! [`text_buffer_factory`] o `createTextBufferFactoryFromStream` do `textModel.ts`.

pub mod code_pages;
pub mod collaborators;
pub mod encoding;
pub mod iconv_lite;
pub mod text_buffer_factory;
pub mod text_file_editor_model;
pub mod text_file_editor_model_manager;
pub mod text_file_save_participant;
pub mod textfiles;
