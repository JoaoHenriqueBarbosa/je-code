//! Porte de `src/vs/platform/policy/common/filePolicyService.ts`: as políticas vêm de um arquivo JSON, relido
//! (com um atraso de 500 ms) quando o arquivo muda.
//!
//! Divergências: as chaves do arquivo cujo valor não é texto, número ou booleano (um objeto, uma lista ou
//! `null`) são ignoradas; o original as guardaria como `PolicyValue` mesmo violando o tipo.

use std::rc::Rc;

use je_base::async_::{ThrottledDelayer, async_fn};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::jsonc::json_parse;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::OrderedMap;
use je_base::promise::Promise;
use je_base::uri::URI;

use super::policy::{AbstractPolicyService, PolicyDefinitions, PolicyService, PolicyUpdater, PolicyValue, PolicyValueSource, PolicyValues, SerializedPolicy};
use crate::files::file_service::FileService;
use crate::files::files::{FileOperationError, FileOperationResult};
use crate::js::from_json;
use crate::log::{LogArg, LogMessage, LogServiceRef};

/// `keysDiff(a, b)`: as chaves cujo valor difere, na ordem de inserção de `a` e depois `b`.
fn keys_diff(a: &OrderedMap<String, PolicyValue>, b: &OrderedMap<String, PolicyValue>) -> Vec<String> {
    let mut keys: Vec<String> = a.keys().cloned().collect();
    for key in b.keys() {
        if !keys.contains(key) {
            keys.push(key.clone());
        }
    }
    keys.into_iter().filter(|key| a.get(key) != b.get(key)).collect()
}

struct FileSource {
    file: URI,
    file_service: Rc<FileService>,
    log_service: LogServiceRef,
}

impl FileSource {
    fn read(self: &Rc<Self>, service: &Rc<AbstractPolicyService>) -> Promise<OrderedMap<String, PolicyValue>> {
        let (this, service) = (self.clone(), service.clone());
        async_fn(async move {
            let mut policies = OrderedMap::new();

            let result: Result<(), Error> = async {
                let content = this.file_service.read_file(&this.file, None, None).await?;
                let units: Vec<u16> = content.value.to_string().encode_utf16().collect();
                let raw = from_json(&json_parse(&units).map_err(|error| Error::named("SyntaxError", error.message))?);

                if !matches!(raw, Dynamic::Object(_)) {
                    return Err(Error::new("Policy file isn't a JSON object"));
                }

                for key in raw.own_keys() {
                    let name = key.to_string_lossy();
                    if service.definition(&name).is_some()
                        && let Some(value) = PolicyValue::from_dynamic(&raw.get(&key))
                    {
                        policies.insert(name, value);
                    }
                }
                Ok(())
            }
            .await;

            if let Err(error) = result
                && FileOperationError::from_error(&error).map(|e| e.file_operation_result) != Some(FileOperationResult::FileNotFound)
            {
                this.log_service.error_args(&LogMessage::from("[FilePolicyService] Failed to read policies"), &[LogArg::Error(error)]);
            }

            Ok(policies)
        })
    }

    fn refresh(self: &Rc<Self>, service: &Rc<AbstractPolicyService>) -> Promise<()> {
        let (this, service) = (self.clone(), service.clone());
        async_fn(async move {
            let policies = this.read(&service).await?;
            let diff = keys_diff(&service.policies(), &policies);
            service.set_policies(policies);

            if !diff.is_empty() {
                service.fire_did_change(diff);
            }
            Ok(())
        })
    }
}

/// `FilePolicyService`.
pub struct FilePolicyService {
    base: Rc<Disposable>,
    service: Rc<AbstractPolicyService>,
    throttled_delayer: Rc<ThrottledDelayer<()>>,
}

impl FilePolicyService {
    pub fn new(file: URI, file_service: Rc<FileService>, log_service: LogServiceRef) -> Rc<Self> {
        let source = Rc::new(FileSource { file: file.clone(), file_service: file_service.clone(), log_service });
        let updater_source = source.clone();
        let updater: PolicyUpdater = Rc::new(move |service| updater_source.refresh(service));
        let service = AbstractPolicyService::new(updater);

        let base = Disposable::new();
        let throttled_delayer = Rc::new(ThrottledDelayer::new(500));
        base.register(throttled_delayer.clone());

        let watched = file.clone();
        let on_did_change_policy_file = file_service.on_did_files_change().filter(move |event| event.affects(&watched, &[]), None);
        base.register(file_service.watch(&file, None));
        let (weak_service, weak_delayer) = (Rc::downgrade(&service), Rc::downgrade(&throttled_delayer));
        base.register(on_did_change_policy_file.listen(move |_| {
            if let (Some(service), Some(delayer)) = (weak_service.upgrade(), weak_delayer.upgrade()) {
                let source = source.clone();
                let _ = delayer.trigger(move |_| source.refresh(&service), None);
            }
        }));

        Rc::new(Self { base, service, throttled_delayer })
    }
}

impl PolicyService for FilePolicyService {
    fn on_did_change(&self) -> Event<Vec<String>> {
        self.service.on_did_change()
    }

    fn update_policy_definitions(&self, policy_definitions: PolicyDefinitions) -> Promise<PolicyValues> {
        self.service.update_policy_definitions(policy_definitions)
    }

    fn get_policy_value(&self, name: &str) -> Option<PolicyValue> {
        self.service.get_policy_value(name)
    }

    fn get_policy_value_source(&self, name: &str) -> Option<PolicyValueSource> {
        self.service.get_policy_value_source(name)
    }

    fn serialize(&self) -> Option<Vec<(String, SerializedPolicy)>> {
        self.service.serialize()
    }

    fn policy_definitions(&self) -> PolicyDefinitions {
        PolicyService::policy_definitions(&self.service)
    }
}

impl IDisposable for FilePolicyService {
    fn dispose(&self) {
        self.base.dispose();
        self.service.dispose();
        let _ = &self.throttled_delayer;
    }
}
