//! Auxiliar dos testes que disparam mudança de configuração: o literal
//! `{ affectsConfiguration: key => key === X, source: ConfigurationTarget.USER, ... }` do original vira o
//! [`ConfigurationChangeEvent`] real com a chave alterada e a origem `USER` (o literal só implementa
//! `affectsConfiguration` por igualdade de chave, que é o que o evento real faz para uma chave sem filhos).
#![allow(dead_code)]

use std::rc::Rc;

use je_base::JsString;
use je_base::map::ResourceMap;
use je_platform::configuration::models::{Configuration, ConfigurationChangeEvent, ConfigurationModel};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::configuration::{ConfigurationChange, ConfigurationTarget};
use je_platform::log::NullLogService;

fn empty_model() -> ConfigurationModel {
    ConfigurationModel::create_empty_model(NullLogService::shared())
}

/// Uma `Configuration` sem nenhuma camada preenchida.
fn empty_configuration() -> Configuration {
    Configuration::new(
        empty_model(),
        empty_model(),
        empty_model(),
        empty_model(),
        empty_model(),
        empty_model(),
        ResourceMap::new(),
        empty_model(),
        ResourceMap::new(),
        NullLogService::shared(),
    )
}

/// `configService.onDidChangeConfigurationEmitter.fire({ affectsConfiguration: key => keys.includes(key), source: USER })`.
pub fn fire_configuration_change(config: &TestConfigurationService, keys: &[&str]) {
    fire_configuration_change_from(config, keys, ConfigurationTarget::User);
}

/// O evento de `fire_configuration_change_from` cuja configuração atual já tem `value` em `key`. O `affectsConfiguration(key, {})`
/// do evento real compara o valor anterior (sem configuração anterior, `undefined`) com o atual, e o literal do original afirma
/// `key === settingId && overrides !== undefined` sem comparar valores: o valor atual faz a comparação divergir.
pub fn fire_configuration_change_with_value(config: &TestConfigurationService, key: &str, value: je_base::dynamic::Dynamic, source: ConfigurationTarget) {
    let current = empty_configuration();
    current.update_value(key, value, &je_platform::configuration::ConfigurationUpdateOverrides::default());
    let change = ConfigurationChange { keys: vec![JsString::from(key)], overrides: Vec::new() };
    let event = ConfigurationChangeEvent::new(change, None, Rc::new(current), None, NullLogService::shared());
    event.source.set(Some(source));
    config.on_did_change_configuration_emitter.fire(Rc::new(event));
}

/// O mesmo evento com a origem (`source`) dada.
pub fn fire_configuration_change_from(config: &TestConfigurationService, keys: &[&str], source: ConfigurationTarget) {
    let change = ConfigurationChange { keys: keys.iter().map(|key| JsString::from(*key)).collect(), overrides: Vec::new() };
    let event = ConfigurationChangeEvent::new(change, None, Rc::new(empty_configuration()), None, NullLogService::shared());
    event.source.set(Some(source));
    config.on_did_change_configuration_emitter.fire(Rc::new(event));
}
