//! Porte de `src/vs/workbench/contrib/searchEditor/test/browser/searchEditorModel.test.ts` (suíte `SearchEditorModel`), caso
//! a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo do caso.
//! - O `teardown(() => searchEditorModelFactory.models.delete(resource))` é o `Drop` de `Teardown`.
//! - `{ resolve: () => Promise.reject(new Error('Not implemented')) }` é `RejectingReference`, que implementa
//!   `ISearchEditorModelReference`; a identidade `===` do original é `Rc::ptr_eq`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::errors::Error;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_workbench::contrib::search_editor::search_editor_model::{
    ISearchEditorModelReference, SearchEditorData, SearchEditorModel, search_editor_model_factory,
};

/// `{ resolve: () => Promise.reject(new Error('Not implemented')) }`.
struct RejectingReference;

impl ISearchEditorModelReference for RejectingReference {
    fn resolve(&self) -> Promise<SearchEditorData> {
        Promise::rejected(Error::new("Not implemented"))
    }
}

/// `const resource = URI.from({ scheme: 'search-editor', fragment: 'test' })`.
fn resource() -> URI {
    URI::from(
        &UriComponents { scheme: Some("search-editor".into()), fragment: Some("test".into()), ..Default::default() },
        false,
    )
}

/// `teardown(() => searchEditorModelFactory.models.delete(resource))`.
struct Teardown(URI);

impl Drop for Teardown {
    fn drop(&mut self) {
        search_editor_model_factory().models.borrow_mut().delete(&self.0);
    }
}

#[test]
fn does_not_delete_a_replacement_model_when_disposed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let resource = resource();
    let _teardown = Teardown(resource.clone());

    let model_reference: Rc<dyn ISearchEditorModelReference> = Rc::new(RejectingReference);
    search_editor_model_factory().models.borrow_mut().set(resource.clone(), model_reference);
    let model = SearchEditorModel::new(resource.clone());

    let replacement_model_reference: Rc<dyn ISearchEditorModelReference> = Rc::new(RejectingReference);
    search_editor_model_factory().models.borrow_mut().set(resource.clone(), replacement_model_reference.clone());
    model.dispose();

    let current = search_editor_model_factory().models.borrow().get(&resource).cloned().expect("o modelo de troca continua registrado");
    assert!(Rc::ptr_eq(&current, &replacement_model_reference));
}
