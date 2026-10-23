//! Porte de `src/vs/platform/quickinput/browser/tree/quickInputTreeSorter.ts`.
//!
//! Divergências:
//! - `implements ITreeSorter<IQuickTreeItem>` não vira `impl ITreeSorter`: o trait vive em `je-widgets`
//!   (`tree::tree_types::ITreeSorter`), que não é dependência de `je-platform`. O método [`QuickInputTreeSorter::compare`]
//!   tem a mesma assinatura do trait (`fn compare(&self, &T, &T) -> i32`), então o `impl` é uma linha em
//!   quem unir as duas crates.
//! - `extends Disposable` é composição: um [`Disposable`] por dentro, descartado em `dispose`.
//! - `a.label < b.label` do JavaScript compara por unidade UTF-16, que é a ordem de [`JsString`].
//! - `if (a.description && b.description)` testa verdade: descrição vazia conta como ausente.

use std::cell::Cell;
use std::cmp::Ordering;
use std::rc::Rc;

use je_base::JsString;
use je_base::lifecycle::{Disposable, IDisposable};

use crate::quickinput::quick_input::IQuickTreeItem;

pub struct QuickInputTreeSorter {
    base: Rc<Disposable>,
    sort_by_label: Cell<bool>,
}

/// `-1`, `0` ou `1`, o que os `if (a < b) return -1; else if (a > b) return 1;` do original devolvem.
fn sign(ordering: Ordering) -> i32 {
    match ordering {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// A descrição quando é verdadeira em JavaScript (`undefined` e `''` são falsas).
fn truthy_description(item: &IQuickTreeItem) -> Option<&JsString> {
    item.base.description.as_ref().filter(|description| !description.is_empty())
}

impl QuickInputTreeSorter {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), sort_by_label: Cell::new(true) })
    }

    pub fn sort_by_label(&self) -> bool {
        self.sort_by_label.get()
    }

    pub fn set_sort_by_label(&self, value: bool) {
        self.sort_by_label.set(value);
    }

    pub fn compare(&self, a: &IQuickTreeItem, b: &IQuickTreeItem) -> i32 {
        // No-op
        if !self.sort_by_label.get() {
            return 0;
        }

        let by_label = sign(a.base.label.cmp(&b.base.label));
        if by_label != 0 {
            return by_label;
        }
        // use description to break ties
        match (truthy_description(a), truthy_description(b)) {
            (Some(a_description), Some(b_description)) => sign(a_description.cmp(b_description)),
            (Some(_), None) => -1,
            (None, Some(_)) => 1,
            (None, None) => 0,
        }
    }
}

impl IDisposable for QuickInputTreeSorter {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "QuickInputTreeSorter"
    }
}
