//! A camada de UI nativa do je-code: o que o DOM é para o VS Code, sem DOM e sem CSS.
//!
//! O VS Code monta cada widget como uma árvore de elementos HTML estilizados por CSS e posicionados,
//! quase sempre, por código próprio (`layout(width, height)` em splitview, grid, list, tree). O porte
//! mantém a lógica de cada widget como no original e troca o elemento por um [`Node`] desta árvore:
//!
//! - **Geometria explícita.** Cada nó tem um retângulo em pixels lógicos relativo ao pai, definido pelo
//!   widget dono, como o `style.top/left/width/height` que o VS Code escreve. Não há motor de layout
//!   genérico: o layout de cada widget é o do código original portado.
//! - **Estilo resolvido.** Cor de fundo, borda, raio, texto e ícone já vêm resolvidos do tema pelo
//!   widget (a regra de CSS correspondente vira código do widget). Nada de seletores nem cascata.
//! - **Pintura.** A árvore vira uma [`je_render::DisplayList`] por frame, pela ordem dos filhos (a
//!   ordem do DOM), com recorte por nó (`overflow: hidden`).
//! - **Eventos.** Ponteiro por hit test do nó mais ao fundo; teclado para o nó com foco; eventos com
//!   fase de captura e de bolha como no DOM, porque os widgets do VS Code dependem disso. Ver [`events`].
//! - **Caixa.** O mínimo do modelo de caixa do CSS que o código dos widgets escreve ou herda:
//!   `left/top/width/height` explícitos, `width/height: 100%` ([`Tree::set_fill`]), `right/bottom`
//!   ([`Tree::set_right`], [`Tree::set_bottom`]), `z-index` entre irmãos, `pointer-events` e `display:
//!   none`.
//! - **Acessibilidade.** Cada nó declara papel e rótulo (`role`, `aria-label`), para a árvore do
//!   AccessKit.

pub mod events;
mod tree;
pub mod wheel;

pub use events::{Event, EventKind, Handler, KeyData, Modifiers, Phase, PointerButton, PointerData, PointerInput, PointerInputKind, Ui};
pub use wheel::{DeltaMode, StandardWheelEvent, WheelData};
pub use tree::{Border, Content, Cursor, NodeId, PointerEvents, NodeStyle, Role, Tree};
