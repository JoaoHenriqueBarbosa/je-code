//! Helpers dos testes portados: os do `je-base` (`run_with_faked_timers`, `timeout`) e os do
//! `testUtils.ts` do VS Code (`getRandomTestPath`) e do mocha (`Promise.all`, `setup`/`teardown`).
#![allow(dead_code)]

#[path = "../../../je-base/tests/common/mod.rs"]
mod base;

use std::cell::Cell;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use je_base::errors::Error;
use je_base::promise::Promise;
use je_base::runtime;

#[allow(unused_imports)]
pub use base::{run_with_faked_timers, timeout};

/// O corpo `async` de um teste do mocha que não usa `runWithFakedTimers`: relógio real, e o loop roda
/// até esvaziar.
pub fn run_with_real_timers(body: impl Future<Output = ()> + 'static) {
    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    runtime::spawn_local(async move {
        body.await;
        finished.set(true);
    });
    runtime::run_until_idle();
    base::fail_on_unexpected_errors();
    assert!(done.get(), "o teste assíncrono não terminou");
}

/// `Promise.all(promises)`: o primeiro erro rejeita; na ordem de entrada.
pub async fn all<T: Clone + 'static>(promises: Vec<Promise<T>>) -> Result<Vec<T>, Error> {
    let mut values = Vec::new();
    for result in Promise::all_settled(promises).await? {
        values.push(result?);
    }
    Ok(values)
}

/// `promise.then(() => flag = true)`: o flag que vira `true` quando a promessa resolve.
pub fn track(promise: &Promise<()>) -> Rc<Cell<bool>> {
    let flag = Rc::new(Cell::new(false));
    let f = flag.clone();
    drop(promise.then(move |()| f.set(true)));
    flag
}

/// O diretório de teste do `setup`/`teardown`: criado no início e apagado no `Drop`
/// (`getRandomTestPath(tmpdir(), 'vsctests', ...)` + `Promises.rm`).
pub struct TestDir {
    path: PathBuf,
}

impl TestDir {
    pub fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = format!(
            "{}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
            je_base::uuid::generate_uuid()
        );
        let path = std::env::temp_dir()
            .join("vsctests")
            .join(unique)
            .join(name);
        std::fs::create_dir_all(&path).expect("cria o diretório de teste");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `join(testDir, name)`, como texto.
    pub fn join(&self, name: &str) -> String {
        self.path.join(name).to_string_lossy().into_owned()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let root = self.path.parent().unwrap_or(&self.path).to_path_buf();
        let _ = std::fs::remove_dir_all(root);
    }
}
