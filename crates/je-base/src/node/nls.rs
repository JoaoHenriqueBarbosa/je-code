//! Porte de `src/vs/base/node/nls.ts` e dos tipos de configuração de NLS de `src/vs/nls.ts`
//! (`INLSConfiguration`, `INLSLanguagePackConfiguration`, `ILanguagePack`, `ILanguagePacks`): resolve a
//! configuração de idioma do produto e gera o cache de mensagens traduzidas a partir de um language pack.
//!
//! Notas de tradução:
//! - o original é assíncrono sobre `fs.promises`; aqui as leituras e escritas são bloqueantes (`std::fs`), como em
//!   `node::zip`. O `touch` do cache, que o original dispara sem esperar, é feito na hora e o erro é ignorado;
//! - o `process.env['VSCODE_DEV']` vem do [`Environment`] recebido, pois mexer no ambiente do processo exige `unsafe`;
//! - o `console.error` é `eprintln!`;
//! - o JSON dos arquivos é lido de forma tolerante, como o `JSON.parse` do original: o que não tem o formato esperado
//!   cai no `catch` e devolve a configuração padrão.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::Value;

use crate::path::{Environment, PathModule};
use crate::performance::mark;

/// `INLSLanguagePackConfiguration`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NlsLanguagePackConfiguration {
    /// O arquivo de configuração das traduções, com ponteiros para os pacotes de mensagens do `main` e das extensões.
    pub translations_config_file: String,
    /// O arquivo com as traduções deste language pack como vetor plano de textos.
    pub messages_file: String,
    /// O arquivo que sinaliza um language pack corrompido: o cache é recriado na próxima partida.
    pub corrupt_marker_file: String,
}

/// `INLSConfiguration`. Os campos `locale` em diante são relíquias do `vscode-nls`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NlsConfiguration {
    pub user_locale: String,
    pub os_locale: String,
    pub resolved_language: String,
    pub language_pack: Option<NlsLanguagePackConfiguration>,
    pub default_messages_file: String,
    pub locale: String,
    pub available_languages: IndexMap<String, String>,
    pub language_pack_support: Option<bool>,
    pub language_pack_id: Option<String>,
    pub translations_config_file: Option<String>,
    pub cache_root: Option<String>,
    pub resolved_language_pack_core_location: Option<String>,
    pub corrupted_file: Option<String>,
}

/// `ILanguagePack.extensions[n]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LanguagePackExtension {
    #[serde(rename = "extensionIdentifier")]
    pub extension_identifier: ExtensionIdentifier,
    pub version: String,
}

/// `{ id, uuid? }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExtensionIdentifier {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
}

/// `ILanguagePack`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LanguagePack {
    pub hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub extensions: Vec<LanguagePackExtension>,
    pub translations: IndexMap<String, Option<String>>,
}

/// `ILanguagePacks`: o conteúdo do `languagepacks.json`, por idioma.
pub type LanguagePacks = IndexMap<String, Option<LanguagePack>>;

/// `IResolveNLSConfigurationContext`.
#[derive(Clone, Debug, Default)]
pub struct ResolveNlsConfigurationContext {
    /// Onde ficam o `nls.messages.json` e o `nls.keys.json`.
    pub nls_metadata_path: String,
    /// Identidade pré-calculada do commit e das tabelas de NLS, fornecida pelos produtos empacotados.
    pub nls_metadata_hash: Option<String>,
    /// O diretório de dados do usuário, usado como cache dos language packs convertidos.
    pub user_data_path: String,
    /// O commit da aplicação; `None` quando não é um build.
    pub commit: Option<String>,
    /// O idioma do `argv.json` ou de `app.getLocale()`.
    pub user_locale: String,
    /// O idioma do SO.
    pub os_locale: String,
}

/// `JS truthiness` de um valor JSON.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// `resolveNLSConfiguration`.
pub fn resolve_nls_configuration(context: &ResolveNlsConfigurationContext, env: &dyn Environment) -> NlsConfiguration {
    let ResolveNlsConfigurationContext { user_locale, os_locale, user_data_path, commit, nls_metadata_path, .. } = context;
    mark("code/willGenerateNls", None);

    let dev = env.env_var("VSCODE_DEV").is_some_and(|v| !v.is_empty());
    let commit = match commit {
        Some(commit) if !commit.is_empty() && !dev && user_locale != "pseudo" && !user_locale.starts_with("en") && !user_data_path.is_empty() => commit,
        _ => return default_nls_configuration(user_locale, os_locale, nls_metadata_path, env),
    };

    match generate(context, commit, env) {
        Ok(Some(result)) => result,
        Ok(None) => default_nls_configuration(user_locale, os_locale, nls_metadata_path, env),
        Err(error) => {
            eprintln!("Generating translation files failed. {error}");
            default_nls_configuration(user_locale, os_locale, nls_metadata_path, env)
        }
    }
}

/// O corpo do `try` do original: `Ok(None)` é cada `return defaultNLSConfiguration(...)` interno.
fn generate(context: &ResolveNlsConfigurationContext, commit: &str, env: &dyn Environment) -> Result<Option<NlsConfiguration>, String> {
    let path = PathModule::native(env);
    let ResolveNlsConfigurationContext { user_locale, os_locale, user_data_path, nls_metadata_path, nls_metadata_hash, .. } = context;

    let Some(language_packs) = get_language_pack_configurations(user_data_path, env) else {
        return Ok(None);
    };
    let Some(resolved_language) = resolve_language_pack_language(&language_packs, user_locale) else {
        return Ok(None);
    };

    let language_pack = &language_packs[&resolved_language];
    let main_language_pack_path = language_pack
        .get("translations")
        .filter(|t| is_truthy(t))
        .and_then(|t| t.get("vscode"))
        .and_then(Value::as_str);
    let (Some(hash), Some(main_language_pack_path)) = (language_pack.get("hash").and_then(Value::as_str), main_language_pack_path) else {
        return Ok(None);
    };
    if !exists(main_language_pack_path) {
        return Ok(None);
    }

    let language_pack_id = format!("{hash}.{resolved_language}");
    let global_language_pack_cache_path = path.join(&[user_data_path, "clp", &language_pack_id]);
    let commit_language_pack_cache_path = path.join(&[&global_language_pack_cache_path, nls_metadata_hash.as_deref().unwrap_or(commit)]);
    let language_pack_messages_file = path.join(&[&commit_language_pack_cache_path, "nls.messages.json"]);
    let translations_config_file = path.join(&[&global_language_pack_cache_path, "tcf.json"]);
    let language_pack_corrupt_marker_file = path.join(&[&global_language_pack_cache_path, "corrupted.info"]);

    if exists(&language_pack_corrupt_marker_file) {
        // apaga a pasta de cache corrompida
        let _ = fs::remove_dir_all(&global_language_pack_cache_path);
    }

    let result = NlsConfiguration {
        user_locale: user_locale.clone(),
        os_locale: os_locale.clone(),
        resolved_language: resolved_language.clone(),
        default_messages_file: path.join(&[nls_metadata_path, "nls.messages.json"]),
        language_pack: Some(NlsLanguagePackConfiguration {
            translations_config_file: translations_config_file.clone(),
            messages_file: language_pack_messages_file.clone(),
            corrupt_marker_file: language_pack_corrupt_marker_file.clone(),
        }),

        // NLS: as propriedades abaixo são relíquias do `vscode-nls`, só ele as usa e estão obsoletas
        locale: user_locale.clone(),
        available_languages: IndexMap::from([("*".to_owned(), resolved_language.clone())]),
        language_pack_support: Some(true),
        language_pack_id: Some(language_pack_id),
        translations_config_file: Some(translations_config_file.clone()),
        cache_root: Some(global_language_pack_cache_path),
        resolved_language_pack_core_location: Some(commit_language_pack_cache_path.clone()),
        corrupted_file: Some(language_pack_corrupt_marker_file),
    };

    if exists(&language_pack_messages_file) {
        // Não é esperado e o erro não importa: pouco mal se não der para tocar
        touch(&commit_language_pack_cache_path);
        mark("code/didGenerateNls", None);
        return Ok(Some(result));
    }

    let read_json = |file: &str| -> Result<Value, String> {
        let content = fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("{file}: {e}"))
    };
    let nls_default_keys = read_json(&path.join(&[nls_metadata_path, "nls.keys.json"]))?;
    let nls_default_messages = read_json(&path.join(&[nls_metadata_path, "nls.messages.json"]))?;
    let nls_packdata = read_json(main_language_pack_path)?;

    // As mensagens de NLS são esperadas num vetor plano na ordem em que o build as produziu. O `nls.keys.json` dá a
    // ordem certa, e a mensagem de cada chave é buscada na tradução. Sem tradução, vale a mensagem padrão.
    let contents = nls_packdata.get("contents").ok_or("Cannot read properties of undefined (reading 'contents')")?;
    let default_keys = nls_default_keys.as_array().ok_or("nlsDefaultKeys is not iterable")?;
    let default_messages = nls_default_messages.as_array();

    let mut nls_result: Vec<Value> = Vec::new();
    let mut nls_index = 0;
    for entry in default_keys {
        let module_id = entry.get(0).and_then(Value::as_str).ok_or("invalid nls.keys.json entry")?;
        let nls_keys = entry.get(1).and_then(Value::as_array).ok_or("invalid nls.keys.json entry")?;
        let module_translations = contents.get(module_id);
        for nls_key in nls_keys {
            let nls_key = nls_key.as_str().unwrap_or_default();
            let translated = module_translations.and_then(|m| m.get(nls_key)).filter(|v| is_truthy(v));
            let fallback = default_messages.and_then(|m| m.get(nls_index)).cloned().unwrap_or(Value::Null);
            nls_result.push(translated.cloned().unwrap_or(fallback));
            nls_index += 1;
        }
    }

    fs::create_dir_all(&commit_language_pack_cache_path).map_err(|e| e.to_string())?;
    fs::write(&language_pack_messages_file, serde_json::to_string(&nls_result).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::write(&translations_config_file, serde_json::to_string(language_pack.get("translations").unwrap_or(&Value::Null)).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    mark("code/didGenerateNls", None);
    Ok(Some(result))
}

/// O `languagepacks.json` guarda os metadados de todos os language packs instalados por idioma: para o núcleo
/// (`vscode`) e para cada extensão suportada, aponta os arquivos de tradução. É atualizado a cada instalação ou remoção.
/// Sem arquivo legível não há configuração de language pack.
fn get_language_pack_configurations(user_data_path: &str, env: &dyn Environment) -> Option<serde_json::Map<String, Value>> {
    let config_file = PathModule::native(env).join(&[user_data_path, "languagepacks.json"]);
    let content = fs::read_to_string(config_file).ok()?;
    match serde_json::from_str::<Value>(&content).ok()? {
        Value::Object(packs) => Some(packs),
        _ => None,
    }
}

fn resolve_language_pack_language(language_packs: &serde_json::Map<String, Value>, locale: &str) -> Option<String> {
    let mut locale = locale;
    while !locale.is_empty() {
        if language_packs.get(locale).is_some_and(is_truthy) {
            return Some(locale.to_owned());
        }
        match locale.rfind('-') {
            Some(index) if index > 0 => locale = &locale[..index],
            _ => return None,
        }
    }
    None
}

fn default_nls_configuration(user_locale: &str, os_locale: &str, nls_metadata_path: &str, env: &dyn Environment) -> NlsConfiguration {
    mark("code/didGenerateNls", None);

    NlsConfiguration {
        user_locale: user_locale.to_owned(),
        os_locale: os_locale.to_owned(),
        resolved_language: "en".to_owned(),
        language_pack: None,
        default_messages_file: PathModule::native(env).join(&[nls_metadata_path, "nls.messages.json"]),

        // NLS: as duas abaixo são relíquias do `vscode-nls`, só ele as usa e estão obsoletas
        locale: user_locale.to_owned(),
        available_languages: IndexMap::new(),
        language_pack_support: None,
        language_pack_id: None,
        translations_config_file: None,
        cache_root: None,
        resolved_language_pack_core_location: None,
        corrupted_file: None,
    }
}

// region fs helpers

fn touch(path: &str) {
    if let Ok(file) = fs::File::open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

// endregion
