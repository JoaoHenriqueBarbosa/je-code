//! Porte de `src/vs/platform/extensionManagement/common/extensionTipsService.ts` (`ExtensionTipsService` e
//! `AbstractNativeExtensionTipsService`) e de `node/extensionTipsService.ts`: as dicas de extensão por configuração do
//! workspace e por executável instalado, e o aviso das dicas de executável importantes.
//!
//! Notas de tradução:
//! - **Injeção de dependência.** Os serviços do construtor entram agrupados em [`NativeExtensionTipsServiceParts`]. O
//!   `INativeHostService` só serve aos eventos de janela, que viram [`WindowEvents`].
//! - **Herança.** `AbstractNativeExtensionTipsService extends ExtensionTipsService`: [`NativeExtensionTipsService`] guarda
//!   um [`ExtensionTipsService`] e entrega a ele `getConfigBasedTips`.
//! - **`whenNotInstalled` das dicas de executável.** O original monta as listas de recomendações sem esse campo, então ele é
//!   sempre `undefined` nas dicas devolvidas; o porte repete.
//! - **Caminhos de executável.** `join('/usr/local/bin', exeName)` é a concatenação com `/` (o ramo não-Windows só roda em
//!   sistemas de caminho POSIX); no Windows as variáveis de ambiente de `windowsPath` são lidas de `std::env`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::{async_fn, disposable_timeout};
use je_base::dynamic::Dynamic;
use je_base::event::{self, Event};
use je_base::js_regexp::JsRegExp;
use je_base::lifecycle::{Disposable, IDisposable, MutableDisposable};
use je_base::platform::Platform;
use je_base::product::IConfigBasedExtensionTip as IRawConfigBasedExtensionTip;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionType;
use je_extensions::util::are_same_extensions;
use je_platform::extension_management::extension_tips::{ConfigBasedExtensionTip, ExecutableBasedExtensionTip, IExtensionTipsService};
use je_platform::extension_recommendations::extension_recommendations::{
    ExtensionRecommendationsRequest, IExtensionRecommendationNotificationService, RecommendationSource, RecommendationsNotificationResult, RecommendedExtension,
};
use je_platform::files::i_file_service::IFileService;
use je_platform::product::IProductService;
use je_platform::resources::{join_path, uri_file};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry::ITelemetryService;

use crate::services::extension_management::extension_management::{ExtensionRef, IExtensionManagementService};

// ---------------------------------------------------------------------------------------------------------------------
// Base Extension Tips Service
// ---------------------------------------------------------------------------------------------------------------------

/// `ExtensionTipsService`.
pub struct ExtensionTipsService {
    file_service: Rc<dyn IFileService>,
    all_config_based_tips: IndexMap<String, IRawConfigBasedExtensionTip>,
}

impl ExtensionTipsService {
    /// `constructor(fileService, productService)`.
    pub fn new(file_service: Rc<dyn IFileService>, product_service: &dyn IProductService) -> Self {
        let mut all_config_based_tips = IndexMap::new();
        if let Some(tips) = product_service.config_based_extension_tips() {
            for value in tips.values() {
                all_config_based_tips.insert(value.config_path.clone(), value.clone());
            }
        }
        Self { file_service, all_config_based_tips }
    }

    /// `getValidConfigBasedTips(folder)`.
    fn get_valid_config_based_tips(&self, folder: URI) -> Promise<Vec<ConfigBasedExtensionTip>> {
        let file_service = self.file_service.clone();
        let all = self.all_config_based_tips.clone();
        async_fn(async move {
            let mut result: Vec<ConfigBasedExtensionTip> = Vec::new();
            for (config_path, tip) in &all {
                if let Some(scheme) = &tip.config_scheme {
                    if !scheme.is_empty() && *scheme != folder.scheme().to_string_lossy() {
                        continue;
                    }
                }
                let attempt = async {
                    let content = file_service.read_file(&join_path(&folder, &[config_path]), None, None).await?.value.to_string();
                    let units: Vec<u16> = content.encode_utf16().collect();
                    let mut found = Vec::new();
                    for (key, value) in &tip.recommendations {
                        let matches = match value.content_pattern.as_deref().filter(|pattern| !pattern.is_empty()) {
                            None => true,
                            Some(pattern) => JsRegExp::from_str(pattern, "mig").map_err(|e| je_base::errors::Error::new(format!("{e:?}")))?.exec(&units).is_some(),
                        };
                        if matches {
                            found.push(ConfigBasedExtensionTip {
                                extension_id: key.clone(),
                                extension_name: value.name.clone(),
                                config_name: tip.config_name.clone(),
                                important: value.important.unwrap_or(false),
                                is_extension_pack: value.is_extension_pack.unwrap_or(false),
                                when_not_installed: value.when_not_installed.clone(),
                            });
                        }
                    }
                    Ok::<_, je_base::errors::Error>(found)
                };
                // `try { ... } catch (error) { /* Ignore */ }`: o que já foi lido dessa dica antes da falha fica de fora.
                if let Ok(found) = attempt.await {
                    result.extend(found);
                }
            }
            Ok(result)
        })
    }
}

impl IExtensionTipsService for ExtensionTipsService {
    fn get_config_based_tips(&self, folder: &URI) -> Promise<Vec<ConfigBasedExtensionTip>> {
        self.get_valid_config_based_tips(folder.clone())
    }

    fn get_important_executable_based_tips(&self) -> Promise<Vec<ExecutableBasedExtensionTip>> {
        Promise::resolved(Vec::new())
    }

    fn get_other_executable_based_tips(&self) -> Promise<Vec<ExecutableBasedExtensionTip>> {
        Promise::resolved(Vec::new())
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Native Extension Tips Service
// ---------------------------------------------------------------------------------------------------------------------

const PROMPTED_EXECUTABLE_TIPS_STORAGE_KEY: &str = "extensionTips/promptedExecutableTips";
const LAST_PROMPTED_MEDIUM_IMP_EXE_TIME_STORAGE_KEY: &str = "extensionTips/lastPromptedMediumImpExeTime";
const ONE_HOUR_MS: u64 = 60 * 60 * 1000;
const SEVEN_DAYS_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// Os eventos de janela do `INativeHostService` que o serviço usa.
#[derive(Clone)]
pub struct WindowEvents {
    pub on_did_open_main_window: Event<()>,
    pub on_did_focus_main_window: Event<()>,
}

/// Os serviços do construtor de `AbstractNativeExtensionTipsService`.
pub struct NativeExtensionTipsServiceParts {
    pub user_home: URI,
    pub window_events: WindowEvents,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub extension_management_service: Rc<dyn IExtensionManagementService>,
    pub storage_service: Rc<dyn StorageService>,
    pub extension_recommendation_notification_service: Rc<dyn IExtensionRecommendationNotificationService>,
    pub file_service: Rc<dyn IFileService>,
    pub product_service: Rc<dyn IProductService>,
}

/// `IExeBasedExtensionTips`: `recommendations` guarda `{ extensionId, extensionName, isExtensionPack }`.
#[derive(Clone)]
struct ExeBasedExtensionTips {
    exe_friendly_name: String,
    windows_path: Option<String>,
    recommendations: Vec<(String, String, bool)>,
}

type TipsByExe = IndexMap<String, Vec<ExecutableBasedExtensionTip>>;

/// `AbstractNativeExtensionTipsService` (`node/extensionTipsService.ts` só a instancia).
pub struct NativeExtensionTipsService {
    this: Weak<NativeExtensionTipsService>,
    base: Rc<Disposable>,
    tips: ExtensionTipsService,

    high_importance_executable_tips: IndexMap<String, ExeBasedExtensionTips>,
    medium_importance_executable_tips: IndexMap<String, ExeBasedExtensionTips>,
    all_other_executable_tips: IndexMap<String, ExeBasedExtensionTips>,

    high_importance_tips_by_exe: RefCell<TipsByExe>,
    medium_importance_tips_by_exe: RefCell<TipsByExe>,

    p: NativeExtensionTipsServiceParts,
}

impl NativeExtensionTipsService {
    /// `constructor(userHome, windowEvents, telemetryService, extensionManagementService, storageService, ..., fileService, productService)`.
    pub fn new(parts: NativeExtensionTipsServiceParts) -> Rc<Self> {
        let base = Disposable::new();
        let tips = ExtensionTipsService::new(parts.file_service.clone(), parts.product_service.as_ref());

        let mut high = IndexMap::new();
        let mut medium = IndexMap::new();
        let mut other = IndexMap::new();
        if let Some(exe_tips) = parts.product_service.exe_based_extension_tips() {
            for (key, exe_based_extension_tip) in exe_tips {
                let mut high_importance_recommendations = Vec::new();
                let mut medium_importance_recommendations = Vec::new();
                let mut other_recommendations = Vec::new();
                for (extension_id, value) in &exe_based_extension_tip.recommendations {
                    let entry = (extension_id.clone(), value.name.clone(), value.is_extension_pack.unwrap_or(false));
                    if value.important.unwrap_or(false) {
                        if exe_based_extension_tip.important.unwrap_or(false) {
                            high_importance_recommendations.push(entry);
                        } else {
                            medium_importance_recommendations.push(entry);
                        }
                    } else {
                        other_recommendations.push(entry);
                    }
                }
                let make = |recommendations: Vec<(String, String, bool)>| ExeBasedExtensionTips {
                    exe_friendly_name: exe_based_extension_tip.friendly_name.clone(),
                    windows_path: exe_based_extension_tip.windows_path.clone(),
                    recommendations,
                };
                if !high_importance_recommendations.is_empty() {
                    high.insert(key.clone(), make(high_importance_recommendations));
                }
                if !medium_importance_recommendations.is_empty() {
                    medium.insert(key.clone(), make(medium_importance_recommendations));
                }
                if !other_recommendations.is_empty() {
                    other.insert(key.clone(), make(other_recommendations));
                }
            }
        }

        let this = Rc::new_cyclic(|weak| Self {
            this: weak.clone(),
            base: base.clone(),
            tips,
            high_importance_executable_tips: high,
            medium_importance_executable_tips: medium,
            all_other_executable_tips: other,
            high_importance_tips_by_exe: RefCell::new(IndexMap::new()),
            medium_importance_tips_by_exe: RefCell::new(IndexMap::new()),
            p: parts,
        });

        // 3s has come out to be the good number to fetch and prompt important exe based recommendations.
        // Also fetch important exe based recommendations for reporting telemetry.
        let weak = Rc::downgrade(&this);
        this.base.register(disposable_timeout(
            move || {
                let Some(this) = weak.upgrade() else { return };
                let _ = async_fn(async move {
                    this.collect_tips().await?;
                    this.prompt_high_importance_exe_based_tip();
                    this.prompt_medium_importance_exe_based_tip();
                    Ok(())
                });
            },
            3000,
            None,
        ));
        this
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    /// `const disposable = this._register(new MutableDisposable()); disposable.value = disposableTimeout(() => { disposable.dispose(); action(); }, delay)`.
    fn run_after(&self, delay_ms: u64, action: fn(&Rc<Self>)) {
        let disposable = MutableDisposable::<dyn IDisposable>::new();
        self.base.register(disposable.clone());
        let this = self.rc();
        let handle = disposable.clone();
        disposable.set(Some(disposable_timeout(
            move || {
                handle.dispose();
                action(&this);
            },
            delay_ms,
            None,
        )));
    }

    /// `this._register(Event.once(Event.latch(Event.any(onDidOpenMainWindow, onDidFocusMainWindow)))(() => action()))`.
    fn run_on_active_window_change(&self, action: fn(&Rc<Self>)) {
        let on_active_window_change = event::any(vec![self.p.window_events.on_did_open_main_window.clone(), self.p.window_events.on_did_focus_main_window.clone()]).latch(None).once();
        let this = self.rc();
        self.base.register(on_active_window_change.listen(move |_| action(&this)));
    }

    fn collect_tips(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let high_importance_exe_tips = this.get_valid_executable_based_extension_tips(&this.high_importance_executable_tips).await?;
            let medium_importance_exe_tips = this.get_valid_executable_based_extension_tips(&this.medium_importance_executable_tips).await?;
            let local = this.p.extension_management_service.get_installed().await?;

            *this.high_importance_tips_by_exe.borrow_mut() = this.group_important_tips_by_exe(high_importance_exe_tips, &local);
            *this.medium_importance_tips_by_exe.borrow_mut() = this.group_important_tips_by_exe(medium_importance_exe_tips, &local);
            Ok(())
        })
    }

    fn group_important_tips_by_exe(&self, important_exe_based_tips: Vec<ExecutableBasedExtensionTip>, local: &[ExtensionRef]) -> TipsByExe {
        let mut important_exe_based_recommendations: IndexMap<String, ExecutableBasedExtensionTip> = IndexMap::new();
        for tip in important_exe_based_tips {
            important_exe_based_recommendations.insert(tip.extension_id.to_lowercase(), tip);
        }

        let keys: Vec<String> = important_exe_based_recommendations.keys().cloned().collect();
        let (installed, uninstalled) = Self::group_by_installed(&keys, local);

        // Log installed and uninstalled exe based recommendations.
        for (extension_id, event_name) in installed.iter().map(|id| (id, "exeExtensionRecommendations:alreadyInstalled")).chain(uninstalled.iter().map(|id| (id, "exeExtensionRecommendations:notInstalled"))) {
            if let Some(tip) = important_exe_based_recommendations.get(extension_id) {
                let data = Dynamic::object([("extensionId", Dynamic::string(extension_id)), ("exeName", Dynamic::string(&tip.exe_name))]);
                self.p.telemetry_service.public_log2(event_name, Some(data));
            }
        }

        let prompted_executable_tips = self.get_prompted_executable_tips();
        let mut tips_by_exe: TipsByExe = IndexMap::new();
        for extension_id in &uninstalled {
            if let Some(tip) = important_exe_based_recommendations.get(extension_id) {
                let already_prompted = prompted_executable_tips.get(&tip.exe_name).is_some_and(|ids| ids.contains(&tip.extension_id));
                if !already_prompted {
                    tips_by_exe.entry(tip.exe_name.clone()).or_default().push(tip.clone());
                }
            }
        }
        tips_by_exe
    }

    /// High importance tips are prompted once per restart session.
    fn prompt_high_importance_exe_based_tip(self: &Rc<Self>) {
        let Some((exe_name, tips)) = self.high_importance_tips_by_exe.borrow().first().map(|(exe_name, tips)| (exe_name.clone(), tips.clone())) else { return };
        let this = self.clone();
        let _ = self.prompt_exe_recommendations(tips.clone()).then(move |result| match result {
            RecommendationsNotificationResult::Accepted => this.add_to_recommended_executables(&tips[0].exe_name, &tips),
            RecommendationsNotificationResult::Ignored => {
                this.high_importance_tips_by_exe.borrow_mut().shift_remove(&exe_name);
            }
            // Recommended in incompatible window. Schedule the prompt after active window change.
            RecommendationsNotificationResult::IncompatibleWindow => this.run_on_active_window_change(Self::prompt_high_importance_exe_based_tip),
            // Too many notifications. Schedule the prompt after one hour.
            RecommendationsNotificationResult::TooMany => this.run_after(ONE_HOUR_MS, Self::prompt_high_importance_exe_based_tip),
            RecommendationsNotificationResult::Cancelled => {}
        });
    }

    /// Medium importance tips are prompted once per 7 days.
    fn prompt_medium_importance_exe_based_tip(self: &Rc<Self>) {
        if self.medium_importance_tips_by_exe.borrow().is_empty() {
            return;
        }

        let last_prompted_medium_exe_time = self.get_last_prompted_medium_exe_time();
        let time_since_last_prompt = je_base::runtime::date_now() as f64 - last_prompted_medium_exe_time;
        if time_since_last_prompt < SEVEN_DAYS_MS as f64 {
            // Wait until interval and prompt.
            self.run_after((SEVEN_DAYS_MS as f64 - time_since_last_prompt) as u64, Self::prompt_medium_importance_exe_based_tip);
            return;
        }

        let Some((exe_name, tips)) = self.medium_importance_tips_by_exe.borrow().first().map(|(exe_name, tips)| (exe_name.clone(), tips.clone())) else { return };
        let this = self.clone();
        let _ = self.prompt_exe_recommendations(tips.clone()).then(move |result| match result {
            RecommendationsNotificationResult::Accepted => {
                // Accepted: Update the last prompted time and caches.
                this.update_last_prompted_medium_exe_time(je_base::runtime::date_now() as f64);
                this.medium_importance_tips_by_exe.borrow_mut().shift_remove(&exe_name);
                this.add_to_recommended_executables(&tips[0].exe_name, &tips);

                // Schedule the next recommendation for next interval.
                this.run_after(SEVEN_DAYS_MS, Self::prompt_medium_importance_exe_based_tip);
            }
            RecommendationsNotificationResult::Ignored => {
                // Ignored: Remove from the cache and prompt next recommendation.
                this.medium_importance_tips_by_exe.borrow_mut().shift_remove(&exe_name);
                this.prompt_medium_importance_exe_based_tip();
            }
            RecommendationsNotificationResult::IncompatibleWindow => this.run_on_active_window_change(Self::prompt_medium_importance_exe_based_tip),
            RecommendationsNotificationResult::TooMany => this.run_after(ONE_HOUR_MS, Self::prompt_medium_importance_exe_based_tip),
            RecommendationsNotificationResult::Cancelled => {}
        });
    }

    fn prompt_exe_recommendations(&self, tips: Vec<ExecutableBasedExtensionTip>) -> Promise<RecommendationsNotificationResult> {
        let this = self.rc();
        async_fn(async move {
            let installed: Vec<ExtensionRef> =
                this.p.extension_management_service.get_installed().await?.into_iter().filter(|local| local.extension_type == ExtensionType::User).collect();
            let extensions: Vec<RecommendedExtension> = tips
                .iter()
                .filter(|tip| {
                    tip.when_not_installed
                        .as_ref()
                        .is_none_or(|ids| ids.iter().all(|id| installed.iter().all(|local| !are_same_extensions(&local.identifier, &je_extensions::manifest::ExtensionIdentifierInfo::new(id)))))
                })
                .map(|tip| RecommendedExtension::Gallery(tip.extension_id.to_lowercase()))
                .collect();
            this.p
                .extension_recommendation_notification_service
                .prompt_important_extensions_install_notification(ExtensionRecommendationsRequest {
                    extensions,
                    source: RecommendationSource::Exe,
                    name: tips[0].exe_friendly_name.clone(),
                    search_value: Some(format!("@exe:\"{}\"", tips[0].exe_name)),
                })
                .await
        })
    }

    fn get_last_prompted_medium_exe_time(&self) -> f64 {
        let value = self.p.storage_service.get_number(LAST_PROMPTED_MEDIUM_IMP_EXE_TIME_STORAGE_KEY, StorageScope::Application, None);
        match value {
            Some(value) if value != 0.0 && !value.is_nan() => value,
            _ => {
                let now = je_base::runtime::date_now() as f64;
                self.update_last_prompted_medium_exe_time(now);
                now
            }
        }
    }

    fn update_last_prompted_medium_exe_time(&self, value: f64) {
        self.p.storage_service.store(LAST_PROMPTED_MEDIUM_IMP_EXE_TIME_STORAGE_KEY, value.into(), StorageScope::Application, StorageTarget::Machine);
    }

    fn get_prompted_executable_tips(&self) -> IndexMap<String, Vec<String>> {
        let stored = self.p.storage_service.get(PROMPTED_EXECUTABLE_TIPS_STORAGE_KEY, StorageScope::Application, Some("{}")).unwrap_or_else(|| "{}".to_owned());
        serde_json::from_str(&stored).unwrap_or_default()
    }

    fn add_to_recommended_executables(&self, exe_name: &str, tips: &[ExecutableBasedExtensionTip]) {
        let mut prompted_executable_tips = self.get_prompted_executable_tips();
        prompted_executable_tips.insert(exe_name.to_owned(), tips.iter().map(|tip| tip.extension_id.to_lowercase()).collect());
        let serialized = serde_json::to_string(&prompted_executable_tips).unwrap_or_else(|_| "{}".to_owned());
        self.p.storage_service.store(PROMPTED_EXECUTABLE_TIPS_STORAGE_KEY, serialized.as_str().into(), StorageScope::Application, StorageTarget::User);
    }

    fn group_by_installed(recommendations_to_suggest: &[String], local: &[ExtensionRef]) -> (Vec<String>, Vec<String>) {
        let installed_extensions_ids: Vec<String> = local.iter().map(|i| i.identifier.id.to_lowercase()).collect();
        let mut installed = Vec::new();
        let mut uninstalled = Vec::new();
        for id in recommendations_to_suggest {
            if installed_extensions_ids.contains(&id.to_lowercase()) {
                installed.push(id.clone());
            } else {
                uninstalled.push(id.clone());
            }
        }
        (installed, uninstalled)
    }

    fn get_valid_executable_based_extension_tips(&self, executable_tips: &IndexMap<String, ExeBasedExtensionTips>) -> Promise<Vec<ExecutableBasedExtensionTip>> {
        let file_service = self.p.file_service.clone();
        let user_home_fs_path = je_platform::resources::fs_path(&self.p.user_home);
        let is_windows = Platform::detect().is_windows;
        let executable_tips = executable_tips.clone();
        async_fn(async move {
            let mut result: Vec<ExecutableBasedExtensionTip> = Vec::new();
            let mut checked_executables: IndexMap<String, bool> = IndexMap::new();
            for (exe_name, extension_tip) in &executable_tips {
                if extension_tip.recommendations.is_empty() {
                    continue;
                }

                let mut exe_paths: Vec<String> = Vec::new();
                if is_windows {
                    if let Some(windows_path) = &extension_tip.windows_path {
                        let env = |name: &str| std::env::var(name).unwrap_or_default();
                        exe_paths.push(
                            windows_path
                                .replacen("%USERPROFILE%", &env("USERPROFILE"), 1)
                                .replacen("%ProgramFiles(x86)%", &env("ProgramFiles(x86)"), 1)
                                .replacen("%ProgramFiles%", &env("ProgramFiles"), 1)
                                .replacen("%APPDATA%", &env("APPDATA"), 1)
                                .replacen("%WINDIR%", &env("WINDIR"), 1),
                        );
                    }
                } else {
                    exe_paths.push(format!("/usr/local/bin/{exe_name}"));
                    exe_paths.push(format!("/usr/bin/{exe_name}"));
                    exe_paths.push(format!("{}/{exe_name}", user_home_fs_path.trim_end_matches('/')));
                }

                for exe_path in exe_paths {
                    let exists = match checked_executables.get(&exe_path) {
                        Some(exists) => *exists,
                        None => {
                            let exists = file_service.exists(&uri_file(&exe_path)).await?;
                            checked_executables.insert(exe_path.clone(), exists);
                            exists
                        }
                    };
                    if exists {
                        for (extension_id, extension_name, is_extension_pack) in &extension_tip.recommendations {
                            result.push(ExecutableBasedExtensionTip {
                                extension_id: extension_id.clone(),
                                extension_name: extension_name.clone(),
                                is_extension_pack: *is_extension_pack,
                                exe_name: exe_name.clone(),
                                exe_friendly_name: extension_tip.exe_friendly_name.clone(),
                                windows_path: extension_tip.windows_path.clone(),
                                when_not_installed: None,
                            });
                        }
                    }
                }
            }
            Ok(result)
        })
    }
}

impl IExtensionTipsService for NativeExtensionTipsService {
    fn get_config_based_tips(&self, folder: &URI) -> Promise<Vec<ConfigBasedExtensionTip>> {
        self.tips.get_config_based_tips(folder)
    }

    fn get_important_executable_based_tips(&self) -> Promise<Vec<ExecutableBasedExtensionTip>> {
        let this = self.rc();
        async_fn(async move {
            let mut high_importance_exe_tips = this.get_valid_executable_based_extension_tips(&this.high_importance_executable_tips).await?;
            let medium_importance_exe_tips = this.get_valid_executable_based_extension_tips(&this.medium_importance_executable_tips).await?;
            high_importance_exe_tips.extend(medium_importance_exe_tips);
            Ok(high_importance_exe_tips)
        })
    }

    fn get_other_executable_based_tips(&self) -> Promise<Vec<ExecutableBasedExtensionTip>> {
        self.get_valid_executable_based_extension_tips(&self.all_other_executable_tips)
    }
}

impl IDisposable for NativeExtensionTipsService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
