//! Porte de `src/vs/workbench/contrib/notebook/test/browser/cellToolbars.test.ts` (suíte `Notebook cell toolbars`).
//!
//! Nenhum dos 2 casos foi portado, e o `CellTitleToolbarPart` (`browser/view/cellParts/cellToolbars.ts`) também não existe na produção.
//! Casos de fora e o motivo exato:
//! - `title overflow menu escapes the notebook clipping container` e `delete overflow menu escapes the notebook clipping container`
//!   (o mesmo corpo, repetido para o menu `title` e para o `delete`): o caso monta o `CellTitleToolbarPart` sobre um contêiner do DOM
//!   (`$('.cell-title-toolbar')` anexado ao `document.body`), acha o botão `.codicon-toolbar-more` por `querySelector`, dispara um
//!   `MouseEvent('mousedown')` nele e lê o `domForShadowRoot` do `IContextMenuDelegate` e a classe `cell-toolbar-dropdown-active`
//!   que o `ICellParts` alterna. Tudo isso é DOM e CSS, que o porte não tem no caminho de pintura (a UI é uma árvore de widgets
//!   nativa, e os `.css` e `.html` do VS Code são só especificação). O comportamento (o menu de estouro abrir sem clipagem do
//!   contêiner do notebook e a marca de menu ativo enquanto ele está aberto) volta a ser testável quando o widget nativo de barra de
//!   ferramentas da célula for portado, e o caso entra então com o clique no botão no lugar do `mousedown`.
