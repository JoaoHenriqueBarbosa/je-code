//! Porte de `src/vs/platform/policy/common/fileManagedSettingsService.ts`: as configurações gerenciadas de um
//! arquivo (`managed-settings.json`), relidas (com um atraso de 500 ms) quando o arquivo muda.
//!
//! Decisões de tradução:
//! - O estado e o `Barrier` ficam em células compartilhadas, para o `initialize()` (que só tem `&self`) devolver
//!   o valor de depois da espera, e `refresh` e `scheduleRefresh` são métodos de `Rc<Self>`, como o
//!   [`FilePolicyService`](super::file_policy_service::FilePolicyService).
//! - `readFile(file, { limits: { size } })` é `ReadFileOptions.stream.limit_size`.
//! - `JSON.parse` é `json_parse` (o `SyntaxError` do original), e o documento lido é um [`Dynamic`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::{Barrier, ThrottledDelayer, async_fn};
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, is_cancellation_error};
use je_base::event::{Emitter, Event};
use je_base::jsonc::json_parse;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::objects::equals;
use je_base::promise::Promise;
use je_base::types::is_object;
use je_base::uri::URI;

use super::copilot_managed_settings::{IFileManagedSettingsService, ManagedSettingsData, RawManagedSettingsData, empty_raw_managed_settings, normalize_managed_settings};
use crate::files::file_service::FileService;
use crate::files::files::{FileOperationError, FileOperationResult, FileReadStreamOptions, ReadFileOptions};
use crate::log::{LogArg, LogMessage, LogServiceRef};

/// Upper bound on the size of `managed-settings.json` we will read into the main process. A
/// legitimate settings file is a few KB; capping the read keeps a malformed or hostile file on the
/// (externally writable, privileged) well-known path from OOM-ing the main process. `readFile`
/// throws `FILE_TOO_LARGE` past this, which `refresh()` treats like any other read failure.
const MANAGED_SETTINGS_MAX_FILE_SIZE: u64 = 1024 * 1024; // 1 MB

/// `FileManagedSettingsService`.
pub struct FileManagedSettingsService {
    base: Rc<Disposable>,
    file: URI,
    file_service: Rc<FileService>,
    log_service: LogServiceRef,
    raw_managed_settings: Rc<RefCell<RawManagedSettingsData>>,
    managed_settings: Rc<RefCell<ManagedSettingsData>>,
    on_did_change_raw_managed_settings: Rc<Emitter<RawManagedSettingsData>>,
    on_did_change_managed_settings: Rc<Emitter<ManagedSettingsData>>,
    throttled_delayer: Rc<ThrottledDelayer<()>>,
    initialized: Barrier,
}

impl FileManagedSettingsService {
    pub fn new(file: URI, file_service: Rc<FileService>, log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_raw_managed_settings: Rc<Emitter<RawManagedSettingsData>> = Emitter::new();
        base.register(on_did_change_raw_managed_settings.clone());
        let on_did_change_managed_settings: Rc<Emitter<ManagedSettingsData>> = Emitter::new();
        base.register(on_did_change_managed_settings.clone());
        let throttled_delayer = Rc::new(ThrottledDelayer::new(500));
        base.register(throttled_delayer.clone());

        let this = Rc::new(Self {
            base,
            file: file.clone(),
            file_service: file_service.clone(),
            log_service,
            raw_managed_settings: Rc::new(RefCell::new(empty_raw_managed_settings())),
            managed_settings: Rc::new(RefCell::new(ManagedSettingsData::new())),
            on_did_change_raw_managed_settings,
            on_did_change_managed_settings,
            throttled_delayer,
            initialized: Barrier::new(),
        });

        let watched = file.clone();
        let on_did_change_file = file_service.on_did_files_change().filter(move |event| event.affects(&watched, &[]), None);
        this.base.register(file_service.watch(&file, None));
        let weak = Rc::downgrade(&this);
        this.base.register(on_did_change_file.listen(move |_| {
            if let Some(service) = weak.upgrade() {
                service.schedule_refresh(None);
            }
        }));

        this.schedule_refresh(Some(0));
        this
    }

    fn schedule_refresh(self: &Rc<Self>, delay: Option<u64>) {
        let service = self.clone();
        let _ = self.throttled_delayer.trigger(move |_| service.refresh(), delay).then_result({
            let service = self.clone();
            move |result| {
                match result {
                    Ok(()) => service.initialized.open(),
                    Err(error) => {
                        if !is_cancellation_error(&error) {
                            service.log_service.error_args(&LogMessage::from("[FileManagedSettingsService] Failed to schedule managed-settings refresh"), &[LogArg::Error(error)]);
                            service.initialized.open();
                        }
                    }
                }
                Ok(())
            }
        });
    }

    /// Lê e interpreta o arquivo; qualquer falha de leitura ou de formato vira "sem configurações".
    async fn read(&self) -> Result<Dynamic, Error> {
        let options = ReadFileOptions { stream: FileReadStreamOptions { limit_size: Some(MANAGED_SETTINGS_MAX_FILE_SIZE), ..Default::default() }, ..Default::default() };
        let content = self.file_service.read_file(&self.file, Some(options), None).await?;
        let units: Vec<u16> = content.value.to_string().encode_utf16().collect();
        let parsed = json_parse(&units).map_err(|error| Error::named("SyntaxError", error.message))?;
        Ok(Dynamic::from_json(&parsed))
    }

    fn refresh(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let previous_raw = this.raw_managed_settings.borrow().clone();
            let previous = this.managed_settings.borrow().clone();

            match this.read().await {
                Ok(parsed) if is_object(&parsed) => {
                    let log_service = this.log_service.clone();
                    let managed_settings = normalize_managed_settings(&parsed, Some(&move |message: &str| log_service.warn(&format!("[FileManagedSettingsService] {message}"))));
                    *this.raw_managed_settings.borrow_mut() = parsed;
                    *this.managed_settings.borrow_mut() = managed_settings;
                }
                Ok(_) => {
                    this.log_service.warn("[FileManagedSettingsService] managed-settings.json is not a JSON object");
                    this.clear();
                }
                Err(error) => {
                    if FileOperationError::from_error(&error).map(|e| e.file_operation_result) != Some(FileOperationResult::FileNotFound) {
                        this.log_service.error_args(&LogMessage::from("[FileManagedSettingsService] Failed to read managed-settings.json"), &[LogArg::Error(error)]);
                    }
                    this.clear();
                }
            }

            let raw = this.raw_managed_settings.borrow().clone();
            if !equals(&previous_raw, &raw) {
                this.on_did_change_raw_managed_settings.fire(raw);
            }
            let managed_settings = this.managed_settings.borrow().clone();
            if previous != managed_settings {
                this.on_did_change_managed_settings.fire(managed_settings);
            }
            Ok(())
        })
    }

    /// `this._rawManagedSettings = {}; this._managedSettings = {};`
    fn clear(&self) {
        *self.raw_managed_settings.borrow_mut() = empty_raw_managed_settings();
        *self.managed_settings.borrow_mut() = ManagedSettingsData::new();
    }
}

impl IFileManagedSettingsService for FileManagedSettingsService {
    fn raw_managed_settings(&self) -> RawManagedSettingsData {
        self.raw_managed_settings.borrow().clone()
    }

    fn managed_settings(&self) -> ManagedSettingsData {
        self.managed_settings.borrow().clone()
    }

    fn on_did_change_raw_managed_settings(&self) -> Event<RawManagedSettingsData> {
        self.on_did_change_raw_managed_settings.event()
    }

    fn on_did_change_managed_settings(&self) -> Event<ManagedSettingsData> {
        self.on_did_change_managed_settings.event()
    }

    fn initialize(&self) -> Promise<ManagedSettingsData> {
        let managed_settings = self.managed_settings.clone();
        self.initialized.wait().then(move |_| managed_settings.borrow().clone())
    }
}

impl IDisposable for FileManagedSettingsService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
