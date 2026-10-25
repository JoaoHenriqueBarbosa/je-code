//! Porte de `src/vs/workbench/test/browser/viewlet.test.ts` (suíte `Viewlets`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o [`LeakGuard`] criado no começo de cada caso. O registro de painéis
//!   (`Registry.add(Extensions.Viewlets, new PaneCompositeRegistry())`) nasce no carregamento do módulo do original, antes do
//!   rastreador de vazamentos; aqui ele nasce no primeiro `pane_composite_registry`, que o caso 3 chama antes do guarda.
//! - **`TestViewlet`.** A `PaneComposite` abstrata não foi portada (o descritor só guarda o construtor); o `TestViewlet` é um
//!   [`IPaneComposite`] de id `id` e a fábrica do descritor (`IConstructorSignature<PaneComposite>`) o devolve. Nenhum caso instancia
//!   o painel (o `layout` e o `setBoundarySashes` que lançam no original não são chamados).
//! - **`Registry.as<PaneCompositeRegistry>(Extensions.Viewlets)`** é `pane_composite_registry(extensions::VIEWLETS)`. Os três
//!   `assert(isFunction(...))` do caso 3 (os métodos `registerPaneComposite`, `getPaneComposite` e `getPaneComposites` existem) são
//!   garantidos na compilação.
//! - **`d === registry.getPaneComposite(id)`** é `PaneCompositeDescriptor::is_same`: o descritor devolvido é o registrado (o
//!   clone compartilha a fábrica).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_instantiation::InstantiationService;
use je_workbench::browser::pane_composite::{PaneCompositeDescriptor, PaneCompositeFactory, extensions, pane_composite_registry};
use je_workbench::services::panecomposite::panecomposite::IPaneComposite;

/// `TestViewlet`.
struct TestViewlet;

impl IPaneComposite for TestViewlet {
    fn get_id(&self) -> String {
        "id".to_owned()
    }
}

/// `TestViewlet` como `IConstructorSignature<PaneComposite>`.
fn test_viewlet() -> PaneCompositeFactory {
    Rc::new(|_: &Rc<InstantiationService>| Rc::new(TestViewlet) as Rc<dyn IPaneComposite>)
}

#[test]
fn viewlet_descriptor_api() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let d = PaneCompositeDescriptor::create(test_viewlet(), "id", "name", Some("class"), Some(5.0), None, None);
    assert_eq!(d.id, "id");
    assert_eq!(d.name, "name");
    assert_eq!(d.css_class.as_deref(), Some("class"));
    assert_eq!(d.order, Some(5.0));
}

#[test]
fn editor_aware_viewlet_descriptor_api() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let mut d = PaneCompositeDescriptor::create(test_viewlet(), "id", "name", Some("class"), Some(5.0), None, None);
    assert_eq!(d.id, "id");
    assert_eq!(d.name, "name");

    d = PaneCompositeDescriptor::create(test_viewlet(), "id", "name", Some("class"), Some(5.0), None, None);
    assert_eq!(d.id, "id");
    assert_eq!(d.name, "name");
}

#[test]
fn viewlet_extension_point_and_registration() {
    let registry = pane_composite_registry(extensions::VIEWLETS);
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let old_count = registry.get_pane_composites().len();
    let d = PaneCompositeDescriptor::create(test_viewlet(), "reg-test-id", "name", None, None, None, None);
    registry.register_pane_composite(d.clone());

    assert!(registry.get_pane_composite("reg-test-id").is_some_and(|registered| d.is_same(&registered)));
    assert_eq!(old_count + 1, registry.get_pane_composites().len());
}
