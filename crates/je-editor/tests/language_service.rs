//! Porte de `src/vs/editor/test/common/services/languageService.test.ts` (suíte `LanguageService`), caso a caso e na mesma ordem.
//!
//! O original da versão 1.143.1 tem sete casos (os quatro `for` de `ID`, `MIME type`, `file path` e
//! `first line` viram quatro funções).
//!
//! Notas de tradução:
//! - `TestLanguageService.setLanguages` chama `registry.setDynamicLanguages`, que o `LanguageService` do porte
//!   já expõe como `set_dynamic_languages`; a subclasse não é necessária.
//! - `getListenerCount` lê `_onDidChange._size`; vira `on_did_change_listener_count_for_testing`.
//! - O `setup`/`teardown` com `LanguageService.instanceCount` vira `with_suite`, que confere a contagem depois
//!   do descarte dos `disposables` do caso.
//!
//! Pendente:
//! - `live text models share a registry listener and release it when disposed`: precisa do `IModelService`
//!   (`createModel`/`getModels`), que não existe no porte. Além disso, o `LanguageSelection` do porte assina o
//!   evento do serviço uma vez por seleção observada (o original compartilha uma única assinatura por um
//!   `observableSignalFromEvent`), então a contagem esperada `[0, 1, 1, 0, 1, 1, 0]` não seria alcançada
//!   mesmo com o `IModelService`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::lifecycle::{DisposableRef, IDisposable, MutableDisposable};
use je_base::uri::URI;
use je_editor::language_service::{ILanguageSelection, ILanguageService, LanguageService};
use je_editor::modes_registry::{LanguageExtensionPoint, PLAINTEXT_LANGUAGE_ID};

mod language_service {
    use super::*;

    /// O `setup`/`teardown` da suíte: guarda `LanguageService.instanceCount` e confere no fim.
    fn with_suite(body: impl FnOnce(&LeakGuard)) {
        struct InstanceCountCheck(i32);
        impl Drop for InstanceCountCheck {
            fn drop(&mut self) {
                if !std::thread::panicking() {
                    assert_eq!(LanguageService::instance_count(), self.0);
                }
            }
        }
        // Declarado antes do `LeakGuard`, portanto conferido depois do descarte dos `disposables`.
        let _count = InstanceCountCheck(LanguageService::instance_count());
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        body(&disposables);
    }

    fn get_listener_count(language_service: &LanguageService) -> usize {
        language_service.on_did_change_listener_count_for_testing()
    }

    fn language() -> LanguageExtensionPoint {
        LanguageExtensionPoint {
            id: "testLanguageSelection".to_owned(),
            extensions: Some(vec![".testLanguageSelection".to_owned()]),
            mimetypes: Some(vec!["text/testLanguageSelection".to_owned()]),
            first_line: Some("^#!testLanguageSelection".to_owned()),
            ..LanguageExtensionPoint::default()
        }
    }

    const LANGUAGE_ID: &str = "testLanguageSelection";

    #[test]
    fn language_selection_does_not_leak_a_disposable() {
        with_suite(|_| {
            let language_service = LanguageService::new(false);
            let language_selection1 = language_service.create_by_id(Some(PLAINTEXT_LANGUAGE_ID));
            assert_eq!(language_selection1.language_id(), PLAINTEXT_LANGUAGE_ID);
            let language_selection2 = language_service.create_by_id(Some(PLAINTEXT_LANGUAGE_ID));
            let listener = language_selection2.on_did_change().listen(|_| {});
            assert_eq!(language_selection2.language_id(), PLAINTEXT_LANGUAGE_ID);
            listener.dispose();
            language_service.dispose();
        });
    }

    #[test]
    #[ignore = "Pendente: IModelService não portado (createModel/getModels) e LanguageSelection sem assinatura compartilhada do registro"]
    fn live_text_models_share_a_registry_listener_and_release_it_when_disposed() {}

    /// O corpo comum dos quatro casos do `for`.
    fn selections_update_after_an_initial_read_and_after_resubscribing(
        create: impl FnOnce(&Rc<LanguageService>) -> Rc<dyn ILanguageSelection>,
    ) {
        with_suite(|disposables| {
            let language_service = disposables.add_rc(LanguageService::new(false));
            let selection = create(&language_service);
            let subscribe = selection.on_did_change();
            let initial = selection.language_id();
            let mut counts = vec![get_listener_count(&language_service)];
            let events: Rc<RefCell<Vec<String>>> = Rc::default();
            let listener = disposables.add_rc(MutableDisposable::<dyn IDisposable>::new());
            let listen = |subscribe: &je_base::event::Event<String>| -> DisposableRef {
                let events = events.clone();
                subscribe.listen(move |value| events.borrow_mut().push(value.clone()))
            };
            listener.set(Some(listen(&subscribe)));
            counts.push(get_listener_count(&language_service));

            language_service.set_dynamic_languages(vec![language()]);
            language_service.set_dynamic_languages(vec![language(), LanguageExtensionPoint::new("unrelatedLanguageSelection")]);
            let registered = selection.language_id();
            listener.clear();
            language_service.set_dynamic_languages(vec![]);
            let unobserved = selection.language_id();
            counts.push(get_listener_count(&language_service));

            listener.set(Some(listen(&subscribe)));
            language_service.set_dynamic_languages(vec![language()]);
            language_service.set_dynamic_languages(vec![]);
            listener.clear();
            counts.push(get_listener_count(&language_service));

            assert_eq!(initial, PLAINTEXT_LANGUAGE_ID);
            assert_eq!(registered, LANGUAGE_ID);
            assert_eq!(unobserved, PLAINTEXT_LANGUAGE_ID);
            assert_eq!(*events.borrow(), vec![LANGUAGE_ID.to_owned(), LANGUAGE_ID.to_owned(), PLAINTEXT_LANGUAGE_ID.to_owned()]);
            assert_eq!(counts, vec![0, 1, 0, 0]);
        });
    }

    #[test]
    fn id_selections_update_after_an_initial_read_and_after_resubscribing() {
        selections_update_after_an_initial_read_and_after_resubscribing(|service| service.create_by_id(Some(LANGUAGE_ID)));
    }

    #[test]
    fn mime_type_selections_update_after_an_initial_read_and_after_resubscribing() {
        selections_update_after_an_initial_read_and_after_resubscribing(|service| {
            service.create_by_mime_type(Some("text/testLanguageSelection"))
        });
    }

    #[test]
    fn file_path_selections_update_after_an_initial_read_and_after_resubscribing() {
        selections_update_after_an_initial_read_and_after_resubscribing(|service| {
            service.create_by_filepath_or_first_line(Some(&URI::parse("test:file.testLanguageSelection", false)), None)
        });
    }

    #[test]
    fn first_line_selections_update_after_an_initial_read_and_after_resubscribing() {
        selections_update_after_an_initial_read_and_after_resubscribing(|service| {
            service.create_by_filepath_or_first_line(
                Some(&URI::parse("test:file", false)),
                Some(&JsString::from("#!testLanguageSelection")),
            )
        });
    }

    #[test]
    fn language_notifications_tolerate_removing_and_adding_observed_selections() {
        with_suite(|disposables| {
            let language_service = disposables.add_rc(LanguageService::new(false));
            let first = language_service.create_by_id(Some(LANGUAGE_ID));
            let second: Rc<dyn ILanguageSelection> = language_service.create_by_id(Some(LANGUAGE_ID));
            let listener = disposables.add_rc(MutableDisposable::<dyn IDisposable>::new());
            let events: Rc<RefCell<Vec<String>>> = Rc::default();

            let observe_second = {
                let (second, events) = (second.clone(), events.clone());
                move || -> DisposableRef {
                    let events = events.clone();
                    second.on_did_change().listen(move |value| events.borrow_mut().push(format!("second:{value}")))
                }
            };

            let first_listener = {
                let (listener, events, observe_second) = (listener.clone(), events.clone(), observe_second.clone());
                disposables.add(first.on_did_change().listen(move |value| {
                    events.borrow_mut().push(format!("first:{value}"));
                    listener.clear();
                    listener.set(Some(observe_second()));
                }))
            };
            listener.set(Some(observe_second()));

            language_service.set_dynamic_languages(vec![language()]);
            first_listener.dispose();
            language_service.set_dynamic_languages(vec![]);
            listener.clear();

            assert_eq!(
                *events.borrow(),
                vec![format!("first:{LANGUAGE_ID}"), format!("second:{PLAINTEXT_LANGUAGE_ID}")]
            );
            assert_eq!(get_listener_count(&language_service), 0);
        });
    }
}
