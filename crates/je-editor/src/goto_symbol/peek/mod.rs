//! Porte de `src/vs/editor/contrib/gotoSymbol/browser/peek`: o peek de referências. [`references_controller`] é o
//! `referencesController.ts` (o controlador do peek, a chave `referenceSearchVisible` e os atalhos), [`references_widget`] é o
//! `referencesWidget.ts` (o `ReferenceWidget`, o editor de prévia, o sash e a árvore) e [`references_tree`] é o
//! `referencesTree.ts` (a fonte de dados, o delegate, os provedores e os renderers da árvore).

pub mod references_controller;
pub mod references_tree;
pub mod references_widget;
