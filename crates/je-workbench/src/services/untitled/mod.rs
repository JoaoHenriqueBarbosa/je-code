//! Porte de `src/vs/workbench/services/untitled/common`: os editores de texto sem título. O modelo
//! (`untitledTextEditorModel.ts`) em [`untitled_text_editor_model`], o serviço (`untitledTextEditorService.ts`) em
//! [`untitled_text_editor_service`], a entrada de editor (`untitledTextEditorInput.ts`) em [`untitled_text_editor_input`] e
//! o serializador e o handler de working copy (`untitledTextEditorHandler.ts`) em [`untitled_text_editor_handler`].

pub mod untitled_text_editor_handler;
pub mod untitled_text_editor_input;
pub mod untitled_text_editor_model;
pub mod untitled_text_editor_service;
