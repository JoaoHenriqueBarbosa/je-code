//! Porte de `src/vs/platform/files/test/node/diskFileSystemProvider.test.ts` (suíte `DiskFileSystemProvider`), caso a caso e na mesma ordem.
//!
//! Divergências:
//!
//! - Cada teste `async` roda no laço de eventos real do thread ([`run_on_real_loop`]), porque a E/S de disco
//!   leva tempo de verdade; o `setup` e o `teardown` (provider e diretório de teste) são o `Setup` e o `Drop`
//!   do `TestDir`.
//! - Os três casos de named pipe rodam o que o `readNamedPipe.ts` faz num thread isolado, com o laço de
//!   eventos e o provider dele, esperando no máximo 5 segundos (o `timeout: 5000` do `execFile`): se uma
//!   regressão voltar a bloquear a leitura, só aquele thread trava, e o teste reprova por tempo.
//! - O caso do Windows (`rename fails quickly...`) troca o processo filho do Node, que segura a pasta como
//!   diretório de trabalho, por um `ping` do Windows. É `ignore` fora do Windows, como o `test.skip` do
//!   original, e não foi executado aqui (a máquina de desenvolvimento é Linux).

mod common;

use std::path::Path;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::disk::{TestDir, file_uri, run_on_real_loop};
use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::buffer::stream_to_buffer;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::lifecycle::DisposableStore;
use je_platform::files::disk::DiskFileSystemProvider;
use je_platform::files::files::*;
use je_platform::log::NullLogService;

/// O `setup` da suíte: o provider (descartado junto com o store do teste) e o diretório de teste.
struct Setup {
    provider: Rc<DiskFileSystemProvider>,
    test_dir: TestDir,
}

fn setup(store: &Rc<DisposableStore>) -> Setup {
    let provider = store.add_rc(DiskFileSystemProvider::new(NullLogService::shared(), None));
    Setup { provider, test_dir: TestDir::new(&["vsctests", "diskfilesystemprovider"]) }
}

/// `error instanceof Error && toFileSystemProviderErrorCode(error) === code`.
fn assert_rejects_with_code<T: std::fmt::Debug>(result: Result<T, Error>, code: FileSystemProviderErrorCode) {
    let error = result.expect_err("a operação devia ter rejeitado");
    assert_eq!(to_file_system_provider_error_code(Some(&error)), code, "erro: {error}");
}

#[test]
fn read_file_reads_regular_files() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Setup { provider, test_dir } = setup(&guard.store());
    run_on_real_loop(async move {
        let file_path = test_dir.join("file.txt");
        std::fs::write(&file_path, "contents").unwrap();

        assert_eq!(provider.read_file(&file_uri(&file_path), false).await.unwrap().to_string(), "contents");
    });
}

#[test]
fn realpath_reports_missing_paths_as_a_filesystem_error() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Setup { provider, test_dir } = setup(&guard.store());
    run_on_real_loop(async move {
        assert_rejects_with_code(provider.realpath(&test_dir.uri("removed-checkout")).await, FileSystemProviderErrorCode::FileNotFound);
    });
}

/// O que o `readNamedPipe.ts` faz: o `method` sobre o named pipe tem de rejeitar com `Unavailable`. Roda no
/// thread que chama, com o provider e o laço de eventos dele.
fn read_named_pipe(method: &str, file_path: &Path) {
    let provider = DiskFileSystemProvider::new(NullLogService::shared(), None);
    let resource = file_uri(file_path);
    let method = method.to_string();
    let reader = provider.clone();
    run_on_real_loop(async move {
        let result = match method.as_str() {
            "open" => match reader.open(&resource, &FileOpenOptions::for_read()).await {
                Ok(handle) => reader.close(handle).await,
                Err(error) => Err(error),
            },
            "readFile" => reader.read_file(&resource, false).await.map(drop),
            "readFileStream" => stream_to_buffer(&*reader.read_file_stream(&resource, &FileReadStreamOptions::default(), &CancellationToken::None)).await.map(drop),
            other => panic!("Unexpected read method: {other}"),
        };
        assert_rejects_with_code(result, FileSystemProviderErrorCode::Unavailable);
    });
    use je_base::lifecycle::IDisposable;
    provider.dispose();
}

fn named_pipe_without_a_writer_is_rejected(method: &'static str) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Setup { provider: _, test_dir } = setup(&guard.store());
    let fifo_path = test_dir.join("named-pipe");
    let status = Command::new("mkfifo").arg(&fifo_path).status().expect("mkfifo");
    assert!(status.success(), "mkfifo falhou");

    // Isolate blocking reads so a regression cannot leave the test runner's worker pool stuck.
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(|| read_named_pipe(method, &fifo_path));
        let _ = sender.send(outcome.map_err(|payload| {
            payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_else(|| "pânico".to_string())
        }));
    });
    match receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => {}
        Ok(Err(message)) => panic!("{method} não rejeitou o named pipe como esperado: {message}"),
        Err(_) => panic!("{method} travou lendo o named pipe sem escritor"),
    }
}

#[test]
#[cfg_attr(windows, ignore = "test.skip no Windows (não há mkfifo)")]
fn open_rejects_named_pipes_without_a_writer() {
    named_pipe_without_a_writer_is_rejected("open");
}

#[test]
#[cfg_attr(windows, ignore = "test.skip no Windows (não há mkfifo)")]
fn read_file_rejects_named_pipes_without_a_writer() {
    named_pipe_without_a_writer_is_rejected("readFile");
}

#[test]
#[cfg_attr(windows, ignore = "test.skip no Windows (não há mkfifo)")]
fn read_file_stream_rejects_named_pipes_without_a_writer() {
    named_pipe_without_a_writer_is_rejected("readFileStream");
}

#[test]
#[cfg_attr(windows, ignore = "test.skip no Windows (não há /dev/null)")]
fn read_file_rejects_non_regular_files() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Setup { provider, test_dir: _test_dir } = setup(&guard.store());
    run_on_real_loop(async move {
        assert_rejects_with_code(provider.read_file(&file_uri("/dev/null"), false).await, FileSystemProviderErrorCode::Unavailable);
    });
}

/// Mata o processo filho ao sair, mesmo se o teste falhar.
struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "só no Windows: o caso é test.skip nas outras plataformas")]
fn rename_fails_quickly_when_a_folder_is_in_use_by_another_process() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Setup { provider, test_dir } = setup(&guard.store());
    let folder_path = test_dir.join("folder");
    std::fs::create_dir(&folder_path).unwrap();

    // On Windows, a process whose working directory is the folder prevents renaming it. O processo segura
    // o diretório de trabalho desde que é criado, então não precisa do aperto de mão do original.
    let _child = KillOnDrop(Command::new("ping").args(["-n", "60", "127.0.0.1"]).current_dir(&folder_path).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("ping"));

    run_on_real_loop(async move {
        let start = Instant::now();
        let error = provider
            .rename(&file_uri(&folder_path), &test_dir.uri("renamed"), &FileOverwriteOptions { overwrite: false })
            .await
            .expect_err("renomear uma pasta em uso devia falhar");
        assert!(error.to_string().contains("in use by another application"), "erro: {error}");
        assert!(start.elapsed() < Duration::from_secs(10), "renaming a folder that is in use should not retry for a long time");
    });
}
