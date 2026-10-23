//! Testes próprios (sem equivalente no VS Code) dos loggers de arquivo: o `FileLoggerService` sobre o
//! `IFileService` (`fileLog.ts`, com o `BufferLogger` guardando as mensagens até o provider existir) e o
//! `LoggerService` do node, que escreve `logs/*.log` (o papel do `SpdLogLogger`, `loggerService.ts`).

mod common;

use common::disk::{get_random_test_path, run_on_real_loop};
use common::files::async_test;
use je_base::async_::timeout;
use je_platform::files::file_service::FileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::file_log::file_logger_service;
use je_platform::log::node::logger_service;
use je_platform::log::{LogLevel, LoggerService, NullLogService, ResourceOrId};
use je_platform::resources::uri_file;

#[test]
fn file_logger_service_buffers_until_the_provider_exists_and_then_writes_the_file() {
    async_test(|d| async move {
        let file_service = d.add_rc(FileService::new(NullLogService::shared()));
        let logs_home = je_base::uri::URI::parse("memory:///logs", false);
        let service = d.add_rc(file_logger_service(LogLevel::Info, logs_home.clone(), file_service.clone()));

        // O provider ainda não existe: as mensagens ficam no buffer.
        let logger = service.create_logger(ResourceOrId::from("main"), None);
        logger.info("before provider");
        logger.debug("below level");

        let provider = d.add_rc(InMemoryFileSystemProvider::new());
        d.add(file_service.register_provider("memory", provider).unwrap());
        logger.info("after provider");
        timeout(500).await.unwrap();

        let resource = je_platform::resources::join_path(&logs_home, &["main.log"]);
        let content = file_service.read_file(&resource, None, None).await.unwrap().value.to_string();
        assert!(content.contains("[info] before provider\n"), "{content}");
        assert!(content.contains("[info] after provider\n"), "{content}");
        assert!(!content.contains("below level"), "{content}");
    });
}

#[test]
fn node_logger_service_writes_files_under_the_logs_folder() {
    let directory = get_random_test_path(&["vsctests", "logs"]);
    let logs = directory.clone();
    run_on_real_loop(async move {
        let service = logger_service(LogLevel::Info, uri_file(&logs.to_string_lossy()));
        let logger = service.create_logger(ResourceOrId::from("renderer"), None);
        logger.info("hello");
        logger.trace("hidden");
        logger.warn("careful");
        logger.flush();
        je_base::lifecycle::IDisposable::dispose(service.as_ref());
    });
    let content = std::fs::read_to_string(directory.join("renderer.log")).unwrap();
    assert!(content.contains("[info] hello\n"), "{content}");
    assert!(content.contains("[warning] careful\n"), "{content}");
    assert!(!content.contains("hidden"), "{content}");
    let _ = std::fs::remove_dir_all(directory);
}
