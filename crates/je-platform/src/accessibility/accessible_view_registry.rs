//! Porte de `src/vs/platform/accessibility/browser/accessibleViewRegistry.ts`: o contrato `IAccessibleViewImplementation`
//! e o `AccessibleViewRegistry`.
//!
//! Notas de tradução:
//! - **`AccessibleViewRegistry`** (singleton do módulo) é um `thread_local` com as funções [`register`] e
//!   [`get_implementations`]; o `IDisposable` literal que o `register` devolve é um [`plain_disposable`].
//! - **`type` e `getProvider`** são `view_type()` e `get_provider(accessor)`; `when?` é `when()` com `None` por padrão.

use std::cell::RefCell;
use std::rc::Rc;

use je_instantiation::ServicesAccessor;

use super::accessible_view::{AccessibleViewContentProvider, AccessibleViewType};
use crate::contextkey::ContextKeyExpression;
use je_base::lifecycle::{DisposableRef, plain_disposable};

/// `IAccessibleViewImplementation`.
pub trait IAccessibleViewImplementation {
    fn view_type(&self) -> AccessibleViewType;
    fn priority(&self) -> i32;
    fn name(&self) -> &str;
    /// The provider, or `None` if the view should not be shown.
    fn get_provider(&self, accessor: &ServicesAccessor) -> Option<AccessibleViewContentProvider>;
    fn when(&self) -> Option<ContextKeyExpression> {
        None
    }
}

thread_local! {
    static IMPLEMENTATIONS: RefCell<Vec<Rc<dyn IAccessibleViewImplementation>>> = const { RefCell::new(Vec::new()) };
}

/// `AccessibleViewRegistry.register`.
pub fn register(implementation: Rc<dyn IAccessibleViewImplementation>) -> DisposableRef {
    IMPLEMENTATIONS.with(|implementations| implementations.borrow_mut().push(implementation.clone()));
    plain_disposable(move || {
        IMPLEMENTATIONS.with(|implementations| {
            let mut implementations = implementations.borrow_mut();
            if let Some(index) = implementations.iter().position(|candidate| Rc::ptr_eq(candidate, &implementation)) {
                implementations.remove(index);
            }
        });
    })
}

/// `AccessibleViewRegistry.getImplementations`.
pub fn get_implementations() -> Vec<Rc<dyn IAccessibleViewImplementation>> {
    IMPLEMENTATIONS.with(|implementations| implementations.borrow().clone())
}
