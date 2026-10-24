//! Porte de `UserSettings` de `src/vs/platform/configuration/common/configurationModels.ts`: lê o
//! `settings.json` do usuário pelo `IFileService` e avisa quando ele muda.
//!
//! O arquivo e o diretório que o contém são observados (o recurso também, por causa de links simbólicos:
//! microsoft/vscode#118134). `loadConfiguration` nunca falha: um arquivo ausente ou ilegível dá um modelo
//! vazio. O `async` do original é uma [`Promise`].

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::event::{Emitter, Event, any};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;

use super::models::{ConfigurationModel, ConfigurationModelParser, ConfigurationParseOptions};
use crate::files::file_service::FileService;
use crate::files::files::FileOperation;
use crate::log::LogServiceRef;

/// `UserSettings`.
pub struct UserSettings {
    base: Rc<Disposable>,
    user_settings_resource: URI,
    parser: RefCell<ConfigurationModelParser>,
    parse_options: RefCell<ConfigurationParseOptions>,
    file_service: Rc<FileService>,
    log_service: LogServiceRef,
    on_did_change: Rc<Emitter<()>>,
}

impl UserSettings {
    pub fn new(
        user_settings_resource: URI,
        parse_options: ConfigurationParseOptions,
        ext_uri: ExtUri,
        file_service: Rc<FileService>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());

        base.register(file_service.watch(&ext_uri.dirname(&user_settings_resource), None));
        // Also listen to the resource incase the resource is a symlink - https://github.com/microsoft/vscode/issues/118134
        base.register(file_service.watch(&user_settings_resource, None));

        let contained = user_settings_resource.clone();
        let files_change = file_service.on_did_files_change().filter(move |event| event.contains(&contained, &[]), None);
        let compared = user_settings_resource.clone();
        let operation = file_service.on_did_run_operation().filter(
            move |event| {
                (event.is_operation(FileOperation::Create)
                    || event.is_operation(FileOperation::Copy)
                    || event.is_operation(FileOperation::Delete)
                    || event.is_operation(FileOperation::Write))
                    && ext_uri.is_equal(Some(&event.resource), Some(&compared), false)
            },
            None,
        );
        let emitter = on_did_change.clone();
        base.register(any(vec![files_change.signal(), operation.signal()]).listen(move |()| emitter.fire(())));

        Rc::new(Self {
            base,
            parser: RefCell::new(ConfigurationModelParser::new(user_settings_resource.to_string(false).to_string_lossy(), log_service.clone())),
            user_settings_resource,
            parse_options: RefCell::new(parse_options),
            file_service,
            log_service,
            on_did_change,
        })
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// `loadConfiguration()`.
    pub fn load_configuration(self: &Rc<Self>) -> Promise<ConfigurationModel> {
        let this = self.clone();
        async_fn(async move {
            match this.file_service.read_file(&this.user_settings_resource, None, None).await {
                Ok(content) => {
                    let text = content.value.to_string();
                    let text = if text.is_empty() { "{}".to_owned() } else { text };
                    let options = this.parse_options.borrow().clone();
                    let mut parser = this.parser.borrow_mut();
                    parser.parse(Some(&text), Some(&options));
                    Ok(parser.configuration_model())
                }
                Err(_) => Ok(ConfigurationModel::create_empty_model(this.log_service.clone())),
            }
        })
    }

    /// `parseOptions` (`protected` no original: a subclasse `ApplicationConfiguration` do workbench o lê).
    pub fn parse_options(&self) -> ConfigurationParseOptions {
        self.parse_options.borrow().clone()
    }

    /// `reparse(parseOptions?)`.
    pub fn reparse(&self, parse_options: Option<ConfigurationParseOptions>) -> ConfigurationModel {
        if let Some(parse_options) = parse_options {
            *self.parse_options.borrow_mut() = parse_options;
        }
        let options = self.parse_options.borrow().clone();
        let mut parser = self.parser.borrow_mut();
        parser.reparse(&options);
        parser.configuration_model()
    }

    /// `getRestrictedSettings()`.
    pub fn get_restricted_settings(&self) -> Vec<je_base::JsString> {
        self.parser.borrow().restricted_configurations().to_vec()
    }
}

impl IDisposable for UserSettings {
    fn dispose(&self) {
        self.base.dispose();
    }
}
