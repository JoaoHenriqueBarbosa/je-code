//! Porte de `src/vs/editor/contrib/hover/browser/contentHoverTypes.ts`: o [`ContentHoverResult`] e a sua variante
//! filtrada (`FilteredContentHoverResult`).
//!
//! Notas de tradução:
//! - `FilteredContentHoverResult extends ContentHoverResult` é uma variante do mesmo tipo (o campo `original`): não há
//!   herança, e o `filter` de uma instância filtrada delega no `original`, como no original. O construtor da variante é
//!   [`ContentHoverResult::new_filtered`].
//! - `filter(anchor)` devolve `this` quando nenhuma parte sai; para isso o resultado é compartilhado (`Rc`) e `filter`
//!   devolve outro `Rc` do mesmo objeto.

use std::rc::Rc;

use crate::hover::content_hover_computer::ContentHoverComputerOptions;
use crate::hover::hover_types::{HoverAnchor, IHoverPart};

/// `ContentHoverResult` (e, com `original`, o `FilteredContentHoverResult`).
pub struct ContentHoverResult {
    pub hover_parts: Vec<Rc<dyn IHoverPart>>,
    pub is_complete: bool,
    pub options: ContentHoverComputerOptions,
    /// O `original` do `FilteredContentHoverResult`; `None` num resultado que não é filtrado.
    original: Option<Rc<ContentHoverResult>>,
}

impl ContentHoverResult {
    /// `new ContentHoverResult(hoverParts, isComplete, options)`.
    pub fn new(hover_parts: Vec<Rc<dyn IHoverPart>>, is_complete: bool, options: ContentHoverComputerOptions) -> Rc<Self> {
        Rc::new(Self { hover_parts, is_complete, options, original: None })
    }

    /// `new FilteredContentHoverResult(original, messages, isComplete, options)`.
    pub fn new_filtered(original: Rc<ContentHoverResult>, messages: Vec<Rc<dyn IHoverPart>>, is_complete: bool, options: ContentHoverComputerOptions) -> Rc<Self> {
        Rc::new(Self { hover_parts: messages, is_complete, options, original: Some(original) })
    }

    /// O `original` do `FilteredContentHoverResult`, ou `None` se este resultado não é filtrado.
    pub fn original(&self) -> Option<&Rc<ContentHoverResult>> {
        self.original.as_ref()
    }

    /// `filter(anchor)`: as partes que ainda valem para o ancorador. Um resultado filtrado refiltra o original.
    pub fn filter(self: &Rc<Self>, anchor: &HoverAnchor) -> Rc<ContentHoverResult> {
        if let Some(original) = &self.original {
            return original.filter(anchor);
        }
        let filtered_hover_parts: Vec<Rc<dyn IHoverPart>> = self.hover_parts.iter().filter(|part| part.is_valid_for_hover_anchor(anchor)).cloned().collect();
        if filtered_hover_parts.len() == self.hover_parts.len() {
            return Rc::clone(self);
        }
        ContentHoverResult::new_filtered(Rc::clone(self), filtered_hover_parts, self.is_complete, self.options.clone())
    }
}
