//! Porte de `src/vs/editor/common/services/languageFeatureDebounce.ts`: o serviço que diz quanto esperar antes de
//! pedir de novo um recurso de linguagem (`ILanguageFeatureDebounceService`), com a média móvel do tempo que cada
//! recurso levou em cada modelo.
//!
//! Representação:
//! - `ILanguageFeatureDebounceService.for(feature, debugName, config)` é [`ILanguageFeatureDebounceService::for_feature`]
//!   (`for` é palavra reservada). O `LanguageFeatureRegistry<object>` do parâmetro é o `Rc` de um
//!   [`AnyLanguageFeatureRegistry`], que dá a identidade do registro e a dos provedores que casam com um modelo.
//! - `IdentityHash` (um `WeakMap<object, number>` com contador) é [`identity_hash_of`], sobre a identidade (o endereço)
//!   que o registro e os provedores informam. Como o endereço de um objeto solto pode ser reaproveitado por outro, uma
//!   entrada antiga pode valer para um objeto novo no mesmo lugar (o `WeakMap` do original esquece o objeto morto).
//! - `IEnvironmentService` é o [`EnvironmentService`] de `je-platform`, de onde o serviço lê só `isExtensionDevelopment`
//!   e `isBuilt`; `ILogService` é o [`LogServiceRef`].
//! - O `{ min?, max?, key? }` de `config` é [`DebounceConfig`] (a interface do original declara `salt`, mas a classe lê
//!   `key`; vale a classe).
//! - `registerSingleton(ILanguageFeatureDebounceService, ...)` não existe: o serviço é criado por quem o usa.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::dynamic::{Dynamic, number_to_js_string};
use je_base::hash::do_hash;
use je_base::map::{LRUCache, OrderedMap};
use je_base::network::{UriOrString, matches_scheme};
use je_base::numbers::{MovingAverage, SlidingWindowAverage, clamp};
use je_platform::environment::environment::EnvironmentService;
use je_platform::log::{LogServiceRef, Logger};

use crate::language_feature_registry::AnyLanguageFeatureRegistry;
use crate::text_model::TextModel;

/// `IFeatureDebounceInformation`.
pub trait IFeatureDebounceInformation {
    fn get(&self, model: &TextModel) -> f64;
    fn update(&self, model: &TextModel, value: f64) -> f64;
    /// `default()`.
    fn default_value(&self) -> f64;
}

/// O `config` de `ILanguageFeatureDebounceService.for`: `{ min?: number; max?: number; key?: string }`.
#[derive(Clone, Debug, Default)]
pub struct DebounceConfig {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub key: Option<String>,
}

/// `ILanguageFeatureDebounceService`.
pub trait ILanguageFeatureDebounceService {
    /// `for(feature, debugName, config?)`.
    fn for_feature(
        &self,
        feature: Rc<dyn AnyLanguageFeatureRegistry>,
        debug_name: &str,
        config: Option<DebounceConfig>,
    ) -> Rc<dyn IFeatureDebounceInformation>;
}

thread_local! {
    /// `IdentityHash._hashes` e `pool`.
    static IDENTITY_HASHES: RefCell<(HashMap<usize, u64>, u64)> = RefCell::new((HashMap::new(), 0));
}

/// `IdentityHash.of(obj)`: um número por objeto, em ordem de primeiro uso, a partir de 1.
fn identity_hash_of(identity: usize) -> u64 {
    IDENTITY_HASHES.with(|state| {
        let (hashes, pool) = &mut *state.borrow_mut();
        *hashes.entry(identity).or_insert_with(|| {
            *pool += 1;
            *pool
        })
    })
}

/// `NullDebounceInformation`.
struct NullDebounceInformation {
    default: f64,
}

impl IFeatureDebounceInformation for NullDebounceInformation {
    fn get(&self, _model: &TextModel) -> f64 {
        self.default
    }

    fn update(&self, _model: &TextModel, _value: f64) -> f64 {
        self.default
    }

    fn default_value(&self) -> f64 {
        self.default
    }
}

/// `FeatureDebounceInformation`.
struct FeatureDebounceInformation {
    cache: LRUCache<String, Rc<RefCell<SlidingWindowAverage>>>,
    log_service: LogServiceRef,
    name: String,
    registry: Rc<dyn AnyLanguageFeatureRegistry>,
    default: f64,
    min: f64,
    max: f64,
}

impl FeatureDebounceInformation {
    fn new(
        log_service: LogServiceRef,
        name: &str,
        registry: Rc<dyn AnyLanguageFeatureRegistry>,
        default: f64,
        min: f64,
        max: f64,
    ) -> Self {
        Self { cache: LRUCache::new(50, 0.7), log_service, name: name.to_owned(), registry, default, min, max }
    }

    fn key(&self, model: &TextModel) -> String {
        let hash = self
            .registry
            .provider_identities(model)
            .into_iter()
            .fold(0i32, |hash_val, identity| do_hash(&Dynamic::Number(identity_hash_of(identity) as f64), hash_val));
        format!("{}{}", model.id(), hash)
    }

    fn overall(&self) -> f64 {
        let mut result = MovingAverage::new();
        for avg in self.cache.values() {
            result.update(avg.borrow().value());
        }
        result.value()
    }
}

impl IFeatureDebounceInformation for FeatureDebounceInformation {
    fn get(&self, model: &TextModel) -> f64 {
        let key = self.key(model);
        match self.cache.get(&key) {
            Some(avg) => clamp(avg.borrow().value(), self.min, self.max),
            None => self.default_value(),
        }
    }

    fn update(&self, model: &TextModel, value: f64) -> f64 {
        let key = self.key(model);
        let avg = match self.cache.get(&key) {
            Some(avg) => avg,
            None => {
                let avg = Rc::new(RefCell::new(SlidingWindowAverage::new(6)));
                self.cache.set(key, Rc::clone(&avg));
                avg
            }
        };
        let new_value = clamp(avg.borrow_mut().update(value), self.min, self.max);
        let uri = model.uri();
        if !matches_scheme(UriOrString::Uri(&uri), "output") {
            self.log_service.trace(&format!(
                "[DEBOUNCE: {}] for {} is {}ms",
                self.name,
                uri.to_string(false).to_string_lossy(),
                number_to_js_string(new_value)
            ));
        }
        new_value
    }

    fn default_value(&self) -> f64 {
        // `(this._overall() | 0) || this._default`
        let overall = f64::from(self.overall() as i32);
        let value = if overall != 0.0 { overall } else { self.default };
        clamp(value, self.min, self.max)
    }
}

/// `LanguageFeatureDebounceService`.
pub struct LanguageFeatureDebounceService {
    log_service: LogServiceRef,
    data: RefCell<OrderedMap<String, Rc<dyn IFeatureDebounceInformation>>>,
    is_dev: Cell<bool>,
}

impl LanguageFeatureDebounceService {
    pub fn new(log_service: LogServiceRef, env_service: &dyn EnvironmentService) -> Self {
        Self {
            log_service,
            data: RefCell::new(OrderedMap::new()),
            is_dev: Cell::new(env_service.is_extension_development() || !env_service.is_built()),
        }
    }

    fn overall_average(&self) -> f64 {
        // Average of all language features. Not a great value but an approximation
        let mut result = MovingAverage::new();
        for info in self.data.borrow().values() {
            result.update(info.default_value());
        }
        result.value()
    }
}

impl ILanguageFeatureDebounceService for LanguageFeatureDebounceService {
    fn for_feature(
        &self,
        feature: Rc<dyn AnyLanguageFeatureRegistry>,
        name: &str,
        config: Option<DebounceConfig>,
    ) -> Rc<dyn IFeatureDebounceInformation> {
        let config = config.unwrap_or_default();
        let min = config.min.unwrap_or(50.0);
        let max = config.max.unwrap_or(min.powi(2));
        let extra = config.key.filter(|extra| !extra.is_empty());
        let key = format!(
            "{},{}{}",
            identity_hash_of(feature.identity()),
            number_to_js_string(min),
            extra.map(|extra| format!(",{extra}")).unwrap_or_default()
        );
        if let Some(info) = self.data.borrow().get(&key) {
            return Rc::clone(info);
        }

        let info: Rc<dyn IFeatureDebounceInformation> = if self.is_dev.get() {
            self.log_service.debug(&format!("[DEBOUNCE: {name}] is disabled in developed mode"));
            Rc::new(NullDebounceInformation { default: min * 1.5 })
        } else {
            // default is overall default or derived from min-value
            let overall = f64::from(self.overall_average() as i32);
            let default = if overall != 0.0 { overall } else { min * 1.5 };
            Rc::new(FeatureDebounceInformation::new(Rc::clone(&self.log_service), name, feature, default, min, max))
        };
        self.data.borrow_mut().insert(key, Rc::clone(&info));
        info
    }
}
