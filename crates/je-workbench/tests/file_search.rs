//! Porte de `src/vs/workbench/services/search/test/node/fileSearch.test.ts` (suíte `FileWalker`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - Cada caso é `async` e roda em [`run_with_real_timers`]; `await promise` é `.await.unwrap()` sobre a promessa.
//! - `new DeferredPromise<T>()` é o [`DeferredPromise`] do porte (`.p` é o campo `promise`); `completed.complete()` é
//!   `complete(())`.
//! - `TEST_FIXTURES` é `tests/fixtures/search` deste crate (no original, `vs/workbench/services/search/test/node/fixtures`):
//!   os casos só precisam de um diretório existente como pasta de busca e como `cwd` do processo filho.
//! - `childProcess.spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'])`, um processo que não termina sozinho, é
//!   `sleep 1000` no Unix e `ping -n 1000 127.0.0.1` no Windows; `rgDiskPath: process.execPath` é o nome desse programa.
//!   `childProcess.spawn(path.join(TEST_FIXTURES, 'missing-ripgrep'))` é o mesmo `ChildProcess::spawn` de um executável
//!   inexistente, que também não falha na hora: o erro chega depois pelo ouvinte.
//! - `cmd.killed` é `ChildProcess::killed()` e `cmd.kill()` é `ChildProcess::kill()`.
//! - O `try { ... } finally { ... }` do primeiro caso é o `catch_unwind` da asserção seguido da limpeza, que relança o pânico
//!   no fim (a limpeza espera uma promessa, então não cabe num `Drop`).
//! - O `spawnRipgrep` async do `FileWalker` é um `SpawnRipgrepCmd`: o `async () => {...}` que espera `allowSpawn.p` é
//!   `allow_spawn.promise.then(...)`, e o que não espera é `Promise::resolved(...)`.
//! - `walker.walk([TEST_FOLDER_QUERY], [], undefined, () => { }, () => { }, () => completed.complete())` é `walk` com
//!   `numThreads` `None` e os dois retornos de chamada vazios.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::process::Command;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::glob::Expression;
use je_base::promise::{DeferredPromise, Promise};
use je_platform::resources::uri_file;
use je_workbench::services::search::file_walker::{ChildProcess, FileWalker, OnMessage, OnResult, RipgrepCmd, SpawnRipgrepCmd};
use je_workbench::services::search::ripgrep_file_search::RgArgs;
use je_workbench::services::search::search::{IFileQuery, IFolderQuery, IProgressMessage, IRawFileMatch};

fn test_fixtures() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("search").to_string_lossy().into_owned()
}

fn test_folder_query() -> IFolderQuery {
    IFolderQuery::new(uri_file(&test_fixtures()))
}

fn test_query() -> IFileQuery {
    IFileQuery::new(vec![test_folder_query()])
}

/// O programa que não termina sozinho (o `process.execPath -e setInterval(...)` do original).
fn long_running_program() -> (&'static str, Vec<&'static str>) {
    if cfg!(windows) { ("ping", vec!["-n", "1000", "127.0.0.1"]) } else { ("sleep", vec!["1000"]) }
}

fn spawn_long_running() -> (Rc<ChildProcess>, &'static str) {
    let (program, args) = long_running_program();
    (ChildProcess::spawn(Command::new(program).args(args)), program)
}

/// O objeto que o `spawnRipgrep` do original devolve.
fn ripgrep_cmd(cmd: Rc<ChildProcess>, rg_disk_path: &str) -> RipgrepCmd {
    RipgrepCmd {
        cmd,
        rg_disk_path: rg_disk_path.to_owned(),
        sibling_clauses: Expression::new(),
        rg_args: RgArgs { args: Vec::new(), sibling_clauses: Expression::new() },
        cwd: test_fixtures(),
    }
}

fn ignore_result() -> OnResult {
    Rc::new(|_: &IRawFileMatch| {})
}

fn ignore_message() -> OnMessage {
    Rc::new(|_: &IProgressMessage| {})
}

struct StartedWalker {
    walker: Rc<FileWalker>,
    spawned: Promise<Rc<ChildProcess>>,
    completed: Promise<()>,
}

#[test]
fn cancelling_one_walker_does_not_cancel_another_walker() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(async {
        let start_walker = || {
            let spawned = Rc::new(DeferredPromise::<Rc<ChildProcess>>::new());
            let completed = Rc::new(DeferredPromise::<()>::new());
            let spawn_ripgrep: SpawnRipgrepCmd = {
                let spawned = spawned.clone();
                Rc::new(move |_: &IFileQuery, _: &IFolderQuery, _: Option<&Expression>, _: Option<&Expression>, _: Option<i64>| {
                    let (cmd, program) = spawn_long_running();
                    spawned.complete(cmd.clone());
                    Promise::resolved(ripgrep_cmd(cmd, program))
                })
            };
            let walker = FileWalker::new(&test_query(), spawn_ripgrep);
            let completed_for_walk = completed.clone();
            walker.walk(&[test_folder_query()], &[], None, ignore_result(), ignore_message(), move |_, _| {
                completed_for_walk.complete(());
            });
            StartedWalker { walker, spawned: spawned.promise.clone(), completed: completed.promise.clone() }
        };

        let first = start_walker();
        let second = start_walker();
        let processes = Promise::all(vec![first.spawned.clone(), second.spawned.clone()]).await.unwrap();
        let (first_process, second_process) = (processes[0].clone(), processes[1].clone());

        first.walker.cancel();
        first.completed.clone().await.unwrap();

        let asserted = catch_unwind(AssertUnwindSafe(|| {
            assert_eq!((first_process.killed(), second_process.killed()), (true, false));
        }));

        if !first_process.killed() {
            first_process.kill();
        }
        second.walker.cancel();
        second.completed.clone().await.unwrap();

        if let Err(payload) = asserted {
            resume_unwind(payload);
        }
    });
}

#[test]
fn cancelling_while_ripgrep_is_resolving_kills_the_spawned_process() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(async {
        let allow_spawn = Rc::new(DeferredPromise::<()>::new());
        let spawned = Rc::new(DeferredPromise::<Rc<ChildProcess>>::new());
        let completed = Rc::new(DeferredPromise::<()>::new());
        let spawn_ripgrep: SpawnRipgrepCmd = {
            let allow_spawn = allow_spawn.clone();
            let spawned = spawned.clone();
            Rc::new(move |_: &IFileQuery, _: &IFolderQuery, _: Option<&Expression>, _: Option<&Expression>, _: Option<i64>| {
                let spawned = spawned.clone();
                allow_spawn.promise.then(move |()| {
                    let (cmd, program) = spawn_long_running();
                    spawned.complete(cmd.clone());
                    ripgrep_cmd(cmd, program)
                })
            })
        };
        let walker = FileWalker::new(&test_query(), spawn_ripgrep);
        let completed_for_walk = completed.clone();
        walker.walk(&[test_folder_query()], &[], None, ignore_result(), ignore_message(), move |_, _| {
            completed_for_walk.complete(());
        });

        walker.cancel();
        allow_spawn.complete(());
        let spawned_process = spawned.promise.clone().await.unwrap();

        let asserted = {
            completed.promise.clone().await.unwrap();
            catch_unwind(AssertUnwindSafe(|| assert!(spawned_process.killed())))
        };
        if !spawned_process.killed() {
            spawned_process.kill();
        }
        if let Err(payload) = asserted {
            resume_unwind(payload);
        }
    });
}

#[test]
fn cancelling_while_a_missing_ripgrep_executable_is_resolving_handles_the_spawn_error() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(async {
        let allow_spawn = Rc::new(DeferredPromise::<()>::new());
        let completed = Rc::new(DeferredPromise::<()>::new());
        let spawn_ripgrep: SpawnRipgrepCmd = {
            let allow_spawn = allow_spawn.clone();
            Rc::new(move |_: &IFileQuery, _: &IFolderQuery, _: Option<&Expression>, _: Option<&Expression>, _: Option<i64>| {
                allow_spawn.promise.then(|()| {
                    let cmd = ChildProcess::spawn(&mut Command::new(std::path::Path::new(&test_fixtures()).join("missing-ripgrep")));
                    ripgrep_cmd(cmd, "ripgrep")
                })
            })
        };
        let walker = FileWalker::new(&test_query(), spawn_ripgrep);
        let completed_for_walk = completed.clone();
        walker.walk(&[test_folder_query()], &[], None, ignore_result(), ignore_message(), move |_, _| {
            completed_for_walk.complete(());
        });

        walker.cancel();
        allow_spawn.complete(());

        completed.promise.clone().await.unwrap();
    });
}
