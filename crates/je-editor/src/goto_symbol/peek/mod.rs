//! Porte de `src/vs/editor/contrib/gotoSymbol/browser/peek`: o peek de referências. [`references_controller`] é o
//! `referencesController.ts` (o controlador do peek, a chave `referenceSearchVisible` e os atalhos). O `ReferenceWidget`
//! (`referencesWidget.ts`) e a árvore (`referencesTree.ts`) são a fatia seguinte, que implementa o contrato
//! [`references_controller::IReferenceWidget`].

pub mod references_controller;
