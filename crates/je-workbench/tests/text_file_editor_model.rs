//! Porte de `src/vs/workbench/services/textfile/test/browser/textFileEditorModel.test.ts` (suíte `Files - TextFileEditorModel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso (`LeakGuard`, o `disposables` do original é o store dele) e o `setup` ([`setup`]) logo depois. O `teardown`
//!   (`models.dispose()` e `disposables.clear()`) é o primeiro descartável registrado no store (descarta os modelos do
//!   gerenciador) seguido do descarte do restante, que roda quando o guarda sai de escopo e antes de ele conferir os
//!   vazamentos. Os casos `async` rodam em `run_with_faked_timers`; `await` é `.await.unwrap()` (uma rejeição reprova o caso).
//! - **`workbenchInstantiationService` e `TestServiceAccessor`.** Estão em `tests/support/text_file_support.rs`. O
//!   `accessor.workingCopyService` é o `TestWorkingCopyService` dali (com o `testUnregisterWorkingCopy`) e o
//!   `TestReadonlyTextFileEditorModel` é [`TextFileTestAccessor::create_readonly_model`].
//! - **`instantiationService.createInstance(TextFileEditorModel, resource, 'utf8', undefined)`** é
//!   [`TextFileTestAccessor::create_model`].
//! - **`toResource.call(this, path)`.** `URI.file(join('/', btoa(this.test.fullTitle()), path))`; o título do caso (a suíte
//!   mais o título do caso) é o argumento de [`to_resource`].
//! - **`getLastResolvedFileStat(model)`** é `TextFileEditorModel::last_resolved_file_stat`; `Date.now()` é
//!   `je_base::runtime::date_now()`.
//! - **`model.onDidSave`** é o `on_did_save_stored` (o `ITextFileEditorModelSaveEvent` com o `stat`, que no porte não é
//!   opcional, então o `assert.ok(savedEvent.stat)` é a leitura do campo).
//! - **`accessor.workingCopyService.get(model) as TextFileEditorModel`.** `IWorkingCopy` não tem `as_any`: o modelo que a
//!   working copy devolve é o mesmo que o gerenciador criou ao resolver de novo, então o caso confere que o serviço devolve
//!   uma working copy, que ela é o modelo do gerenciador (`files.get`) e usa este.
//! - **`isTextFileEditorModel`** é a conferência estrutural (`areFunctions(...)`) do original; em Rust os seis membros são
//!   garantidos por tipo, então [`is_text_file_editor_model`] só os nomeia como itens do contrato.
//! - **`assertThrowsAsync(() => model.setEncoding(...))`** não é aguardado no original; o porte aguarda a promessa e exige o
//!   erro (que é a intenção do `assertThrowsAsync`).
//! - **`assert.fail()` dentro de ouvintes** (`Resolve does not trigger save`) vira um contador conferido no fim do caso.
//! - **`try/finally` do `writeShouldThrowError`.** O valor é desfeito em seguida, sem `finally` (uma falha de asserção já
//!   reprova o caso, e o `teardown` descarta o serviço).
//! - **Casos sem `async` real** (`isTextFileEditorModel`) não usam `run_with_faked_timers`.
//! - **Promessas do `testSaveFromSaveParticipant`.** `assert.strictEqual(savePromise, newSavePromise)` é `Promise::ptr_eq`; a
//!   asserção que falha dentro do participante é um erro devolvido por ele (no original ela lança e o participante de salvar
//!   a engole e registra), e `savePromise` ainda não atribuído (a zona morta temporal do `const`) é o mesmo erro.
//! - **`createFileEditorInput`** é [`TextFileTestAccessor::create_file_editor_input`], e `await input.resolve()` é o
//!   `FileEditorInput::resolve_file` (o `as TextFileEditorModel` é o `downcast_ref` por `IEditorModel::as_any`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{run_with_real_timers, LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{IDisposable, to_disposable};
use je_base::path::PathModule;
use je_base::promise::Promise;
use je_base::runtime::date_now;
use je_base::uri::URI;
use je_editor::language_service::ILanguageService;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::services_model::IModelService;
use je_editor::text_model::create_text_buffer_factory;
use je_platform::files::files::{FileOperationError, FileOperationResult, FileStatWithMetadata};
use je_platform::resources::{host_environment, uri_file};
use je_workbench::common::editor::editor_model::IEditorModel;
use je_workbench::common::editor::{RevertOptions, SaveOptions, SaveReason, with_save_source_registry};
use je_workbench::services::textfile::collaborators::{IProgress, ProgressStep};
use je_workbench::services::textfile::encoding::UTF16BE;
use je_workbench::services::textfile::text_file_editor_model::TextFileEditorModel;
use je_workbench::services::textfile::text_file_editor_model_manager::TextFileEditorModelManager;
use je_workbench::services::textfile::textfiles::{
    EncodingMode, ITextFileEditorModel, ITextFileEditorModelManager, ITextFileSaveParticipant, ITextFileService, TextBufferFactoryRef, TextFileEditorModelState, TextFileResolveOptions, TextFileSaveAsOptions,
    TextFileSaveOptions, TextFileSaveParticipantContext, snapshot_to_string,
};
use je_workbench::services::working_copy::stored_file_working_copy::StoredFileWorkingCopySaveEvent;
use je_workbench::services::working_copy::working_copy::{IWorkingCopy, WorkingCopyIdentifier};
use je_workbench::services::working_copy::working_copy_service::IWorkingCopyService;
use support::text_file_support::TextFileTestAccessor;

//#region Utilitários dos casos

/// O prefixo do `this.test.fullTitle()` do mocha: o título da suíte.
const SUITE_TITLE: &str = "Files - TextFileEditorModel";

/// O `setup`: `workbenchInstantiationService(undefined, disposables)`, o `TestServiceAccessor`, o gerenciador e o conteúdo
/// do serviço de arquivos como descartáveis. O primeiro descartável registrado é o `teardown`, que descarta os modelos do
/// gerenciador antes do restante.
fn setup(disposables: &LeakGuard) -> TextFileTestAccessor {
    let manager_slot: Rc<RefCell<Option<Rc<TextFileEditorModelManager>>>> = Rc::default();
    let slot = manager_slot.clone();
    disposables.add(to_disposable(move || {
        let manager = slot.borrow_mut().take();
        if let Some(manager) = manager {
            for text_file_editor_model in manager.models() {
                text_file_editor_model.dispose();
            }
        }
    }));

    let accessor = TextFileTestAccessor::new(&disposables.store());

    let files = accessor.files();
    *manager_slot.borrow_mut() = Some(files.clone());
    disposables.add_rc(files);

    let file_service = accessor.file_service.clone();
    let content = file_service.get_content();
    disposables.add(to_disposable(move || file_service.set_content(&content)));

    accessor
}

/// `toResource.call(this, path)`: `URI.file(join('/', btoa(this.test.fullTitle()), path))`.
fn to_resource(test_title: &str, path: &str) -> URI {
    let full_title = format!("{SUITE_TITLE} {test_title}");
    let env = host_environment();
    uri_file(&PathModule::native(env.as_ref()).join(&["/", &encode_base64(&VSBuffer::from_string(&full_title), true, false), path]))
}

/// `resource.toString()`.
fn uri_string(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `createTextBufferFactory(text)`.
fn text_buffer_factory(text: &str) -> TextBufferFactoryRef {
    let units: Vec<u16> = text.encode_utf16().collect();
    TextBufferFactoryRef::new(create_text_buffer_factory(&units))
}

/// `model.updateTextEditorModel(createTextBufferFactory(text))`.
fn update_model(model: &TextFileEditorModel, text: &str) {
    model.update_text_editor_model(Some(&text_buffer_factory(text)), None, None);
}

/// `{ contents: createTextBufferFactory(text) }`.
fn with_contents(text: &str) -> Option<TextFileResolveOptions> {
    Some(TextFileResolveOptions { contents: Some(text_buffer_factory(text)), ..TextFileResolveOptions::default() })
}

/// `{ force: true }` das opções de salvar.
fn force_save() -> Option<SaveOptions> {
    Some(SaveOptions { force: Some(true), ..SaveOptions::default() })
}

/// `{ soft: true }` das opções de reverter.
fn soft_revert() -> Option<RevertOptions> {
    Some(RevertOptions { soft: Some(true), ..RevertOptions::default() })
}

/// `model.textEditorModel.getValue()`.
fn editor_value(model: &dyn ITextFileEditorModel) -> String {
    model.text_editor_model().expect("o modelo de texto existe").get_value().to_string_lossy()
}

/// `snapshotToString(model.createSnapshot()!)`.
fn snapshot_text(model: &dyn ITextFileEditorModel) -> String {
    let mut snapshot = model.create_snapshot().expect("o modelo tem snapshot");
    snapshot_to_string(&mut *snapshot)
}

/// `getLastModifiedTime(model)`: o `mtime` do último stat resolvido, ou -1.
fn get_last_modified_time(model: &TextFileEditorModel) -> i64 {
    model.last_resolved_file_stat().map_or(-1, |stat| stat.mtime())
}

/// `a === b` para os modelos (o endereço do objeto, sem o `vtable`).
fn same_model<A: ?Sized, B: ?Sized>(a: &Rc<A>, b: &Rc<B>) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

/// `accessor.workingCopyService.isDirty(model.resource, model.typeId)`.
fn working_copy_is_dirty(accessor: &TextFileTestAccessor, model: &dyn IWorkingCopy) -> bool {
    accessor.working_copy_service.is_dirty(&model.resource(), Some(&model.working_copy_type_id()))
}

/// `accessor.fileService.writeShouldThrowError = error`.
fn set_write_error(accessor: &TextFileTestAccessor, error: Option<Error>) {
    *accessor.file_service.write_should_throw_error.borrow_mut() = error;
}

/// `new Error('failed to write')`.
fn failed_to_write() -> Error {
    Error::new("failed to write")
}

/// Conta os disparos de `event`.
fn count<T: 'static>(disposables: &LeakGuard, event: Event<T>) -> Rc<Cell<usize>> {
    let counter = Rc::new(Cell::new(0));
    let listener_counter = counter.clone();
    disposables.add(event.listen(move |_| listener_counter.set(listener_counter.get() + 1)));

    counter
}

/// `let flag = false; disposables.add(event(() => flag = true))`.
fn flag_on<T: 'static>(disposables: &LeakGuard, event: Event<T>) -> Rc<Cell<bool>> {
    let flag = Rc::new(Cell::new(false));
    let listener_flag = flag.clone();
    disposables.add(event.listen(move |_| listener_flag.set(true)));

    flag
}

/// `disposables.add(accessor.workingCopyService.onDidChangeDirty(e => { if (e.resource.toString() === resource.toString()) flag = true; }))`.
fn working_copy_dirty_event_of(disposables: &LeakGuard, accessor: &TextFileTestAccessor, resource: &URI) -> Rc<Cell<bool>> {
    let flag = Rc::new(Cell::new(false));
    let (listener_flag, resource) = (flag.clone(), uri_string(resource));
    disposables.add(accessor.working_copy_service.on_did_change_dirty().listen(move |working_copy| {
        if uri_string(&working_copy.resource()) == resource {
            listener_flag.set(true);
        }
    }));

    flag
}

/// O captador do último `ITextFileEditorModelSaveEvent` de `model.onDidSave(e => savedEvent = e)`.
fn last_save_event_of(disposables: &LeakGuard, model: &TextFileEditorModel) -> Rc<RefCell<Option<StoredFileWorkingCopySaveEvent>>> {
    let saved_event: Rc<RefCell<Option<StoredFileWorkingCopySaveEvent>>> = Rc::default();
    let slot = saved_event.clone();
    disposables.add(model.on_did_save_stored().listen(move |event| *slot.borrow_mut() = Some(event.clone())));

    saved_event
}

/// `isTextFileEditorModel(model)`: a conferência `areFunctions(setEncoding, getEncoding, save, revert, isDirty,
/// getLanguageId)` é a existência dos seis membros no contrato.
fn is_text_file_editor_model(_model: &dyn ITextFileEditorModel) -> bool {
    let _ = <dyn ITextFileEditorModel as ITextFileEditorModel>::set_encoding;
    let _ = <dyn ITextFileEditorModel as ITextFileEditorModel>::get_encoding;
    let _ = <dyn ITextFileEditorModel as IWorkingCopy>::save;
    let _ = <dyn ITextFileEditorModel as IWorkingCopy>::revert;
    let _ = <dyn ITextFileEditorModel as IWorkingCopy>::is_dirty;
    let _ = <dyn ITextFileEditorModel as ITextFileEditorModel>::get_language_id;

    true
}

/// O objeto literal `{ participate: ... }` que os casos passam ao `addSaveParticipant`.
struct FnSaveParticipant<F>(F);

impl<F> ITextFileSaveParticipant for FnSaveParticipant<F>
where
    F: Fn(Rc<dyn ITextFileEditorModel>, TextFileSaveParticipantContext, CancellationToken) -> Promise<()>,
{
    fn participate(
        &self,
        model: Rc<dyn ITextFileEditorModel>,
        context: TextFileSaveParticipantContext,
        _progress: Rc<dyn IProgress<ProgressStep>>,
        token: CancellationToken,
    ) -> Promise<()> {
        (self.0)(model, context, token)
    }
}

fn participant_of(participate: impl Fn(Rc<dyn ITextFileEditorModel>, TextFileSaveParticipantContext, CancellationToken) -> Promise<()> + 'static) -> Rc<dyn ITextFileSaveParticipant> {
    Rc::new(FnSaveParticipant(participate))
}

/// `(model as TextFileEditorModel)`.
fn as_text_file_editor_model(model: &Rc<dyn ITextFileEditorModel>) -> &TextFileEditorModel {
    model.as_any().downcast_ref::<TextFileEditorModel>().expect("o modelo é um TextFileEditorModel")
}

/// `(model as TextFileEditorModel)` do modelo que `FileEditorInput.resolve` devolve.
fn as_resolved_text_file_editor_model(model: &Rc<dyn IEditorModel>) -> &TextFileEditorModel {
    model.as_any().downcast_ref::<TextFileEditorModel>().expect("o modelo é um TextFileEditorModel")
}

/// `disposables.add(model)` do modelo que `FileEditorInput.resolve` devolve.
fn track_editor_model(disposables: &LeakGuard, model: &Rc<dyn IEditorModel>) {
    let model = model.clone();
    disposables.add(to_disposable(move || model.dispose()));
}

/// `testSaveFromSaveParticipant(model, async, modelChange, force)`.
async fn test_save_from_save_participant(disposables: &LeakGuard, accessor: &TextFileTestAccessor, model: &Rc<TextFileEditorModel>, async_: bool, model_change: bool, force: bool) {
    let save_promise: Rc<RefCell<Option<Promise<bool>>>> = Rc::default();

    let (weak_model, slot) = (Rc::downgrade(model), save_promise.clone());
    disposables.add(accessor.files().add_save_participant(participant_of(move |_model, _context, _token| {
        let (weak_model, slot) = (weak_model.clone(), slot.clone());
        async_fn(async move {
            if async_ {
                timeout(10).await?;
            }

            let model = weak_model.upgrade().expect("o modelo está vivo");
            let save_options = || if force { force_save() } else { None };
            let outer = slot.borrow().clone();

            if model_change {
                update_model(&model, "bar");

                let new_save_promise = model.save(save_options());

                // assert that this is not the same promise as the outer one
                match &outer {
                    Some(outer) if !outer.ptr_eq(&new_save_promise) => {}
                    _ => return Err(Error::new("savePromise should not be the same as newSavePromise")),
                }

                new_save_promise.await?;
            } else {
                let new_save_promise = model.save(save_options());

                // assert that this is the same promise as the outer one
                let Some(outer) = outer.filter(|outer| outer.ptr_eq(&new_save_promise)) else {
                    return Err(Error::new("savePromise should be the same as newSavePromise"));
                };

                outer.await?;
            }

            Ok(())
        })
    })));

    model.resolve().await.unwrap();
    update_model(model, "foo");

    let save = model.save(if force { force_save() } else { None });
    *save_promise.borrow_mut() = Some(save.clone());
    save.await.unwrap();
}

//#endregion

#[test]
fn basic_events() {
    const TITLE: &str = "basic events";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));
        accessor.working_copy_service.test_unregister_working_copy(&*model); // causes issues with subsequent resolves otherwise

        let on_did_resolve_counter = count(&disposables, model.on_did_resolve());

        model.resolve().await.unwrap();

        assert_eq!(on_did_resolve_counter.get(), 1);

        let on_did_change_content_counter = count(&disposables, model.on_did_change_content());
        let on_did_change_dirty_counter = count(&disposables, model.on_did_change_dirty());

        update_model(&model, "bar");

        assert_eq!(on_did_change_content_counter.get(), 1);
        assert_eq!(on_did_change_dirty_counter.get(), 1);

        update_model(&model, "foo");

        assert_eq!(on_did_change_content_counter.get(), 2);
        assert_eq!(on_did_change_dirty_counter.get(), 1);

        model.revert(None).await.unwrap();

        assert_eq!(on_did_change_dirty_counter.get(), 2);
    });
}

#[test]
fn is_text_file_editor_model_case() {
    const TITLE: &str = "isTextFileEditorModel";
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let accessor = setup(&disposables);

    let model = accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None);

    assert!(is_text_file_editor_model(&*model));

    model.dispose();
}

#[test]
fn save() {
    const TITLE: &str = "save";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None);

        model.resolve().await.unwrap();

        assert_eq!(accessor.working_copy_service.dirty_count(), 0);

        let saved_event = last_save_event_of(&disposables, &model);

        model.save(None).await.unwrap();
        assert!(saved_event.borrow().is_none());

        update_model(&model, "bar");
        assert!(get_last_modified_time(&model) <= date_now() as i64);
        assert!(model.has_state(TextFileEditorModelState::Dirty));
        assert!(model.is_modified());

        assert_eq!(accessor.working_copy_service.dirty_count(), 1);
        assert!(working_copy_is_dirty(&accessor, &*model));

        let working_copy_event = working_copy_dirty_event_of(&disposables, &accessor, &model.resource());

        let source = with_save_source_registry(|registry| registry.register_source("testSource", "Hello Save"));
        let pending_save = model.save(Some(SaveOptions { reason: Some(SaveReason::Auto), source: Some(source.clone()), ..SaveOptions::default() }));
        assert!(model.has_state(TextFileEditorModelState::PendingSave));

        let joined_state = model.join_state(TextFileEditorModelState::PendingSave);
        pending_save.await.unwrap();
        joined_state.await.unwrap();

        assert!(model.has_state(TextFileEditorModelState::Saved));
        assert!(!model.is_dirty());
        assert!(!model.is_modified());
        let event = saved_event.borrow().clone().expect("o evento de salvar disparou");
        let _stat = &event.stat;
        assert_eq!(event.event.reason, Some(SaveReason::Auto));
        assert_eq!(event.event.source, Some(source));
        assert!(working_copy_event.get());

        assert_eq!(accessor.working_copy_service.dirty_count(), 0);
        assert!(!working_copy_is_dirty(&accessor, &*model));

        *saved_event.borrow_mut() = None;

        model.save(force_save()).await.unwrap();
        assert!(saved_event.borrow().is_some());

        model.dispose();
        assert!(accessor.model_service.get_model(&model.resource()).is_none());
    });
}

#[test]
fn save_touching_also_emits_saved_event() {
    const TITLE: &str = "save - touching also emits saved event";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None);

        model.resolve().await.unwrap();

        let saved_event = flag_on(&disposables, model.on_did_save_stored());

        let working_copy_event = working_copy_dirty_event_of(&disposables, &accessor, &model.resource());

        model.save(force_save()).await.unwrap();

        assert!(saved_event.get());
        assert!(!working_copy_event.get());

        model.dispose();
        assert!(accessor.model_service.get_model(&model.resource()).is_none());
    });
}

#[test]
fn save_touching_with_error_turns_model_dirty() {
    const TITLE: &str = "save - touching with error turns model dirty";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None);

        model.resolve().await.unwrap();

        let save_error_event = flag_on(&disposables, model.on_did_save_error());

        let saved_event = flag_on(&disposables, model.on_did_save_stored());

        set_write_error(&accessor, Some(failed_to_write()));
        model.save(force_save()).await.unwrap();

        assert!(model.has_state(TextFileEditorModelState::Error));
        assert!(model.is_dirty());
        assert!(model.is_modified());
        assert!(save_error_event.get());

        assert_eq!(accessor.working_copy_service.dirty_count(), 1);
        assert!(working_copy_is_dirty(&accessor, &*model));
        set_write_error(&accessor, None);

        model.save(force_save()).await.unwrap();

        assert!(saved_event.get());
        assert!(!model.is_dirty());

        model.dispose();
        assert!(accessor.model_service.get_model(&model.resource()).is_none());
    });
}

#[test]
fn save_returns_false_when_save_fails() {
    const TITLE: &str = "save - returns false when save fails";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None);

        model.resolve().await.unwrap();

        set_write_error(&accessor, Some(failed_to_write()));
        let res = model.save(force_save()).await.unwrap();
        assert!(!res);
        set_write_error(&accessor, None);

        let res = model.save(force_save()).await.unwrap();
        assert!(res);

        model.dispose();
        assert!(accessor.model_service.get_model(&model.resource()).is_none());
    });
}

#[test]
fn save_error_generic() {
    const TITLE: &str = "save error (generic)";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None);

        model.resolve().await.unwrap();

        update_model(&model, "bar");

        let save_error_event = flag_on(&disposables, model.on_did_save_error());

        set_write_error(&accessor, Some(failed_to_write()));
        let pending_save = model.save(None);
        assert!(model.has_state(TextFileEditorModelState::PendingSave));

        pending_save.await.unwrap();

        assert!(model.has_state(TextFileEditorModelState::Error));
        assert!(model.is_dirty());
        assert!(model.is_modified());
        assert!(save_error_event.get());

        assert_eq!(accessor.working_copy_service.dirty_count(), 1);
        assert!(working_copy_is_dirty(&accessor, &*model));

        model.dispose();
        set_write_error(&accessor, None);
    });
}

#[test]
fn save_error_conflict() {
    const TITLE: &str = "save error (conflict)";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None);

        model.resolve().await.unwrap();

        update_model(&model, "bar");

        let save_error_event = flag_on(&disposables, model.on_did_save_error());

        set_write_error(&accessor, Some(FileOperationError::new("save conflict", FileOperationResult::FileModifiedSince).into_error()));
        let pending_save = model.save(None);
        assert!(model.has_state(TextFileEditorModelState::PendingSave));

        pending_save.await.unwrap();

        assert!(model.has_state(TextFileEditorModelState::Conflict));
        assert!(model.is_dirty());
        assert!(model.is_modified());
        assert!(save_error_event.get());

        assert_eq!(accessor.working_copy_service.dirty_count(), 1);
        assert!(working_copy_is_dirty(&accessor, &*model));

        model.dispose();
        set_write_error(&accessor, None);
    });
}

#[test]
fn set_encoding_encode() {
    const TITLE: &str = "setEncoding - encode";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        let encoding_event = flag_on(&disposables, model.on_did_change_encoding());

        model.set_encoding("utf8", EncodingMode::Encode).await.unwrap(); // no-op
        assert_eq!(get_last_modified_time(&model), -1);

        assert!(!encoding_event.get());

        model.set_encoding("utf16", EncodingMode::Encode).await.unwrap();

        assert!(encoding_event.get());

        assert!(get_last_modified_time(&model) <= date_now() as i64); // indicates model was saved due to encoding change
    });
}

#[test]
fn set_encoding_decode() {
    const TITLE: &str = "setEncoding - decode";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));
        accessor.working_copy_service.test_unregister_working_copy(&*model); // causes issues with subsequent resolves otherwise

        model.set_encoding("utf16", EncodingMode::Decode).await.unwrap();

        // we have to get the model again from working copy service
        // because `setEncoding` will resolve it again through the
        // text file service which is outside our scope
        let working_copy = accessor.working_copy_service.get(&WorkingCopyIdentifier::of(&*model)).expect("a working copy foi registrada de novo");
        let model = accessor.files().get(&model.resource()).expect("o gerenciador tem o modelo resolvido de novo");
        assert!(same_model(&working_copy, &model));

        assert!(model.is_resolved()); // model got resolved due to decoding
    });
}

#[test]
fn set_encoding_decode_dirty_file_throws() {
    const TITLE: &str = "setEncoding - decode dirty file throws";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));
        accessor.working_copy_service.test_unregister_working_copy(&*model); // causes issues with subsequent resolves otherwise

        model.resolve().await.unwrap();

        update_model(&model, "bar");
        assert!(model.is_dirty());

        assert!(model.set_encoding("utf16", EncodingMode::Decode).await.is_err());
    });
}

#[test]
fn encoding_updates_with_language_based_configuration() {
    const TITLE: &str = "encoding updates with language based configuration";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let language_id = "text-file-model-test";
        disposables.add(accessor.language_service.register_language(LanguageExtensionPoint::new(language_id)));

        accessor.test_configuration_service.set_override_identifiers("files.encoding", vec![JsString::from(language_id)]);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));
        accessor.working_copy_service.test_unregister_working_copy(&*model); // causes issues with subsequent resolves otherwise

        model.resolve().await.unwrap();

        let (deferred_promise, resolver) = Promise::<()>::new();

        // We use this listener as a way to figure out that the working
        // copy was resolved again as part of the language change
        let model_resource = uri_string(&model.resource());
        disposables.add(accessor.working_copy_service.on_did_register().listen(move |working_copy| {
            if uri_string(&working_copy.resource()) == model_resource {
                resolver.resolve(());
            }
        }));

        drop(accessor.test_configuration_service.set_user_configuration("files.encoding", Dynamic::from(UTF16BE), None));

        je_workbench::services::textfile::textfiles::ILanguageSupport::set_language_id(&*model, language_id, None);

        deferred_promise.await.unwrap(); // this asserts that the model was reloaded due to the language change
    });
}

#[test]
fn create_with_language() {
    const TITLE: &str = "create with language";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let language_id = "text-file-model-test";
        disposables.add(accessor.language_service.register_language(LanguageExtensionPoint::new(language_id)));

        let model = accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), Some(language_id));

        model.resolve().await.unwrap();

        assert_eq!(model.text_editor_model().expect("o modelo de texto existe").get_language_id(), language_id);

        model.dispose();
        assert!(accessor.model_service.get_model(&model.resource()).is_none());
    });
}

#[test]
fn disposes_when_underlying_model_is_destroyed() {
    const TITLE: &str = "disposes when underlying model is destroyed";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None);

        model.resolve().await.unwrap();

        model.text_editor_model().expect("o modelo de texto existe").dispose();
        assert!(model.is_disposed());
    });
}

#[test]
fn resolve_does_not_trigger_save() {
    const TITLE: &str = "Resolve does not trigger save";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = accessor.create_model(&to_resource(TITLE, "/path/index.txt"), Some("utf8"), None);
        assert!(model.has_state(TextFileEditorModelState::Saved));

        let save_counter = count(&disposables, model.on_did_save_stored());
        let dirty_counter = count(&disposables, model.on_did_change_dirty());

        model.resolve().await.unwrap();
        assert!(model.is_resolved());
        model.dispose();
        assert!(accessor.model_service.get_model(&model.resource()).is_none());

        // `assert.fail()` dos ouvintes do original
        assert_eq!(save_counter.get(), 0);
        assert_eq!(dirty_counter.get(), 0);
    });
}

#[test]
fn resolve_returns_dirty_model_as_long_as_model_is_dirty() {
    const TITLE: &str = "Resolve returns dirty model as long as model is dirty";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        model.resolve().await.unwrap();
        update_model(&model, "foo");
        assert!(model.is_dirty());
        assert!(model.has_state(TextFileEditorModelState::Dirty));

        model.resolve().await.unwrap();
        assert!(model.is_dirty());
    });
}

#[test]
fn resolve_with_contents() {
    const TITLE: &str = "Resolve with contents";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None);

        model.resolve_text_file(with_contents("Hello World")).await.unwrap();

        assert_eq!(editor_value(&*model), "Hello World");
        assert!(model.is_dirty());

        model.resolve_text_file(with_contents("Hello Changes")).await.unwrap();

        assert_eq!(editor_value(&*model), "Hello Changes");
        assert!(model.is_dirty());

        // verify that we do not mark the model as saved when undoing once because
        // we never really had a saved state
        model.text_editor_model().expect("o modelo de texto existe").undo();
        assert!(model.is_dirty());

        model.dispose();
        assert!(accessor.model_service.get_model(&model.resource()).is_none());
    });
}

#[test]
fn revert() {
    const TITLE: &str = "Revert";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));
        let resource = model.resource();

        let event_counter = count(&disposables, model.on_did_revert());

        let working_copy_event = working_copy_dirty_event_of(&disposables, &accessor, &resource);

        model.resolve().await.unwrap();
        update_model(&model, "foo");
        assert!(model.is_dirty());
        assert!(model.is_modified());

        assert_eq!(accessor.working_copy_service.dirty_count(), 1);
        assert!(working_copy_is_dirty(&accessor, &*model));

        accessor.working_copy_service.test_unregister_working_copy(&*model); // causes issues with subsequent resolves otherwise

        model.revert(None).await.unwrap();

        // we have to get the model again from working copy service
        // because `setEncoding` will resolve it again through the
        // text file service which is outside our scope
        let working_copy = accessor.working_copy_service.get(&WorkingCopyIdentifier::of(&*model)).expect("a working copy foi registrada de novo");
        let model = accessor.files().get(&resource).expect("o gerenciador tem o modelo resolvido de novo");
        assert!(same_model(&working_copy, &model));

        assert!(!model.is_dirty());
        assert!(!model.is_modified());
        assert_eq!(event_counter.get(), 1);

        assert!(working_copy_event.get());
        assert_eq!(accessor.working_copy_service.dirty_count(), 0);
        assert!(!accessor.working_copy_service.is_dirty(&model.resource(), Some(&model.working_copy_type_id())));
    });
}

#[test]
fn revert_soft() {
    const TITLE: &str = "Revert (soft)";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        let event_counter = count(&disposables, model.on_did_revert());

        let working_copy_event = working_copy_dirty_event_of(&disposables, &accessor, &model.resource());

        model.resolve().await.unwrap();
        update_model(&model, "foo");
        assert!(model.is_dirty());
        assert!(model.is_modified());

        assert_eq!(accessor.working_copy_service.dirty_count(), 1);
        assert!(working_copy_is_dirty(&accessor, &*model));

        model.revert(soft_revert()).await.unwrap();
        assert!(!model.is_dirty());
        assert!(!model.is_modified());
        assert_eq!(editor_value(&*model), "foo");
        assert_eq!(event_counter.get(), 1);

        assert!(working_copy_event.get());
        assert_eq!(accessor.working_copy_service.dirty_count(), 0);
        assert!(!working_copy_is_dirty(&accessor, &*model));
    });
}

#[test]
fn undo_to_saved_state_turns_model_non_dirty() {
    const TITLE: &str = "Undo to saved state turns model non-dirty";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));
        model.resolve().await.unwrap();
        update_model(&model, "Hello Text");
        assert!(model.is_dirty());

        model.text_editor_model().expect("o modelo de texto existe").undo();
        assert!(!model.is_dirty());
    });
}

#[test]
fn resolve_and_undo_turns_model_dirty() {
    const TITLE: &str = "Resolve and undo turns model dirty";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));
        model.resolve().await.unwrap();
        accessor.file_service.set_content("Hello Change");

        model.resolve().await.unwrap();
        model.text_editor_model().expect("o modelo de texto existe").undo();
        assert!(model.is_dirty());

        assert_eq!(accessor.working_copy_service.dirty_count(), 1);
        assert!(working_copy_is_dirty(&accessor, &*model));
    });
}

#[test]
fn update_dirty() {
    const TITLE: &str = "Update Dirty";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        model.set_dirty(true);
        assert!(!model.is_dirty()); // needs to be resolved

        model.resolve().await.unwrap();
        update_model(&model, "foo");
        assert!(model.is_dirty());

        model.revert(soft_revert()).await.unwrap();
        assert!(!model.is_dirty());

        let event_counter = count(&disposables, model.on_did_change_dirty());

        let working_copy_event = working_copy_dirty_event_of(&disposables, &accessor, &model.resource());

        model.set_dirty(true);
        assert!(model.is_dirty());
        assert_eq!(event_counter.get(), 1);
        assert!(working_copy_event.get());

        model.set_dirty(false);
        assert!(!model.is_dirty());
        assert_eq!(event_counter.get(), 2);
    });
}

#[test]
fn no_dirty_or_saving_for_readonly_models() {
    const TITLE: &str = "No Dirty or saving for readonly models";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let resource = to_resource(TITLE, "/path/index_async.txt");
        let working_copy_event = working_copy_dirty_event_of(&disposables, &accessor, &resource);

        let model = disposables.add_rc(accessor.create_readonly_model(&resource, Some("utf8"), None));

        let save_event = flag_on(&disposables, model.on_did_save_stored());

        model.resolve().await.unwrap();
        update_model(&model, "foo");
        assert!(!model.is_dirty());

        model.save(force_save()).await.unwrap();
        assert!(!save_event.get());

        model.revert(soft_revert()).await.unwrap();
        assert!(!model.is_dirty());

        assert!(!working_copy_event.get());
    });
}

#[test]
fn file_not_modified_error_is_handled_gracefully() {
    const TITLE: &str = "File not modified error is handled gracefully";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        model.resolve().await.unwrap();

        let mtime = get_last_modified_time(&model);
        accessor.text_file_service.set_read_stream_error_once(FileOperationError::new("error", FileOperationResult::FileNotModifiedSince));

        model.resolve().await.unwrap();

        assert_eq!(get_last_modified_time(&model), mtime);
    });
}

#[test]
fn stat_readonly_and_locked_can_change_when_decreased_mtime_is_ignored() {
    const TITLE: &str = "stat.readonly and stat.locked can change when decreased mtime is ignored";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        model.resolve().await.unwrap();

        let stat = model.last_resolved_file_stat().expect("o modelo tem o stat resolvido");
        let mut changed_stat = stat.clone().into_stat();
        changed_stat.mtime = Some(stat.mtime() - 1);
        changed_stat.readonly = Some(!stat.readonly());
        changed_stat.locked = Some(!stat.locked());
        accessor.text_file_service.set_read_stream_error_once(FileOperationError::not_modified_since("error", FileStatWithMetadata::new(changed_stat)));

        model.resolve().await.unwrap();

        assert_eq!(get_last_modified_time(&model), stat.mtime(), "mtime should not decrease");
        let last_stat = model.last_resolved_file_stat().expect("o modelo tem o stat resolvido");
        assert_ne!(last_stat.readonly(), stat.readonly(), "readonly should have changed despite simultaneous attempt to decrease mtime");
        assert_ne!(last_stat.locked(), stat.locked(), "locked should have changed despite simultaneous attempt to decrease mtime");
    });
}

#[test]
fn resolve_error_is_handled_gracefully_if_model_already_exists() {
    const TITLE: &str = "Resolve error is handled gracefully if model already exists";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        model.resolve().await.unwrap();
        accessor.text_file_service.set_read_stream_error_once(FileOperationError::new("error", FileOperationResult::FileNotFound));

        model.resolve().await.unwrap();
    });
}

#[test]
fn save_and_is_dirty_proper_with_check_for_mtimes() {
    const TITLE: &str = "save() and isDirty() - proper with check for mtimes";
    // O original roda este caso no relógio real (sem `runWithFakedTimers`): o `mtime` é o `Date.now()` do sistema.
    run_with_real_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let resource1 = to_resource(TITLE, "/path/index_async2.txt");
        let resource2 = to_resource(TITLE, "/path/index_async.txt");

        let input1 = disposables.add_rc(accessor.create_file_editor_input(&resource1));
        let input2 = disposables.add_rc(accessor.create_file_editor_input(&resource2));

        let resolved1 = input1.resolve_file(None).await.unwrap();
        track_editor_model(&disposables, &resolved1);
        let resolved2 = input2.resolve_file(None).await.unwrap();
        track_editor_model(&disposables, &resolved2);
        let (model1, model2) = (as_resolved_text_file_editor_model(&resolved1), as_resolved_text_file_editor_model(&resolved2));

        update_model(model1, "foo");

        let m1_mtime = model1.last_resolved_file_stat().expect("o modelo tem o stat resolvido").mtime();
        let m2_mtime = model2.last_resolved_file_stat().expect("o modelo tem o stat resolvido").mtime();
        assert!(m1_mtime > 0);
        assert!(m2_mtime > 0);

        assert!(accessor.text_file_service.is_dirty(&resource1));
        assert!(!accessor.text_file_service.is_dirty(&resource2));

        update_model(model2, "foo");
        assert!(accessor.text_file_service.is_dirty(&resource2));

        timeout(10).await.unwrap();
        accessor.text_file_service.save(&resource2, None).await.unwrap();
        accessor.text_file_service.save(&resource1, None).await.unwrap();
        assert!(!accessor.text_file_service.is_dirty(&resource2));
        assert!(!accessor.text_file_service.is_dirty(&resource1));

        // on desktop we want to assert this condition more strictly (`isWeb` is false), the timeouts are respected
        assert!(model1.last_resolved_file_stat().expect("o modelo tem o stat resolvido").mtime() > m1_mtime);
        assert!(model2.last_resolved_file_stat().expect("o modelo tem o stat resolvido").mtime() > m2_mtime);
    });
}

#[test]
fn save_participant() {
    const TITLE: &str = "Save Participant";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let event_counter = Rc::new(Cell::new(0));
        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        let (weak_model, counter) = (Rc::downgrade(&model), event_counter.clone());
        disposables.add(model.on_did_save_stored().listen(move |_| {
            let model = weak_model.upgrade().expect("o modelo está vivo");
            assert_eq!(snapshot_text(&*model), if counter.get() == 1 { "bar" } else { "foobar" });
            assert!(!model.is_dirty());
            counter.set(counter.get() + 1);
        }));

        let counter = event_counter.clone();
        let participant = accessor.files().add_save_participant(participant_of(move |model, _context, _token| {
            assert!(model.is_dirty());
            update_model(as_text_file_editor_model(&model), "bar");
            assert!(model.is_dirty());
            counter.set(counter.get() + 1);

            Promise::resolved(())
        }));

        model.resolve().await.unwrap();
        update_model(&model, "foo");
        assert!(model.is_dirty());

        model.save(None).await.unwrap();
        assert_eq!(event_counter.get(), 2);

        participant.dispose();
        update_model(&model, "foobar");
        assert!(model.is_dirty());

        model.save(None).await.unwrap();
        assert_eq!(event_counter.get(), 3);
    });
}

#[test]
fn save_participant_skip() {
    const TITLE: &str = "Save Participant - skip";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let event_counter = Rc::new(Cell::new(0));
        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        let counter = event_counter.clone();
        disposables.add(accessor.files().add_save_participant(participant_of(move |_model, _context, _token| {
            counter.set(counter.get() + 1);

            Promise::resolved(())
        })));

        model.resolve().await.unwrap();
        update_model(&model, "foo");

        model.save(Some(SaveOptions { skip_save_participants: Some(true), ..SaveOptions::default() })).await.unwrap();
        assert_eq!(event_counter.get(), 0);
    });
}

#[test]
fn save_participant_async_participant() {
    const TITLE: &str = "Save Participant, async participant";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let event_counter = Rc::new(Cell::new(0));
        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        let (weak_model, counter) = (Rc::downgrade(&model), event_counter.clone());
        disposables.add(model.on_did_save_stored().listen(move |_| {
            let model = weak_model.upgrade().expect("o modelo está vivo");
            assert!(!model.is_dirty());
            counter.set(counter.get() + 1);
        }));

        let counter = event_counter.clone();
        disposables.add(accessor.files().add_save_participant(participant_of(move |model, _context, _token| {
            assert!(model.is_dirty());
            update_model(as_text_file_editor_model(&model), "bar");
            assert!(model.is_dirty());
            counter.set(counter.get() + 1);

            timeout(10)
        })));

        model.resolve().await.unwrap();
        update_model(&model, "foo");

        let now = date_now();
        model.save(None).await.unwrap();
        assert_eq!(event_counter.get(), 2);
        assert!(date_now() - now >= 10);
    });
}

#[test]
fn save_participant_bad_participant() {
    const TITLE: &str = "Save Participant, bad participant";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        disposables.add(accessor.files().add_save_participant(participant_of(move |_model, _context, _token| {
            let _boom = Error::new("boom");

            Promise::resolved(())
        })));

        model.resolve().await.unwrap();
        update_model(&model, "foo");

        model.save(None).await.unwrap();
    });
}

#[test]
fn save_participant_participant_cancelled_when_saved_again() {
    const TITLE: &str = "Save Participant, participant cancelled when saved again";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        let participations: Rc<RefCell<Vec<bool>>> = Rc::default();

        let collected = participations.clone();
        disposables.add(accessor.files().add_save_participant(participant_of(move |_model, _context, token| {
            let collected = collected.clone();
            async_fn(async move {
                timeout(10).await?;

                if !token.is_cancellation_requested() {
                    collected.borrow_mut().push(true);
                }

                Ok(())
            })
        })));

        model.resolve().await.unwrap();

        update_model(&model, "foo");
        let p1 = model.save(None);

        update_model(&model, "foo 1");
        let p2 = model.save(None);

        update_model(&model, "foo 2");
        let p3 = model.save(None);

        update_model(&model, "foo 3");
        let p4 = model.save(None);

        Promise::all(vec![p1, p2, p3, p4]).await.unwrap();
        assert_eq!(participations.borrow().len(), 1);
    });
}

#[test]
fn save_participant_calling_save_from_within_is_unsupported_but_does_not_explode_sync_save_no_model_change() {
    const TITLE: &str = "Save Participant, calling save from within is unsupported but does not explode (sync save, no model change)";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        test_save_from_save_participant(&disposables, &accessor, &model, false, false, false).await;
    });
}

#[test]
fn save_participant_calling_save_from_within_is_unsupported_but_does_not_explode_async_save_no_model_change() {
    const TITLE: &str = "Save Participant, calling save from within is unsupported but does not explode (async save, no model change)";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        test_save_from_save_participant(&disposables, &accessor, &model, true, false, false).await;
    });
}

#[test]
fn save_participant_calling_save_from_within_is_unsupported_but_does_not_explode_sync_save_model_change() {
    const TITLE: &str = "Save Participant, calling save from within is unsupported but does not explode (sync save, model change)";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        test_save_from_save_participant(&disposables, &accessor, &model, false, true, false).await;
    });
}

#[test]
fn save_participant_calling_save_from_within_is_unsupported_but_does_not_explode_async_save_model_change() {
    const TITLE: &str = "Save Participant, calling save from within is unsupported but does not explode (async save, model change)";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        test_save_from_save_participant(&disposables, &accessor, &model, true, true, false).await;
    });
}

#[test]
fn save_participant_calling_save_from_within_is_unsupported_but_does_not_explode_force() {
    const TITLE: &str = "Save Participant, calling save from within is unsupported but does not explode (force)";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        test_save_from_save_participant(&disposables, &accessor, &model, false, false, true).await;
    });
}

#[test]
fn save_participant_carries_context() {
    const TITLE: &str = "Save Participant carries context";
    run_with_faked_timers(async {
        let disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let accessor = setup(&disposables);

        let model = disposables.add_rc(accessor.create_model(&to_resource(TITLE, "/path/index_async.txt"), Some("utf8"), None));

        let from = uri_file("testFrom");
        let error: Rc<RefCell<Option<String>>> = Rc::default();
        let (slot, from_string) = (error.clone(), uri_string(&from));
        disposables.add(accessor.files().add_save_participant(participant_of(move |_model, context, _token| {
            if context.reason != SaveReason::Explicit {
                *slot.borrow_mut() = Some(format!("expected reason {:?} to be {:?}", context.reason, SaveReason::Explicit));
            } else if context.saved_from.as_ref().map(uri_string).as_deref() != Some(from_string.as_str()) {
                *slot.borrow_mut() = Some(format!("expected savedFrom {:?} to be {from_string}", context.saved_from));
            }

            Promise::resolved(())
        })));

        model.resolve().await.unwrap();
        update_model(&model, "foo");

        model
            .save_text_file(TextFileSaveAsOptions {
                options: TextFileSaveOptions { base: SaveOptions { force: Some(true), ..SaveOptions::default() }, ..TextFileSaveOptions::default() },
                from: Some(from.clone()),
                ..TextFileSaveAsOptions::default()
            })
            .await
            .unwrap();

        if let Some(e) = error.borrow().as_ref() {
            panic!("{e}");
        }
    });
}
