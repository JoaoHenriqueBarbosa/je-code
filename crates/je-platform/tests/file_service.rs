//! Porte de `src/vs/platform/files/test/browser/fileService.test.ts` (suíte `File Service`), caso a caso e na mesma ordem.
//!
//! Divergências: (1) o `throw` síncrono de um provider não existe no trait Rust (os métodos devolvem
//! `Promise` ou stream), então "lançar de forma síncrona" vira uma promessa já rejeitada (ou um stream já
//! com erro); (2) `isFileSystemWatcher` é conferido pelo tipo: `watch` devolve um `DisposableRef` e
//! `watch_correlated` devolve o `FileSystemWatcher`; (3) os `for` do original que geram vários testes viram
//! um `#[test]` por combinação; (4) as subclasses anônimas viram [`OverridableProvider`].

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::files::{NullFileSystemProvider, Overrides, OverridableProvider, async_test};
use je_base::async_::{async_fn, timeout};
use je_base::buffer::{VSBuffer, buffer_to_readable, buffer_to_stream, new_writeable_buffer_stream};
use je_base::cancellation::{CancellationToken, CancellationTokenSource, ICancellationToken};
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::promise::{DeferredPromise, Promise};
use je_base::resources::ExtUri;
use je_base::path::ProcessEnvironment;
use je_base::runtime::date_now;
use je_base::stream::{WriteableStream, WriteableStreamOptions, consume_stream, new_writeable_stream, Reducer};
use je_base::uri::URI;
use je_platform::files::file_service::{FileContentSource, FileService};
use je_platform::files::files::*;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::{LogService, NullLogService};

type Prov = Rc<dyn FileSystemProvider>;

fn now() -> i64 {
    date_now() as i64
}

fn file_stat(size: u64) -> Stat {
    Stat { file_type: FileType::FILE, mtime: now(), ctime: now(), size, permissions: None }
}

fn new_service(d: &Rc<DisposableStore>) -> Rc<FileService> {
    d.add_rc(FileService::new(NullLogService::shared()))
}

fn register(d: &Rc<DisposableStore>, service: &Rc<FileService>, scheme: &str, provider: Prov) {
    d.add(service.register_provider(scheme, provider).unwrap());
}

fn uri(text: &str) -> URI {
    URI::parse(text, false)
}

fn concat() -> Reducer<VSBuffer> {
    Rc::new(|chunks| VSBuffer::concat(&chunks, None))
}

async fn text_of(promise: Promise<VSBuffer>) -> String {
    promise.await.unwrap().to_string()
}

#[derive(Default)]
struct TraceLog {
    traces: RefCell<Vec<String>>,
}

impl LogService for TraceLog {
    fn trace(&self, message: &str) {
        self.traces.borrow_mut().push(message.to_string());
    }
    fn debug(&self, _message: &str) {}
    fn info(&self, _message: &str) {}
    fn warn(&self, _message: &str) {}
    fn error(&self, _message: &str) {}
}

#[test]
fn resolve_all_does_not_trace_missing_resources() {
    async_test(|d| async move {
        let log = Rc::new(TraceLog::default());
        let service = d.add_rc(FileService::new(log.clone()));
        let provider = OverridableProvider::new(
            NullFileSystemProvider::new(),
            Overrides {
                stat: Some(Rc::new(|_: &Prov, resource: &URI| {
                    if resource.path().to_string_lossy() == "/missing" {
                        return Promise::rejected(create_file_system_provider_error_from_text("missing", FileSystemProviderErrorCode::FileNotFound));
                    }
                    Promise::rejected(create_file_system_provider_error_from_text("unavailable", FileSystemProviderErrorCode::Unavailable))
                })),
                ..Default::default()
            },
        );
        register(&d, &service, "test", provider);

        let result = service.resolve_all(vec![(uri("test:///missing"), None), (uri("test:///unavailable"), None)]).await.unwrap();

        assert_eq!(result, vec![FileStatResult { stat: None, success: false }, FileStatResult { stat: None, success: false }]);
        assert_eq!(*log.traces.borrow(), vec!["Unavailable (FileSystemError): unavailable".to_string()]);
    });
}

#[test]
fn provider_registration() {
    async_test(|d| async move {
        let service = new_service(&d);
        let resource = uri("test://foo/bar");
        let provider = NullFileSystemProvider::new();
        let provider_ref: Prov = provider.clone();

        assert!(!service.can_handle_resource(&resource).await.unwrap());
        assert!(!service.has_provider(&resource));
        assert!(service.get_provider(resource.scheme().to_string_lossy().as_str()).is_none());

        let registrations = Rc::new(RefCell::new(Vec::new()));
        let sink = registrations.clone();
        d.add(service.on_did_change_file_system_provider_registrations().listen(move |e| sink.borrow_mut().push(e.clone())));

        let capability_changes = Rc::new(Cell::new(0));
        let counter = capability_changes.clone();
        d.add(service.on_did_change_file_system_provider_capabilities().listen(move |_| counter.set(counter.get() + 1)));

        let registration_disposable = Rc::new(RefCell::new(None));
        let call_count = Rc::new(Cell::new(0));
        {
            let (slot, calls, weak, provider_ref) = (registration_disposable.clone(), call_count.clone(), Rc::downgrade(&service), provider_ref.clone());
            d.add(service.on_will_activate_file_system_provider().listen(move |e| {
                calls.set(calls.get() + 1);
                if e.scheme == "test" && calls.get() == 1 {
                    let (slot, weak, provider_ref) = (slot.clone(), weak.clone(), provider_ref.clone());
                    e.join(Promise::with_executor(move |resolver| {
                        *slot.borrow_mut() = weak.upgrade().map(|s| s.register_provider("test", provider_ref).unwrap());

                        resolver.resolve(());
                    }));
                }
            }));
        }

        assert!(service.can_handle_resource(&resource).await.unwrap());
        assert!(service.has_provider(&resource));
        let got = service.get_provider("test").unwrap();
        assert!(std::ptr::addr_eq(Rc::as_ptr(&got), Rc::as_ptr(&provider_ref)));

        assert_eq!(registrations.borrow().len(), 1);
        assert_eq!(registrations.borrow()[0].scheme, "test");
        assert!(registrations.borrow()[0].added);
        assert!(registration_disposable.borrow().is_some());

        assert_eq!(capability_changes.get(), 0);

        provider.set_capabilities(FileSystemProviderCapabilities::FILE_FOLDER_COPY);
        assert_eq!(capability_changes.get(), 1);
        provider.set_capabilities(FileSystemProviderCapabilities::READONLY);
        assert_eq!(capability_changes.get(), 2);

        service.activate_provider("test").await.unwrap();
        assert_eq!(call_count.get(), 2); // activation is called again

        assert!(service.has_capability(&resource, FileSystemProviderCapabilities::READONLY));
        assert!(!service.has_capability(&resource, FileSystemProviderCapabilities::FILE_OPEN_READ_WRITE_CLOSE));

        registration_disposable.borrow().as_ref().unwrap().dispose();

        assert!(!service.can_handle_resource(&resource).await.unwrap());
        assert!(!service.has_provider(&resource));

        assert_eq!(registrations.borrow().len(), 2);
        assert_eq!(registrations.borrow()[1].scheme, "test");
        assert!(!registrations.borrow()[1].added);
    });
}

#[test]
fn watch() {
    async_test(|d| async move {
        let service = new_service(&d);

        let dispose_counter = Rc::new(Cell::new(0));
        let counter = dispose_counter.clone();
        let provider = NullFileSystemProvider::with_disposable_factory(Box::new(move || {
            let counter = counter.clone();
            to_disposable(move || counter.set(counter.get() + 1))
        }));
        register(&d, &service, "test", provider);
        service.activate_provider("test").await.unwrap();

        let resource1 = uri("test://foo/bar1");
        let watcher1_disposable = service.watch(&resource1, None);

        timeout(0).await.unwrap(); // service.watch() is async
        assert_eq!(dispose_counter.get(), 0);
        watcher1_disposable.dispose();
        assert_eq!(dispose_counter.get(), 1);

        dispose_counter.set(0);
        let resource2 = uri("test://foo/bar2");
        let w2: Vec<_> = (0..3).map(|_| service.watch(&resource2, None)).collect();

        timeout(0).await.unwrap(); // service.watch() is async
        assert_eq!(dispose_counter.get(), 0);
        w2[0].dispose();
        assert_eq!(dispose_counter.get(), 0);
        w2[1].dispose();
        assert_eq!(dispose_counter.get(), 0);
        w2[2].dispose();
        assert_eq!(dispose_counter.get(), 1);

        dispose_counter.set(0);
        let resource3 = uri("test://foo/bar3");
        let w31 = service.watch(&resource3, None);
        let w32 = service.watch(&resource3, Some(WatchOptions { recursive: true, excludes: vec![], ..Default::default() }));
        let w33 = service.watch(&resource3, Some(WatchOptions { recursive: false, excludes: vec![], includes: Some(vec![]), ..Default::default() }));

        timeout(0).await.unwrap(); // service.watch() is async
        assert_eq!(dispose_counter.get(), 0);
        w31.dispose();
        assert_eq!(dispose_counter.get(), 1);
        w32.dispose();
        assert_eq!(dispose_counter.get(), 2);
        w33.dispose();
        assert_eq!(dispose_counter.get(), 3);

        service.dispose();
    });
}

#[test]
fn watch_with_correlation() {
    async_test(|d| async move {
        let service = new_service(&d);
        let provider = NullFileSystemProvider::new();
        register(&d, &service, "test", provider.clone());
        service.activate_provider("test").await.unwrap();

        let global_events = Rc::new(RefCell::new(Vec::new()));
        let sink = global_events.clone();
        d.add(service.on_did_files_change().listen(move |e| sink.borrow_mut().push(e.clone())));

        let options = |correlation_id| WatchOptions { recursive: true, excludes: vec![], includes: Some(vec![]), correlation_id, ..Default::default() };
        // `isFileSystemWatcher(watcher0) === false`: `watch` devolve só o disposable.
        d.add(service.watch(&uri("test://watch/folder1"), Some(options(None))));
        let watcher1 = service.watch_correlated(&uri("test://watch/folder2"), options(Some(100)));
        d.add_rc(watcher1.clone());
        let watcher2 = service.watch_correlated(&uri("test://watch/folder3"), options(Some(200)));
        d.add_rc(watcher2.clone());

        let watcher1_events = Rc::new(RefCell::new(Vec::new()));
        let sink = watcher1_events.clone();
        d.add(watcher1.on_did_change().listen(move |e| sink.borrow_mut().push(e.clone())));

        let watcher2_events = Rc::new(RefCell::new(Vec::new()));
        let sink = watcher2_events.clone();
        d.add(watcher2.on_did_change().listen(move |e| sink.borrow_mut().push(e.clone())));

        let fire = |path: &str, change_type, c_id: Option<i64>| {
            provider.emit_file_change_events(vec![FileChange { c_id, ..FileChange::new(change_type, uri(path)) }]);
        };
        fire("test://watch/folder1", FileChangeType::Added, None);
        fire("test://watch/folder2", FileChangeType::Added, Some(100));
        fire("test://watch/folder2", FileChangeType::Added, Some(100));
        fire("test://watch/folder3/file", FileChangeType::Updated, Some(200));
        fire("test://watch/folder3", FileChangeType::Updated, Some(200));

        fire("test://watch/folder4", FileChangeType::Added, Some(50));
        fire("test://watch/folder4", FileChangeType::Added, Some(60));
        fire("test://watch/folder4", FileChangeType::Added, Some(70));

        assert_eq!(global_events.borrow().len(), 1);
        assert_eq!(watcher1_events.borrow().len(), 2);
        assert_eq!(watcher2_events.borrow().len(), 2);
    });
}

/// Rejeita agora (o `throw` síncrono) ou depois de 5 ms (o `timeout(5).then(() => { throw })`).
fn fail<T: Clone + 'static>(asynchronous: bool) -> Promise<T> {
    if asynchronous {
        timeout(5).then_promise(|_| Promise::rejected(Error::new("failed")))
    } else {
        Promise::rejected(Error::new("failed"))
    }
}

fn test_read_error_bubbles(asynchronous: bool) {
    async_test(|d| async move {
        let service = new_service(&d);
        let inner = NullFileSystemProvider::new();
        let provider = OverridableProvider::new(
            inner.clone(),
            Overrides {
                stat: Some(Rc::new(|_: &Prov, _: &URI| Promise::resolved(file_stat(100)))),
                read_file: Some(Rc::new(move |_: &Prov, _: &URI, _: bool| fail(asynchronous))),
                open: Some(Rc::new(move |_: &Prov, _: &URI, _: &FileOpenOptions| fail(asynchronous))),
                read_file_stream: Some(Rc::new(move |_: &Prov, _: &URI, _: &FileReadStreamOptions, _: &CancellationToken| {
                    let stream = new_writeable_buffer_stream(None);
                    if asynchronous {
                        let target = stream.clone();
                        let _ = timeout(5).then(move |_| target.error(Error::new("failed")));
                    } else {
                        stream.error(Error::new("failed"));
                        stream.end(None);
                    }
                    stream
                })),
                ..Default::default()
            },
        );
        register(&d, &service, "test", provider);

        for capabilities in [FileSystemProviderCapabilities::FILE_READ_WRITE, FileSystemProviderCapabilities::FILE_READ_STREAM, FileSystemProviderCapabilities::FILE_OPEN_READ_WRITE_CLOSE] {
            inner.set_capabilities(capabilities);

            let e1 = service.read_file(&uri("test://foo/bar"), None, None).await;
            assert!(e1.is_err());

            let e2: Result<(), Error> = async {
                let stream = service.read_file_stream(&uri("test://foo/bar"), None, None).await?;
                consume_stream(&*stream.value, concat()).await?;
                Ok(())
            }
            .await;
            assert!(e2.is_err());
        }
    });
}

#[test]
fn error_from_read_file_bubbles_through_async() {
    test_read_error_bubbles(true);
}

#[test]
fn error_from_read_file_bubbles_through() {
    test_read_error_bubbles(false);
}

/// Um token cujo `onCancellationRequested` conta as inscrições vivas (o `emitter._size` do original).
struct CountingToken {
    listeners: Rc<Cell<i32>>,
}

impl ICancellationToken for CountingToken {
    fn is_cancellation_requested(&self) -> bool {
        false
    }

    fn on_cancellation_requested(&self) -> Event<()> {
        let listeners = self.listeners.clone();
        Event::new(move |_| {
            listeners.set(listeners.get() + 1);
            let listeners = listeners.clone();
            to_disposable(move || listeners.set(listeners.get() - 1))
        })
    }
}

#[test]
fn read_file_stream_releases_cancellation_listeners_when_the_provider_throws_synchronously() {
    async_test(|d| async move {
        let service = new_service(&d);
        let inner = NullFileSystemProvider::new();
        let provider = OverridableProvider::new(
            inner.clone(),
            Overrides {
                stat: Some(Rc::new(|_: &Prov, _: &URI| Promise::resolved(Stat { file_type: FileType::FILE, ctime: 0, mtime: 0, size: 1, permissions: None }))),
                read_file_stream: Some(Rc::new(|_: &Prov, _: &URI, _: &FileReadStreamOptions, _: &CancellationToken| {
                    let stream = new_writeable_buffer_stream(None);
                    stream.error(Error::new("read failed"));
                    stream.end(None);
                    stream
                })),
                ..Default::default()
            },
        );
        inner.set_capabilities(FileSystemProviderCapabilities::FILE_READ_STREAM);
        register(&d, &service, "test", provider);

        let listeners = Rc::new(Cell::new(0));
        let token = CancellationToken::Custom(Rc::new(CountingToken { listeners: listeners.clone() }));

        let mut listener_counts = Vec::new();
        for _ in 0..5 {
            // O `throw` síncrono vira um stream com erro: a rejeição aparece ao consumir o stream.
            let error: Result<(), Error> = async {
                let stream = service.read_file_stream(&uri("test:///resource"), None, Some(token.clone())).await?;
                consume_stream(&*stream.value, concat()).await?;
                Ok(())
            }
            .await;
            assert!(error.err().unwrap().message.contains("read failed"));
            listener_counts.push(listeners.get());
        }

        assert_eq!(listener_counts, vec![0, 0, 0, 0, 0]);
    });
}

#[test]
fn read_file_read_file_stream_supports_cancellation() {
    async_test(|d| async move {
        let service = new_service(&d);
        let ready: Rc<RefCell<Option<Rc<DeferredPromise<()>>>>> = Rc::default();

        let inner = NullFileSystemProvider::new();
        let store = d.clone();
        let current = ready.clone();
        let provider = OverridableProvider::new(
            inner.clone(),
            Overrides {
                stat: Some(Rc::new(|_: &Prov, _: &URI| Promise::resolved(file_stat(100)))),
                read_file_stream: Some(Rc::new(move |_: &Prov, _: &URI, _: &FileReadStreamOptions, token: &CancellationToken| {
                    let stream = new_writeable_buffer_stream(None);
                    let target = stream.clone();
                    store.add(token.on_cancellation_requested().listen(move |_| {
                        target.error(Error::new("Expected cancellation"));
                        target.end(None);
                    }));

                    let _ = current.borrow().as_ref().unwrap().complete(());

                    stream
                })),
                ..Default::default()
            },
        );
        register(&d, &service, "test", provider);
        inner.set_capabilities(FileSystemProviderCapabilities::FILE_READ_STREAM);

        let cts = CancellationTokenSource::new(None);
        let deferred = Rc::new(DeferredPromise::new());
        *ready.borrow_mut() = Some(deferred.clone());
        let promise = service.read_file(&uri("test://foo/bar"), None, Some(cts.token())).then(|_| ());
        let cancel = cts.clone();
        let e1 = Promise::all(vec![deferred.promise.then(move |_| cancel.cancel()), promise]).await;
        assert!(e1.is_err());

        let cts = CancellationTokenSource::new(None);
        let deferred = Rc::new(DeferredPromise::new());
        *ready.borrow_mut() = Some(deferred.clone());
        let e2: Result<(), Error> = async {
            let stream = service.read_file_stream(&uri("test://foo/bar"), None, Some(cts.token())).await?;
            let cancel = cts.clone();
            Promise::all(vec![deferred.promise.then(move |_| cancel.cancel()), consume_stream(&*stream.value, concat()).then(|_| ())]).await?;
            Ok(())
        }
        .await;
        assert!(e2.is_err());
    });
}

fn test_read_file_waits_for_handle_closure(validation_error: bool) {
    async_test(|d| async move {
        let service = new_service(&d);
        let cts = Rc::new(CancellationTokenSource::new(None));
        let close_started = Rc::new(DeferredPromise::<()>::new());
        let finish_close = Rc::new(DeferredPromise::<()>::new());
        let calls = Rc::new(RefCell::new(Vec::<String>::new()));
        let inner = NullFileSystemProvider::new();
        let provider = OverridableProvider::new(
            inner.clone(),
            Overrides {
                stat: Some(Rc::new(move |_: &Prov, _: &URI| {
                    if validation_error {
                        return Promise::rejected(Error::new("metadata failed"));
                    }
                    Promise::resolved(Stat { file_type: FileType::FILE, ctime: 0, mtime: 0, size: 1, permissions: None })
                })),
                open: Some(Rc::new({
                    let (calls, cts) = (calls.clone(), cts.clone());
                    move |_: &Prov, _: &URI, _: &FileOpenOptions| {
                        calls.borrow_mut().push("open".into());
                        cts.cancel();
                        Promise::resolved(42)
                    }
                })),
                read: Some(Rc::new({
                    let calls = calls.clone();
                    move |_: &Prov, _: i64, _: u64, _: &VSBuffer, _: usize, _: usize| {
                        calls.borrow_mut().push("read".into());
                        Promise::resolved(0)
                    }
                })),
                close: Some(Rc::new({
                    let (calls, started, finish) = (calls.clone(), close_started.clone(), finish_close.clone());
                    move |_: &Prov, _: i64| {
                        calls.borrow_mut().push("close".into());
                        let (started, finish) = (started.clone(), finish.clone());
                        async_fn(async move {
                            let _ = started.complete(());
                            finish.promise.clone().await?;
                            Ok(())
                        })
                    }
                })),
                ..Default::default()
            },
        );
        inner.set_capabilities(FileSystemProviderCapabilities::FILE_OPEN_READ_WRITE_CLOSE);
        register(&d, &service, "test", provider);

        let settled = Rc::new(Cell::new(false));
        let flag = settled.clone();
        let result = service.read_file(&uri("test:///resource"), None, Some(cts.token())).finally(move || flag.set(true));
        close_started.promise.clone().await.unwrap();
        let settled_before_close = settled.get();
        let _ = finish_close.complete(());
        let error = result.await.err().unwrap();
        assert!(error.message.contains(if validation_error { "metadata failed" } else { "Canceled" }), "{error}");

        assert_eq!(*calls.borrow(), vec!["open".to_string(), "close".to_string()]);
        assert!(!settled_before_close);
    });
}

#[test]
fn read_file_waits_for_handle_closure_after_cancellation_during_open() {
    test_read_file_waits_for_handle_closure(false);
}

#[test]
fn read_file_waits_for_handle_closure_after_metadata_failure() {
    test_read_file_waits_for_handle_closure(true);
}

#[test]
fn enforced_atomic_read_write_delete() {
    async_test(|d| async move {
        let service = new_service(&d);
        let ext_uri = Rc::new(ExtUri::unbiased(Rc::new(ProcessEnvironment)));

        let atomic_resource = uri("test://foo/bar/atomic");
        let non_atomic_resource = uri("test://foo/nonatomic");

        let (reads, writes, deletes) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let is_atomic = {
            let (ext_uri, atomic_resource) = (ext_uri.clone(), atomic_resource.clone());
            move |resource: &URI| ext_uri.is_equal(Some(resource), Some(&atomic_resource), false)
        };

        let inner = NullFileSystemProvider::new();
        let provider = OverridableProvider::new(
            inner.clone(),
            Overrides {
                stat: Some(Rc::new(|_: &Prov, _: &URI| Promise::resolved(Stat { file_type: FileType::FILE, ctime: now(), mtime: now(), size: 0, permissions: None }))),
                read_file: Some(Rc::new({
                    let reads = reads.clone();
                    move |_: &Prov, _: &URI, atomic: bool| {
                        if atomic {
                            reads.set(reads.get() + 1);
                        }
                        Promise::resolved(VSBuffer::alloc(0))
                    }
                })),
                read_file_stream: Some(Rc::new(|_: &Prov, _: &URI, _: &FileReadStreamOptions, _: &CancellationToken| new_writeable_buffer_stream(None))),
                enforce_atomic_read_file: Some(Rc::new({
                    let is_atomic = is_atomic.clone();
                    move |_: &Prov, resource: &URI| is_atomic(resource)
                })),
                write_file: Some(Rc::new({
                    let writes = writes.clone();
                    move |_: &Prov, _: &URI, _: VSBuffer, opts: &FileWriteOptions| {
                        if opts.atomic.is_some() {
                            writes.set(writes.get() + 1);
                        }
                        Promise::resolved(())
                    }
                })),
                enforce_atomic_write_file: Some(Rc::new({
                    let is_atomic = is_atomic.clone();
                    move |_: &Prov, resource: &URI| is_atomic(resource).then(|| FileAtomicOptions { postfix: ".tmp".into() })
                })),
                delete: Some(Rc::new({
                    let deletes = deletes.clone();
                    move |_: &Prov, _: &URI, opts: &FileDeleteOptions| {
                        if opts.atomic.is_some() {
                            deletes.set(deletes.get() + 1);
                        }
                        Promise::resolved(())
                    }
                })),
                enforce_atomic_delete: Some(Rc::new(move |_: &Prov, resource: &URI| is_atomic(resource).then(|| FileAtomicOptions { postfix: ".tmp".into() }))),
                ..Default::default()
            },
        );
        inner.set_capabilities(
            FileSystemProviderCapabilities::FILE_READ_WRITE
                | FileSystemProviderCapabilities::FILE_OPEN_READ_WRITE_CLOSE
                | FileSystemProviderCapabilities::FILE_READ_STREAM
                | FileSystemProviderCapabilities::FILE_ATOMIC_READ
                | FileSystemProviderCapabilities::FILE_ATOMIC_WRITE
                | FileSystemProviderCapabilities::FILE_ATOMIC_DELETE,
        );
        register(&d, &service, "test", provider);

        service.read_file(&atomic_resource, None, None).await.unwrap();
        service.read_file(&non_atomic_resource, None, None).await.unwrap();
        service.read_file_stream(&atomic_resource, None, None).await.unwrap();
        service.read_file_stream(&non_atomic_resource, None, None).await.unwrap();

        let empty = || VSBuffer::from_string("");
        for resource in [&atomic_resource, &non_atomic_resource] {
            service.write_file(resource, empty().into(), None).await.unwrap();
        }
        for resource in [&atomic_resource, &non_atomic_resource] {
            service.write_file(resource, FileContentSource::from(buffer_to_stream(empty()) as je_base::buffer::VSBufferReadableStream), None).await.unwrap();
        }
        for resource in [&atomic_resource, &non_atomic_resource] {
            let readable: Box<dyn je_base::stream::Readable<VSBuffer>> = Box::new(buffer_to_readable(empty()));
            service.write_file(resource, readable.into(), None).await.unwrap();
        }

        service.del(&atomic_resource, None).await.unwrap();
        service.del(&non_atomic_resource, None).await.unwrap();

        assert_eq!(reads.get(), 2);
        assert_eq!(writes.get(), 3);
        assert_eq!(deletes.get(), 1);
    });
}

/// `BufferedWriteFileSystemProvider`: o `InMemoryFileSystemProvider` sem `FileReadWrite`.
fn without_read_write(overrides: Overrides) -> Overrides {
    Overrides { capabilities: Some(Rc::new(|sup: &Prov| sup.capabilities() & !FileSystemProviderCapabilities::FILE_READ_WRITE)), ..overrides }
}

fn buffered_service(d: &Rc<DisposableStore>, overrides: Overrides, buffered: bool) -> (Rc<FileService>, Rc<OverridableProvider>) {
    let service = new_service(d);
    let memory = d.add_rc(InMemoryFileSystemProvider::new());
    let overrides = if buffered { without_read_write(overrides) } else { overrides };
    let provider = OverridableProvider::new(memory, overrides);
    register(d, &service, "test", provider.clone());
    (service, provider)
}

fn destroy_with(d: &Rc<DisposableStore>, stream: &Rc<impl je_base::stream::ReadableStream<VSBuffer> + 'static>) {
    let stream = stream.clone();
    d.add(to_disposable(move || stream.destroy()));
}

fn test_buffered_stream_waits_for_pending_writes(write_fails: bool, stream_fails: bool) {
    async_test(|d| async move {
        let write_started = Rc::new(DeferredPromise::<()>::new());
        let finish_write = Rc::new(DeferredPromise::<()>::new());
        let write_finished = Rc::new(DeferredPromise::<()>::new());
        let events = Rc::new(RefCell::new(Vec::<String>::new()));
        let inner = NullFileSystemProvider::new();
        let provider = OverridableProvider::new(
            inner.clone(),
            Overrides {
                stat: Some(Rc::new(|_: &Prov, _: &URI| Promise::resolved(Stat { file_type: FileType::FILE, ctime: 0, mtime: 0, size: 0, permissions: None }))),
                open: Some(Rc::new(|_: &Prov, _: &URI, _: &FileOpenOptions| Promise::resolved(1))),
                write: Some(Rc::new({
                    let (events, started, finish, finished) = (events.clone(), write_started.clone(), finish_write.clone(), write_finished.clone());
                    move |_: &Prov, _: i64, _: u64, _: &VSBuffer, _: usize, length: usize| {
                        let (events, started, finish, finished) = (events.clone(), started.clone(), finish.clone(), finished.clone());
                        async_fn(async move {
                            events.borrow_mut().push("write started".into());
                            let _ = started.complete(());
                            finish.promise.clone().await?;
                            events.borrow_mut().push("write finished".into());
                            let _ = finished.complete(());
                            if write_fails {
                                return Err(Error::new("write failed"));
                            }
                            Ok(length)
                        })
                    }
                })),
                close: Some(Rc::new({
                    let events = events.clone();
                    move |_: &Prov, _: i64| {
                        events.borrow_mut().push("close".into());
                        Promise::resolved(())
                    }
                })),
                ..Default::default()
            },
        );
        inner.set_capabilities(FileSystemProviderCapabilities::FILE_OPEN_READ_WRITE_CLOSE);
        let service = new_service(&d);
        register(&d, &service, "test", provider);

        let stream = new_writeable_stream::<VSBuffer>(Some(concat()), None);
        destroy_with(&d, &stream);
        stream.write(VSBuffer::from_string("content"));
        if stream_fails {
            stream.error(Error::new("stream failed"));
        }
        stream.end(None);

        let result = service
            .write_file(&uri("test:///file"), FileContentSource::from(stream.clone() as je_base::buffer::VSBufferReadableStream), None)
            .then_result(|r| Ok(match r {
                Ok(_) => "success".to_string(),
                Err(e) => e.message,
            }));
        write_started.promise.clone().await.unwrap();
        timeout(0).await.unwrap();
        let _ = finish_write.complete(());
        let outcome = result.await.unwrap();
        write_finished.promise.clone().await.unwrap();

        let expected_error = if stream_fails { Some("stream failed") } else if write_fails { Some("write failed") } else { None };
        assert_eq!(*events.borrow(), vec!["write started".to_string(), "write finished".to_string(), "close".to_string()]);
        match expected_error {
            Some(message) => assert!(outcome.contains(message), "{outcome}"),
            None => assert_eq!(outcome, "success"),
        }
    });
}

#[test]
fn buffered_stream_waits_for_pending_writes_before_closing_write_error_false_stream_error_false() {
    test_buffered_stream_waits_for_pending_writes(false, false);
}

#[test]
fn buffered_stream_waits_for_pending_writes_before_closing_write_error_true_stream_error_false() {
    test_buffered_stream_waits_for_pending_writes(true, false);
}

#[test]
fn buffered_stream_waits_for_pending_writes_before_closing_write_error_false_stream_error_true() {
    test_buffered_stream_waits_for_pending_writes(false, true);
}

#[test]
fn buffered_stream_waits_for_pending_writes_before_closing_write_error_true_stream_error_true() {
    test_buffered_stream_waits_for_pending_writes(true, true);
}

#[derive(Clone, Debug, PartialEq)]
struct WriteCall {
    pos: u64,
    offset: usize,
    length: usize,
}

#[test]
fn buffered_stream_serializes_buffered_chunks_and_completes_partial_writes() {
    async_test(|d| async move {
        let writes = Rc::new(RefCell::new(Vec::<WriteCall>::new()));
        let (active, max_active) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let (service, provider) = buffered_service(
            &d,
            Overrides {
                write: Some(Rc::new({
                    let (writes, active, max_active) = (writes.clone(), active.clone(), max_active.clone());
                    move |sup: &Prov, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize| {
                        writes.borrow_mut().push(WriteCall { pos, offset, length });
                        active.set(active.get() + 1);
                        max_active.set(max_active.get().max(active.get()));
                        let (sup, data, active) = (sup.clone(), data.clone(), active.clone());
                        async_fn(async move {
                            let result = async {
                                timeout(0).await?;
                                sup.write(fd, pos, &data, offset, length.min(2)).await
                            }
                            .await;
                            active.set(active.get() - 1);
                            result
                        })
                    }
                })),
                ..Default::default()
            },
            true,
        );

        let stream = new_writeable_stream::<VSBuffer>(None, None);
        destroy_with(&d, &stream);
        stream.write(VSBuffer::from_string("abc"));
        stream.write(VSBuffer::from_string("defg"));
        stream.end(None);

        let resource = uri("test:///file");
        let stat = service.write_file(&resource, FileContentSource::from(stream.clone() as je_base::buffer::VSBufferReadableStream), None).await.unwrap();

        assert_eq!(
            *writes.borrow(),
            vec![
                WriteCall { pos: 0, offset: 0, length: 3 },
                WriteCall { pos: 2, offset: 2, length: 1 },
                WriteCall { pos: 3, offset: 0, length: 4 },
                WriteCall { pos: 5, offset: 2, length: 2 },
            ]
        );
        assert_eq!(max_active.get(), 1);
        assert_eq!(stat.size(), 7);
        assert_eq!(text_of(provider.read_file(&resource, false)).await, "abcdefg");
    });
}

/// Um `write` que registra `(pos, conteúdo)`, espera `timeout(0)` e delega ao `super`.
fn logging_write(writes: &Rc<RefCell<Vec<(u64, String)>>>) -> Overrides {
    Overrides {
        write: Some(Rc::new({
            let writes = writes.clone();
            move |sup: &Prov, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize| {
                let content = data.slice(Some(offset as i64), Some((offset + length) as i64)).to_string();
                writes.borrow_mut().push((pos, content));
                let (sup, data) = (sup.clone(), data.clone());
                async_fn(async move {
                    timeout(0).await?;
                    sup.write(fd, pos, &data, offset, length).await
                })
            }
        })),
        ..Default::default()
    }
}

fn pairs(items: &[(u64, &str)]) -> Vec<(u64, String)> {
    items.iter().map(|(p, c)| (*p, c.to_string())).collect()
}

#[test]
fn buffered_stream_resumes_a_producer_waiting_on_backpressure_across_multiple_chunks() {
    async_test(|d| async move {
        let writes = Rc::new(RefCell::new(Vec::new()));
        let (service, provider) = buffered_service(&d, logging_write(&writes), true);

        let stream = new_writeable_stream::<VSBuffer>(Some(concat()), Some(WriteableStreamOptions { high_water_mark: Some(0) }));
        destroy_with(&d, &stream);
        let resource = uri("test:///file");
        let result = service.write_file(&resource, FileContentSource::from(stream.clone() as je_base::buffer::VSBufferReadableStream), None).then(|_| ());
        let backpressure_count = Rc::new(Cell::new(0));
        let producer = {
            let (stream, count) = (stream.clone(), backpressure_count.clone());
            async_fn(async move {
                for chunk in ["one", "two", "three"] {
                    if let Some(pending) = stream.write(VSBuffer::from_string(chunk)) {
                        count.set(count.get() + 1);
                        pending.await?;
                    }
                }
                stream.end(None);
                Ok(())
            })
        };
        Promise::all(vec![result, producer]).await.unwrap();

        assert_eq!(*writes.borrow(), pairs(&[(0, "one"), (3, "two"), (6, "three")]));
        assert_eq!(backpressure_count.get(), 3);
        assert_eq!(text_of(provider.read_file(&resource, false)).await, "onetwothree");
    });
}

#[test]
fn buffered_stream_writes_the_peeked_prefix_before_resuming_the_remaining_chunks() {
    async_test(|d| async move {
        let writes = Rc::new(RefCell::new(Vec::new()));
        let (service, provider) = buffered_service(&d, logging_write(&writes), false);

        let stream = new_writeable_stream::<VSBuffer>(Some(concat()), Some(WriteableStreamOptions { high_water_mark: Some(0) }));
        destroy_with(&d, &stream);
        let resource = uri("test:///file");
        let result = service.write_file(&resource, FileContentSource::from(stream.clone() as je_base::buffer::VSBufferReadableStream), None).then(|_| ());
        let producer = {
            let stream = stream.clone();
            async_fn(async move {
                for chunk in ["a", "b", "c", "d", "ef", "gh"] {
                    if let Some(pending) = stream.write(VSBuffer::from_string(chunk)) {
                        pending.await?;
                    }
                }
                stream.end(None);
                Ok(())
            })
        };
        Promise::all(vec![result, producer]).await.unwrap();

        assert_eq!(*writes.borrow(), pairs(&[(0, "abcd"), (4, "ef"), (6, "gh")]));
        assert_eq!(text_of(provider.read_file(&resource, false)).await, "abcdefgh");
    });
}

fn test_buffered_stream_releases_write_queue(failure: &'static str) {
    async_test(|d| async move {
        let write_started = Rc::new(DeferredPromise::<()>::new());
        let finish_write = Rc::new(DeferredPromise::<()>::new());
        let events = Rc::new(RefCell::new(Vec::<String>::new()));
        let (service, provider) = buffered_service(
            &d,
            Overrides {
                open: Some(Rc::new({
                    let events = events.clone();
                    move |sup: &Prov, resource: &URI, options: &FileOpenOptions| {
                        let (sup, resource, options, events) = (sup.clone(), resource.clone(), *options, events.clone());
                        async_fn(async move {
                            let fd = sup.open(&resource, &options).await?;
                            events.borrow_mut().push(format!("open {fd}"));
                            Ok(fd)
                        })
                    }
                })),
                write: Some(Rc::new({
                    let (events, started, finish) = (events.clone(), write_started.clone(), finish_write.clone());
                    move |sup: &Prov, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize| {
                        events.borrow_mut().push(format!("write {fd}"));
                        let (sup, data, started, finish) = (sup.clone(), data.clone(), started.clone(), finish.clone());
                        async_fn(async move {
                            if fd == 0 {
                                let _ = started.complete(());
                                finish.promise.clone().await?;
                                if failure == "write" {
                                    return Err(Error::new("write failed"));
                                }
                            }
                            sup.write(fd, pos, &data, offset, length).await
                        })
                    }
                })),
                close: Some(Rc::new({
                    let events = events.clone();
                    move |sup: &Prov, fd: i64| {
                        let (sup, events) = (sup.clone(), events.clone());
                        async_fn(async move {
                            sup.close(fd).await?;
                            events.borrow_mut().push(format!("close {fd}"));
                            Ok(())
                        })
                    }
                })),
                ..Default::default()
            },
            true,
        );

        let stream = new_writeable_stream::<VSBuffer>(Some(concat()), None);
        destroy_with(&d, &stream);
        stream.write(VSBuffer::from_string("first"));
        if failure == "stream" {
            stream.error(Error::new("stream failed"));
        }
        stream.end(None);

        let resource = uri("test:///file");
        let first_result = service.write_file(&resource, FileContentSource::from(stream.clone() as je_base::buffer::VSBufferReadableStream), None);
        write_started.promise.clone().await.unwrap();
        let next_stream = buffer_to_stream(VSBuffer::from_string("recovery"));
        destroy_with(&d, &next_stream);
        let next_result = service.write_file(&resource, FileContentSource::from(next_stream.clone() as je_base::buffer::VSBufferReadableStream), None);
        timeout(0).await.unwrap();
        let events_before_release = events.borrow().clone();
        let _ = finish_write.complete(());
        let first_error = first_result.await.err().unwrap();
        assert!(first_error.message.contains(&format!("{failure} failed")), "{first_error}");
        next_result.await.unwrap();

        assert_eq!(events_before_release, vec!["open 0".to_string(), "write 0".to_string()]);
        let expected: Vec<String> = ["open 0", "write 0", "close 0", "open 1", "write 1", "close 1"].iter().map(|s| s.to_string()).collect();
        assert_eq!(*events.borrow(), expected);
        assert_eq!(text_of(provider.read_file(&resource, false)).await, "recovery");
    });
}

#[test]
fn buffered_stream_releases_the_write_queue_after_a_write_failure() {
    test_buffered_stream_releases_write_queue("write");
}

#[test]
fn buffered_stream_releases_the_write_queue_after_a_stream_failure() {
    test_buffered_stream_releases_write_queue("stream");
}

#[test]
fn buffered_stream_closes_an_empty_stream_without_writing() {
    async_test(|d| async move {
        let events = Rc::new(RefCell::new(Vec::<String>::new()));
        let (service, provider) = buffered_service(
            &d,
            Overrides {
                write: Some(Rc::new({
                    let events = events.clone();
                    move |sup: &Prov, fd: i64, pos: u64, data: &VSBuffer, offset: usize, length: usize| {
                        events.borrow_mut().push("write".into());
                        sup.write(fd, pos, data, offset, length)
                    }
                })),
                close: Some(Rc::new({
                    let events = events.clone();
                    move |sup: &Prov, fd: i64| {
                        let (sup, events) = (sup.clone(), events.clone());
                        async_fn(async move {
                            sup.close(fd).await?;
                            events.borrow_mut().push("close".into());
                            Ok(())
                        })
                    }
                })),
                ..Default::default()
            },
            true,
        );

        let stream = buffer_to_stream(VSBuffer::alloc(0));
        destroy_with(&d, &stream);
        let resource = uri("test:///file");
        let stat = service.write_file(&resource, FileContentSource::from(stream.clone() as je_base::buffer::VSBufferReadableStream), None).await.unwrap();

        assert_eq!(*events.borrow(), vec!["close".to_string()]);
        assert_eq!(stat.size(), 0);
        assert_eq!(text_of(provider.read_file(&resource, false)).await, "");
    });
}
