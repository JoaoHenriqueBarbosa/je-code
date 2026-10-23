//! Testes de retrocompatibilidade com o disco real, ambos `#[ignore]` (dependem de dados fora do repositório).
//!
//! - `scans_a_copy_of_the_user_extensions_directory`: com `JE_EXTENSIONS_DIR` apontando para uma CÓPIA do diretório de
//!   extensões do usuário (`~/.vscode/extensions`, com o `extensions.json`), o scanner lista todas as extensões
//!   instaladas sem erro, lê os manifestos e os `package.nls.json`, e confere que a contagem bate com a do
//!   `extensions.json`. Rodar com `JE_EXTENSIONS_DIR=<cópia> cargo test -p je-extensions --test compat -- --ignored`.
//! - `scans_the_oracle_builtin_extensions`: escaneia as extensões embutidas do oráculo em
//!   `ref/oracle/VSCode-linux-x64/resources/app/extensions` e confere o total.
//!
//! Nada que identifique a máquina de quem desenvolve entra aqui: os caminhos vêm do ambiente ou do `CARGO_MANIFEST_DIR`.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use je_extensions::fs::{DiskFs, ExtFs};
use je_extensions::manifest::{ExtensionType, TargetPlatform};
use je_extensions::paths::{from_fs_path, join_path};
use je_extensions::profile::UserDataProfiles;
use je_extensions::profile_scanner::ExtensionsProfileScannerService;
use je_extensions::scanner::{
    ExtensionsProduct, ExtensionsScannerService, ExtensionsScannerServiceParts, ScannerEnvironment, ScannerLocations, SystemExtensionsScanOptions, TranslationsMap,
};
use je_platform::log::NullLogService;
use serde_json::Value;

/// O total de extensões embutidas do oráculo medido neste repositório (pastas com `package.json` direto em
/// `resources/app/extensions`). O briefing do trabalho citava 106; a medição do oráculo versionado local dá 96.
const ORACLE_BUILTIN_EXTENSIONS: usize = 96;

struct Scan {
    service: ExtensionsScannerService,
    profile_scanner: Rc<ExtensionsProfileScannerService>,
    profiles: UserDataProfiles,
    // Mantém vivos os diretórios temporários de cache.
    _cache: tempfile::TempDir,
}

fn scanner(system_dir: &str, user_dir: &str, version: &str) -> Scan {
    let cache = tempfile::tempdir().expect("diretório temporário");
    let cache_home = from_fs_path(&cache.path().to_string_lossy());
    let profiles = UserDataProfiles::with_default(user_dir, &cache_home, &cache_home);
    let fs: Rc<dyn ExtFs> = Rc::new(DiskFs);
    let logger = NullLogService::shared();
    let profile_scanner = Rc::new(ExtensionsProfileScannerService::new(user_dir, fs.clone(), profiles.clone(), logger.clone()));
    let service = ExtensionsScannerService::new(ExtensionsScannerServiceParts {
        locations: ScannerLocations {
            system_extensions_location: system_dir.to_owned(),
            user_extensions_location: user_dir.to_owned(),
            extensions_control_location: join_path(&cache_home, &["control.json"]),
            dev_builtin_extensions_location: join_path(&cache_home, &["builtInExtensions"]),
        },
        environment: ScannerEnvironment { is_built: true, language: "en".to_owned(), ..ScannerEnvironment::default() },
        product: ExtensionsProduct { version: version.to_owned(), ..ExtensionsProduct::default() },
        current_profile: profiles.default_profile.clone(),
        profiles: profiles.clone(),
        fs,
        logger,
        profile_scanner: profile_scanner.clone(),
        get_translations: Box::new(|_| TranslationsMap::new()),
    });
    Scan { service, profile_scanner, profiles, _cache: cache }
}

fn has_localizable_text(value: &Value) -> bool {
    match value {
        Value::String(text) => text.len() > 2 && text.starts_with('%') && text.ends_with('%'),
        Value::Array(items) => items.iter().any(has_localizable_text),
        Value::Object(map) => map.values().any(has_localizable_text),
        _ => false,
    }
}

#[test]
#[ignore = "precisa de JE_EXTENSIONS_DIR apontando para uma cópia do diretório de extensões do usuário"]
fn scans_a_copy_of_the_user_extensions_directory() {
    let dir = std::env::var("JE_EXTENSIONS_DIR").expect("JE_EXTENSIONS_DIR");
    let user_dir = from_fs_path(&dir);
    let scan = scanner(&join_path(&user_dir, &["__sem_embutidas__"]), &user_dir, "1.143.1");

    // A lista do perfil, lida do `extensions.json`.
    let manifest_path = join_path(&user_dir, &["extensions.json"]);
    let stored: Value = serde_json::from_slice(&DiskFs.read(&manifest_path).expect("extensions.json")).expect("extensions.json é JSON");
    let stored_count = stored.as_array().expect("extensions.json é um array").len();
    let profile_extensions = scan.profile_scanner.scan_profile_extensions(&scan.profiles.default_profile.extensions_resource, None).expect("o perfil é legível");
    assert_eq!(profile_extensions.len(), stored_count, "o scanner de perfil lê todas as entradas do extensions.json");

    // A varredura do perfil, com os manifestos e os NLS.
    let scanned = scan
        .service
        .scan_user_extensions(&je_extensions::scanner::UserExtensionsScanOptions {
            profile_location: scan.profiles.default_profile.extensions_resource.clone(),
            include_invalid: true,
            ..Default::default()
        })
        .expect("a varredura do perfil não falha");
    // Uma entrada por extensão (a versão mais nova); o `extensions.json` do VS Code também tem uma por extensão.
    assert_eq!(scanned.len(), stored_count, "a contagem da varredura bate com a do extensions.json");

    let mut localized = 0;
    for extension in &scanned {
        assert_eq!(extension.extension_type, ExtensionType::User);
        assert!(!extension.manifest.name.is_empty(), "{}: manifesto sem name", extension.location);
        let raw: Value = serde_json::from_slice(&DiskFs.read(&join_path(&extension.location, &["package.json"])).unwrap()).unwrap();
        if has_localizable_text(&raw) && DiskFs.exists(&join_path(&extension.location, &["package.nls.json"])) {
            let typed = serde_json::to_value(&extension.manifest).unwrap();
            assert!(
                !typed["displayName"].as_str().is_some_and(|s| s.starts_with('%') && s.ends_with('%')),
                "{}: displayName não foi localizado",
                extension.location
            );
            localized += 1;
        }
    }
    let invalid = scanned.iter().filter(|e| !e.is_valid).count();

    // Todas as versões em disco (`scanAllUserExtensions`) cobrem pelo menos as do perfil.
    let all = scan.service.scan_all_user_extensions().expect("a varredura da pasta não falha");
    assert!(all.len() >= scanned.len());
    eprintln!("extensions.json: {stored_count}; perfil: {}; pasta: {}; com NLS localizado: {localized}; inválidas: {invalid}", profile_extensions.len(), all.len());
}

fn oracle_builtin_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ref/oracle/VSCode-linux-x64/resources/app/extensions")
}

#[test]
#[ignore = "precisa do oráculo em ref/oracle (fora do git)"]
fn scans_the_oracle_builtin_extensions() {
    let dir = std::fs::canonicalize(oracle_builtin_dir()).expect("ref/oracle/VSCode-linux-x64/resources/app/extensions");
    let system_dir = from_fs_path(&dir.to_string_lossy());
    let expected = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).filter(|e| e.path().join("package.json").is_file()).count();
    assert_eq!(expected, ORACLE_BUILTIN_EXTENSIONS, "o total de pastas com package.json do oráculo");

    let scan = scanner(&system_dir, &join_path(&system_dir, &["__sem_usuario__"]), "1.143.1");
    let scanned = scan.service.scan_system_extensions(&SystemExtensionsScanOptions::default()).expect("a varredura não falha");

    let invalid: Vec<&str> = scanned.iter().filter(|e| !e.is_valid).map(|e| e.location.as_str()).collect();
    assert_eq!(scanned.len(), expected, "o scanner lista todas as embutidas; inválidas: {invalid:?}");
    assert!(invalid.is_empty(), "todas as embutidas são válidas: {invalid:?}");
    for extension in &scanned {
        assert!(extension.is_builtin);
        assert_eq!(extension.extension_type, ExtensionType::System);
        assert_eq!(extension.target_platform, TargetPlatform::Undefined);
        assert!(!extension.manifest.name.is_empty());
    }
    // As embutidas localizam o `package.nls.json` padrão: nenhum `displayName` fica como `%chave%`.
    let unlocalized: Vec<&str> =
        scanned.iter().filter(|e| e.manifest.display_name.as_deref().is_some_and(|n| n.starts_with('%') && n.ends_with('%'))).map(|e| e.location.as_str()).collect();
    assert!(unlocalized.is_empty(), "displayName sem localizar: {unlocalized:?}");
    eprintln!("embutidas escaneadas: {}", scanned.len());
}
