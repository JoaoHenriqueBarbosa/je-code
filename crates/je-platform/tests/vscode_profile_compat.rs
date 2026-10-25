//! Retrocompatibilidade com o perfil real do VS Code: o je-code precisa ler os MESMOS diretórios e arquivos
//! do usuário (`settings.json` pelo `ConfigurationService`, `state.vscdb` global pelo serviço de
//! armazenamento) sem erro.
//!
//! O caso `vscode_profile_compat` é `#[ignore]` e lê o caminho de uma CÓPIA do diretório de dados do usuário do
//! VS Code (o `Code`, que contém `User/`) da variável de ambiente `JE_USER_DATA_DIR`; o caminho nunca fica no
//! código. Nada é escrito no perfil de origem: antes de qualquer escrita o teste copia os arquivos que usa
//! (`User/settings.json`, `User/globalStorage/state.vscdb` com `-wal` e `-shm`, e `User/globalStorage/storage.json`)
//! para um diretório temporário, e todas as leituras e escritas acontecem lá. Para rodar:
//! `JE_USER_DATA_DIR=<cópia ou perfil> cargo test -p je-platform --test vscode_profile_compat -- --ignored`.
//!
//! O caso `vscode_profile_compat_synthetic` roda sempre: monta um perfil sintético (um `settings.json` com
//! comentários e um `state.vscdb` escrito pelo próprio serviço de armazenamento) e aplica as mesmas
//! verificações, para o teste não apodrecer quando não há um perfil real à mão.

mod common;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use common::disk::{get_random_test_path, run_on_real_loop};
use common::lifecycle_main_service::TestLifecycleMainService;
use common::storage_main::{StorageMainServiceSetup, create_storage_main_service};
use je_base::dynamic::Dynamic;
use je_base::json::{self, ParseOptions, Value};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_platform::configuration::service::ConfigurationService;
use je_platform::configuration::{ConfigurationOverrides, ConfigurationValue};
use je_platform::environment::argv::{IgnoringReporter, OPTIONS, parse_args};
use je_platform::environment::environment::{EnvironmentService, NativeEnvironmentServiceApi};
use je_platform::environment::native::NativeEnvironmentService;
use je_platform::environment::native_parsed_args::NativeParsedArgs;
use je_platform::files::disk::DiskFileSystemProvider;
use je_platform::files::file_service::FileService;
use je_platform::js::from_json;
use je_platform::log::{LogLevel, LogServiceRef, MessageLogger, MessageSink};
use je_platform::policy::NullPolicyService;
use je_platform::product::ProductService;
use je_platform::resources::{fs_path, uri_file};
use je_platform::storage::{SqliteStorageService, StorageScope, StorageService, StorageServiceOptions, StorageTarget};
use je_platform::user_data_profile::IUserDataProfilesService;
use je_storage::StorageValue;

/// Um diretório temporário que some ao fim do teste.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = get_random_test_path(&["vsctests", name]);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// O destino do logger de teste: guarda o que foi registrado como aviso ou erro.
struct Recorded {
    problems: Rc<RefCell<Vec<String>>>,
}

impl MessageSink for Recorded {
    fn log(&self, level: LogLevel, message: &str) {
        if matches!(level, LogLevel::Warning | LogLevel::Error) {
            self.problems.borrow_mut().push(message.to_owned());
        }
    }
}

fn recording_log() -> (LogServiceRef, Rc<RefCell<Vec<String>>>) {
    let problems: Rc<RefCell<Vec<String>>> = Rc::default();
    (MessageLogger::new(Recorded { problems: problems.clone() }), problems)
}

/// Copia, só leitura na origem, o que o teste usa do perfil para `destination/User/...`.
fn copy_profile(source: &Path, destination: &Path) {
    let source_user = if source.join("User").is_dir() { source.join("User") } else { source.to_path_buf() };
    let destination_user = destination.join("User");
    let files = [
        "settings.json",
        "globalStorage/state.vscdb",
        "globalStorage/state.vscdb-wal",
        "globalStorage/state.vscdb-shm",
        "globalStorage/storage.json",
    ];
    for relative in files {
        let from = source_user.join(relative);
        if from.is_file() {
            let to = destination_user.join(relative);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

/// As propriedades de primeiro nível do arquivo (sem os blocos `[linguagem]`), com o valor que o JSON tem.
fn top_level_settings(text: &str) -> Vec<(String, Dynamic)> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut errors = Vec::new();
    match json::parse(&units, &mut errors, ParseOptions { allow_trailing_comma: true, allow_empty_content: true, ..Default::default() }) {
        Some(Value::Object(object)) => object
            .iter()
            .map(|(key, value)| (key.to_string_lossy(), from_json(value)))
            .filter(|(key, _)| !key.starts_with('['))
            .collect(),
        _ => Vec::new(),
    }
}

/// As verificações sobre a cópia do perfil em `profile`.
async fn verify(profile: PathBuf) {
    let (log, problems) = recording_log();

    // O ambiente: os mesmos diretórios do VS Code a partir do `--user-data-dir` (o compartilhado também fica na cópia).
    let argv = vec!["--user-data-dir".to_owned(), profile.to_string_lossy().into_owned(), "--shared-data-dir".to_owned(), profile.join("shared").to_string_lossy().into_owned()];
    let args = NativeParsedArgs::from_dynamic(&parse_args(&argv, &OPTIONS, &IgnoringReporter));
    let environment = Rc::new(NativeEnvironmentService::native(args, ProductService::default(), "/app").unwrap());
    assert_eq!(environment.user_data_path(), profile.to_string_lossy());
    assert_eq!(fs_path(&environment.app_settings_home()), profile.join("User").to_string_lossy());
    assert!(fs_path(&environment.state_resource()).ends_with("User/globalStorage/storage.json".replace('/', std::path::MAIN_SEPARATOR_STR).as_str()));
    assert!(fs_path(&environment.workspace_storage_home()).ends_with("workspaceStorage"));

    // O `settings.json`, pelo ConfigurationService sobre o provider de disco.
    let file_service = FileService::new(log.clone());
    let provider = DiskFileSystemProvider::new(log.clone(), None);
    let registration = file_service.register_provider("file", provider.clone()).unwrap();
    let settings_path = profile.join("User").join("settings.json");
    let settings_resource = uri_file(&settings_path.to_string_lossy());
    let original = std::fs::read_to_string(&settings_path).unwrap_or_default();

    let configuration = ConfigurationService::new(settings_resource.clone(), file_service.clone(), Rc::new(NullPolicyService), log.clone());
    configuration.initialize().await.unwrap();
    assert!(matches!(configuration.get_value(None, None), Dynamic::Object(_)));
    for (key, expected) in top_level_settings(&original) {
        let inspect = configuration.inspect(&key, &ConfigurationOverrides::default());
        assert!(je_base::objects::equals(&inspect.user_value(), &expected), "a chave {key} do settings.json não foi carregada como está no arquivo: {:?}", inspect.user_value());
    }

    // `updateValue` escreve na cópia e preserva os comentários.
    configuration.update_value("jeCompat.probe", Dynamic::string("valor"), None, None).await.unwrap();
    let written = std::fs::read_to_string(&settings_path).unwrap();
    assert!(written.contains("\"jeCompat.probe\""));
    for line in original.lines().filter(|line| line.trim_start().starts_with("//")) {
        assert!(written.contains(line), "comentário perdido: {line}");
    }
    assert_eq!(configuration.get_value(Some("jeCompat.probe"), None), Dynamic::string("valor"));
    configuration.update_value("jeCompat.probe", Dynamic::Undefined, None, None).await.unwrap();
    assert!(!std::fs::read_to_string(&settings_path).unwrap().contains("jeCompat.probe"));
    configuration.dispose();

    // O `state.vscdb` global, pelo serviço de armazenamento (sobre o `StorageMainService`, dono das conexões).
    let lifecycle_main_service = TestLifecycleMainService::new();
    let disposables = DisposableStore::new();
    let StorageMainServiceSetup { storage_main_service, user_data_profiles_service } =
        create_storage_main_service(&disposables, None, environment.clone(), file_service.clone(), log.clone(), lifecycle_main_service.clone());
    let storage = SqliteStorageService::new(StorageServiceOptions {
        storage_main_service,
        user_data_profiles_service: user_data_profiles_service.as_service(),
        current_profile: user_data_profiles_service.default_profile(),
        initial_workspace: None,
    });
    let service = storage.service();
    storage.initialize().await.unwrap();
    let keys: Vec<String> = [StorageTarget::User, StorageTarget::Machine].into_iter().flat_map(|target| service.keys(StorageScope::Application, target)).collect();
    // O VS Code deixa chaves no mapa de alvos (`__$__targetStorageMarker`) depois de removê-las: o que importa é
    // que o mapa foi lido e que as chaves vivas têm valor.
    assert!(keys.is_empty() || keys.iter().any(|key| service.get(key, StorageScope::Application, None).is_some()), "nenhuma das {} chaves tem valor", keys.len());
    // Uma escrita e uma leitura de volta, na cópia.
    service.store("jeCompat.probe", StorageValue::String("valor".to_owned()), StorageScope::Application, StorageTarget::Machine);
    assert_eq!(service.get("jeCompat.probe", StorageScope::Application, None), Some("valor".to_owned()));
    storage.close().await.unwrap();
    lifecycle_main_service.fire_on_will_shutdown().await.unwrap();
    storage.dispose();
    disposables.dispose();

    registration.dispose();
    provider.dispose();
    file_service.dispose();

    assert!(problems.borrow().is_empty(), "o carregamento do perfil registrou problemas: {:?}", problems.borrow());
}

#[test]
#[ignore = "precisa de JE_USER_DATA_DIR apontando para uma cópia do perfil do VS Code"]
fn vscode_profile_compat() {
    let source = PathBuf::from(std::env::var("JE_USER_DATA_DIR").expect("defina JE_USER_DATA_DIR com o diretório de dados do VS Code (o que contém User/)"));
    let temp = TempDir::new("profilecompat");
    copy_profile(&source, &temp.0);
    let profile = temp.0.clone();
    run_on_real_loop(verify(profile));
}

#[test]
fn vscode_profile_compat_synthetic() {
    let temp = TempDir::new("profilesynthetic");
    let user = temp.0.join("User");
    std::fs::create_dir_all(user.join("globalStorage")).unwrap();
    std::fs::write(
        user.join("settings.json"),
        "{\n\t// tamanho da fonte\n\t\"editor.fontSize\": 14,\n\t\"files.autoSave\": \"afterDelay\", // salvar sozinho\n\t\"[markdown]\": {\n\t\t\"editor.wordWrap\": \"on\"\n\t},\n}\n",
    )
    .unwrap();

    // O `state.vscdb` de origem é escrito pelo próprio serviço de armazenamento (a ida e volta com o disco).
    let source = temp.0.join("source");
    std::fs::create_dir_all(source.join("User").join("globalStorage")).unwrap();
    let source_for_write = source.clone();
    run_on_real_loop(async move {
        let (log, _) = recording_log();
        // Os mesmos diretórios do VS Code a partir do `--user-data-dir` da origem.
        let argv = vec!["--user-data-dir".to_owned(), source_for_write.to_string_lossy().into_owned(), "--shared-data-dir".to_owned(), source_for_write.join("shared").to_string_lossy().into_owned()];
        let args = NativeParsedArgs::from_dynamic(&parse_args(&argv, &OPTIONS, &IgnoringReporter));
        let environment = Rc::new(NativeEnvironmentService::native(args, ProductService::default(), "/app").unwrap());

        let file_service = FileService::new(log.clone());
        let lifecycle_main_service = TestLifecycleMainService::new();
        let disposables = DisposableStore::new();
        let StorageMainServiceSetup { storage_main_service, user_data_profiles_service } =
            create_storage_main_service(&disposables, None, environment, file_service.clone(), log, lifecycle_main_service.clone());
        let storage = SqliteStorageService::new(StorageServiceOptions {
            storage_main_service,
            user_data_profiles_service: user_data_profiles_service.as_service(),
            current_profile: user_data_profiles_service.default_profile(),
            initial_workspace: None,
        });
        storage.initialize().await.unwrap();
        let service = storage.service();
        service.store("telemetry.machineId", StorageValue::String("abc".to_owned()), StorageScope::Application, StorageTarget::Machine);
        service.store("workbench.sideBar.hidden", StorageValue::Bool(false), StorageScope::Application, StorageTarget::User);
        storage.close().await.unwrap();
        lifecycle_main_service.fire_on_will_shutdown().await.unwrap();
        storage.dispose();
        disposables.dispose();
        file_service.dispose();
    });
    std::fs::copy(user.join("settings.json"), source.join("User").join("settings.json")).unwrap();

    copy_profile(&source, &temp.0.join("copy"));
    let profile = temp.0.join("copy");
    run_on_real_loop(async move {
        assert!(profile.join("User/globalStorage/state.vscdb").is_file());
        verify(profile).await;
    });
}
