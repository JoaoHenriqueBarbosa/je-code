//! Porte do que o `ActionWidgetService` pede de `src/vs/platform/contextview/browser` e de
//! `src/vs/base/browser/ui/contextview/contextview.ts`: o contrato do serviço de visão de contexto, a âncora, a animação de
//! fechamento e uma visão de contexto sem DOM (o estado que a `ContextView` guarda: o delegate ativo, o que desmonta no
//! `hide`, a animação em curso e a posição resolvida por `layout2d`).
//!
//! O `ViewElement` é o estado nativo de um elemento (lista de classes, propriedades de estilo, pai, retângulo): o
//! original lê isso do DOM, aqui o widget nativo o guarda.

pub mod context_view;
pub mod context_view_service;
pub mod view_element;
