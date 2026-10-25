//! Porte de `src/vs/editor/browser/widget/multiDiffEditor/model.ts`: o modelo do editor de diff de vários arquivos
//! (`IMultiDiffEditorModel`), o item de documento (`IDocumentDiffItem`) e o lado de um item (`DiffItemSource`).
//!
//! Divergências:
//! - `ITextModel` é o [`TextModel`] concreto (`Rc<TextModel>`); `Event<void>` é `Event<()>`.
//! - `IDocumentDiffItem` é um trait: `options?` e `onOptionsDidChange?` têm corpo padrão `None`, e os campos `readonly` são
//!   métodos. `contextKeys?: Record<string, ContextKeyValue>` é uma lista de pares `(chave, Dynamic)` na ordem de inserção, como
//!   no `MultiDiffEditorItem`; `IDiffEditorOptions` é um objeto [`Dynamic`] (ver `diff_editor_options`).
//! - `RefCounted<IDocumentDiffItem>` é um [`DocumentDiffItemRef`]: o `RefCounted` de `je_base::lifecycle` sobre o `Rc<dyn
//!   IDocumentDiffItem>`, que cada referência (`Rc<RefCounted<...>>`) mantém vivo.
//! - `documents: IValueWithChangeEvent<readonly RefCounted<IDocumentDiffItem>[] | 'loading'>` devolve o [`DocumentsValue`] (o
//!   array, imutável e compartilhado, ou `'loading'`); a igualdade dos observáveis é a identidade do array, como no original.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Event, ValueWithChangeEventLike};
use je_base::lifecycle::RefCounted;
use je_base::observable::StrictEquals;
use je_base::uri::URI;

use crate::text_model::TextModel;

/// `ContextKeyValue` de cada chave, na ordem de inserção (`Record<string, ContextKeyValue>`).
pub type ContextKeys = Vec<(String, Dynamic)>;

/// A resource participating on one side of a document diff.
#[derive(Clone)]
pub struct DiffItemSource {
    pub uri: URI,
    pub text_model: Option<Rc<TextModel>>,
}

impl DiffItemSource {
    pub fn new(uri: URI, text_model: Option<Rc<TextModel>>) -> Self {
        Self { uri, text_model }
    }
}

/// `IDocumentDiffItem`.
pub trait IDocumentDiffItem {
    /// `undefined` if the file was created.
    fn original(&self) -> Option<&DiffItemSource>;

    /// `undefined` if the file was deleted.
    fn modified(&self) -> Option<&DiffItemSource>;

    /// `options?: IDiffEditorOptions`.
    fn options(&self) -> Option<Dynamic> {
        None
    }

    /// `onOptionsDidChange?: Event<void>`.
    fn on_options_did_change(&self) -> Option<Event<()>> {
        None
    }

    /// `contextKeys?`.
    fn context_keys(&self) -> Option<ContextKeys> {
        None
    }
}

/// `RefCounted<IDocumentDiffItem>`: uma referência ao item.
pub type DocumentDiffItemRef = Rc<RefCounted<Rc<dyn IDocumentDiffItem>>>;

/// `readonly RefCounted<IDocumentDiffItem>[] | 'loading'`.
#[derive(Clone)]
pub enum DocumentsValue {
    Loading,
    Items(Rc<Vec<DocumentDiffItemRef>>),
}

impl StrictEquals for DocumentsValue {
    fn strict_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Loading, Self::Loading) => true,
            (Self::Items(a), Self::Items(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// `IMultiDiffEditorModel`.
pub trait IMultiDiffEditorModel {
    fn documents(&self) -> Rc<dyn ValueWithChangeEventLike<DocumentsValue>>;

    /// `contextKeys?: Record<string, ContextKeyValue>`.
    fn context_keys(&self) -> Option<ContextKeys> {
        None
    }
}
