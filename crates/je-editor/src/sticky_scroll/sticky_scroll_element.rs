//! Porte de `src/vs/editor/contrib/stickyScroll/browser/stickyScrollElement.ts`: o intervalo de linhas de um escopo
//! (`StickyRange`), o nó da árvore de escopos (`StickyElement`) e o modelo de um documento (`StickyModel`).
//!
//! Representação:
//! - `StickyElement` é compartilhado por referência no original (o pai guarda os filhos e o filho aponta para o pai),
//!   então é entregue como `Rc<StickyElement>`. O `children` é um `RefCell<Vec<Rc<StickyElement>>>`, porque o modelo
//!   de dobras (`_fromFoldingRegions`) cria o filho primeiro e o acrescenta à lista do pai depois. O `parent` guarda
//!   um `Weak` (o pai é dono dos filhos; um `Rc` nas duas direções formaria um ciclo).
//! - `URI` é o `URI` de `je_base`, e `version` é o `getVersionId()` do modelo de texto (`i64`).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::uri::URI;

/// `StickyRange`: o intervalo de linhas (a partir de 1) de um escopo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StickyRange {
    pub start_line_number: i32,
    pub end_line_number: i32,
}

impl StickyRange {
    pub const fn new(start_line_number: i32, end_line_number: i32) -> Self {
        Self { start_line_number, end_line_number }
    }
}

/// `StickyElement`.
pub struct StickyElement {
    /// Intervalo de linhas do escopo atual (`undefined` na raiz).
    pub range: Option<StickyRange>,
    /// Deve estar ordenado pela linha de início.
    pub children: RefCell<Vec<Rc<StickyElement>>>,
    /// Elemento pai do outline (`undefined` na raiz).
    parent: Option<Weak<StickyElement>>,
}

impl StickyElement {
    pub fn new(range: Option<StickyRange>, children: Vec<Rc<StickyElement>>, parent: Option<&Rc<StickyElement>>) -> Rc<Self> {
        Rc::new(Self { range, children: RefCell::new(children), parent: parent.map(Rc::downgrade) })
    }

    /// `parent`.
    pub fn parent(&self) -> Option<Rc<StickyElement>> {
        self.parent.as_ref().and_then(Weak::upgrade)
    }
}

/// `StickyModel`.
#[derive(Clone)]
pub struct StickyModel {
    pub uri: URI,
    pub version: i64,
    pub element: Option<Rc<StickyElement>>,
    pub outline_provider_id: Option<String>,
}
