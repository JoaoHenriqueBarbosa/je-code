//! Teste próprio (sem equivalente no VS Code) da gravação do `ConfigurationService#updateValue`: o
//! `settings.json` é editado com `jsonEdit` e mantém comentários, vírgula final e a indentação; um bloco
//! `[linguagem]` recebe o valor no bloco certo.

mod common;

use std::rc::Rc;

use common::files::async_test;
use common::*;
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_platform::configuration::service::{ConfigurationService, update_overrides_of};
use je_platform::configuration::{ConfigurationOverrides, ConfigurationValue};
use je_platform::files::file_service::FileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::NullLogService;
use je_platform::policy::NullPolicyService;

async fn setup(d: &Rc<je_base::lifecycle::DisposableStore>, content: &str) -> (RegistryGuard, Rc<FileService>, Rc<ConfigurationService>, je_base::uri::URI) {
    let registry = RegistryGuard::new();
    let file_service = d.add_rc(FileService::new(NullLogService::shared()));
    let provider = d.add_rc(InMemoryFileSystemProvider::new());
    d.add(file_service.register_provider("file", provider).unwrap());
    let resource = uri_file("settings.json");
    file_service.write_file(&resource, VSBuffer::from_string(content).into(), None).await.unwrap();
    let service = d.add_rc(ConfigurationService::new(resource.clone(), file_service.clone(), Rc::new(NullPolicyService), NullLogService::shared()));
    service.initialize().await.unwrap();
    (registry, file_service, service, resource)
}

async fn read(file_service: &Rc<FileService>, resource: &je_base::uri::URI) -> String {
    file_service.read_file(resource, None, None).await.unwrap().value.to_string()
}

#[test]
fn update_value_keeps_comments_and_formatting() {
    async_test(|d| async move {
        let (_registry, file_service, service, resource) = setup(&d, "{\n\t// fonte\n\t\"editor.fontSize\": 14, // inline\n}\n").await;

        service.update_value("files.autoSave", s("afterDelay"), None, None).await.unwrap();
        let content = read(&file_service, &resource).await;

        assert!(content.contains("// fonte"), "{content}");
        assert!(content.contains("// inline"), "{content}");
        assert!(content.contains("\"files.autoSave\": \"afterDelay\""), "{content}");
        assert_eq!(service.get_value(Some("editor.fontSize"), None), n(14.0));

        service.update_value("files.autoSave", Dynamic::Undefined, None, None).await.unwrap();
        let content = read(&file_service, &resource).await;
        assert!(!content.contains("files.autoSave"), "{content}");
        assert!(content.contains("// fonte"), "{content}");
    });
}

#[test]
fn update_value_for_a_language_writes_into_the_language_block() {
    async_test(|d| async move {
        let (_registry, file_service, service, resource) = setup(&d, "{}").await;
        let overrides = update_overrides_of(&ConfigurationOverrides::with_identifier("markdown"));

        service.update_value("editor.wordWrap", s("on"), Some(overrides), None).await.unwrap();

        let content = read(&file_service, &resource).await;
        assert!(content.contains("\"[markdown]\""), "{content}");
        let inspect = service.inspect("editor.wordWrap", &ConfigurationOverrides::with_identifier("markdown"));
        assert_eq!(inspect.user_value(), s("on"));
    });
}

#[test]
fn update_value_refuses_to_write_over_a_settings_file_with_errors() {
    async_test(|d| async move {
        let (_registry, file_service, service, resource) = setup(&d, "{ \"a\": }").await;

        let result = service.update_value("b", s("c"), None, None).await;

        assert!(result.unwrap_err().message.contains("Unable to write into the settings file"));
        assert_eq!(read(&file_service, &resource).await, "{ \"a\": }");
    });
}
