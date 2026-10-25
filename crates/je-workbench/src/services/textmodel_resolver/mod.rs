//! Porte de `src/vs/workbench/services/textmodelResolver`, do contrato de `src/vs/editor/common/services/resolverService.ts` e do
//! `inMemoryTextModelService.ts`: o `ITextEditorModel` e o `ITextModelService` em [`resolver_service`], o `TextModelResolverService`
//! em [`text_model_resolver_service`] e o `InMemoryTextModelService` em [`in_memory_text_model_service`].

pub mod in_memory_text_model_service;
pub mod resolver_service;
pub mod text_model_resolver_service;
