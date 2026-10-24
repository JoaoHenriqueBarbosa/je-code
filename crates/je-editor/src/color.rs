//! Porte de `src/vs/editor/contrib/colorPicker/browser/color.ts`: a coleta das cores dos provedores (`getColors`,
//! `getColorPresentations`, `_findColorData` e os três coletores `ColorDataCollector`, `ExtColorDataCollector` e
//! `ColorPresentationsCollector`).
//!
//! Decisões de tradução:
//! - `'auto' | 'always' | 'never'` é o [`DefaultColorDecoratorsEnablement`]; o parâmetro `defaultColorDecoratorsEnablement
//!   = 'auto'` é passado sempre pelo chamador.
//! - `provider instanceof DefaultDocumentColorProvider` é [`DocumentColorProvider::is_default_document_color_provider`]
//!   (o mesmo desenho do `is_textual_provider` dos destaques): a classe `DefaultDocumentColorProvider` ainda não tem
//!   porte, e só ela responde `true`.
//! - `DataCollector<T>` é o trait [`DataCollector`]; o `result: T[]` que `compute` preenche é compartilhado por um
//!   `Rc<RefCell<Vec<T>>>`, e a promessa devolve o `Array.isArray(documentColors)` (um `Some` é uma lista).
//! - `IExtColorData.color` é `[number, number, number, number]`, o `[f64; 4]`.
//! - `_setupColorCommand` lê serviços de um `ServicesAccessor` (`ILanguageFeaturesService`, `IModelService`,
//!   `IConfigurationService`), que o porte não tem: fica de fora.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::on_unexpected_external_error;
use je_base::promise::Promise;

use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_features_service::{DocumentColorProvider, IColorInformation, IColorPresentation};
use crate::range::Range;
use crate::text_model::TextModel;

/// `'auto' | 'always' | 'never'`: o valor de `editor.defaultColorDecorators`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultColorDecoratorsEnablement {
    Auto,
    Always,
    Never,
}

/// `IColorData`.
#[derive(Clone)]
pub struct IColorData {
    pub color_info: IColorInformation,
    pub provider: Rc<dyn DocumentColorProvider>,
}

/// `IExtColorData`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IExtColorData {
    pub range: Range,
    pub color: [f64; 4],
}

/// `DataCollector<T>`.
pub trait DataCollector<T: Clone + 'static> {
    /// `compute(provider, model, token, result)`: acrescenta os dados de `provider` a `result` e diz se ele devolveu uma
    /// lista.
    fn compute(&self, provider: &Rc<dyn DocumentColorProvider>, model: &Rc<TextModel>, token: &CancellationToken, result: &Rc<RefCell<Vec<T>>>) -> Promise<bool>;
}

/// `ColorDataCollector`.
pub struct ColorDataCollector;

impl DataCollector<IColorData> for ColorDataCollector {
    fn compute(&self, provider: &Rc<dyn DocumentColorProvider>, model: &Rc<TextModel>, token: &CancellationToken, colors: &Rc<RefCell<Vec<IColorData>>>) -> Promise<bool> {
        let (provider_for_data, colors) = (Rc::clone(provider), Rc::clone(colors));
        provider.provide_document_colors(model, token).then(move |document_colors| match document_colors {
            Some(color_infos) => {
                colors.borrow_mut().extend(color_infos.into_iter().map(|color_info| IColorData { color_info, provider: Rc::clone(&provider_for_data) }));
                true
            }
            None => false,
        })
    }
}

/// `ExtColorDataCollector`.
pub struct ExtColorDataCollector;

impl DataCollector<IExtColorData> for ExtColorDataCollector {
    fn compute(&self, provider: &Rc<dyn DocumentColorProvider>, model: &Rc<TextModel>, token: &CancellationToken, colors: &Rc<RefCell<Vec<IExtColorData>>>) -> Promise<bool> {
        let colors = Rc::clone(colors);
        provider.provide_document_colors(model, token).then(move |document_colors| match document_colors {
            Some(color_infos) => {
                colors.borrow_mut().extend(color_infos.into_iter().map(|color_info| IExtColorData {
                    range: color_info.range,
                    color: [color_info.color.red, color_info.color.green, color_info.color.blue, color_info.color.alpha],
                }));
                true
            }
            None => false,
        })
    }
}

/// `ColorPresentationsCollector`.
pub struct ColorPresentationsCollector {
    color_info: IColorInformation,
}

impl ColorPresentationsCollector {
    pub fn new(color_info: IColorInformation) -> Self {
        Self { color_info }
    }
}

impl DataCollector<IColorPresentation> for ColorPresentationsCollector {
    fn compute(&self, provider: &Rc<dyn DocumentColorProvider>, model: &Rc<TextModel>, _token: &CancellationToken, colors: &Rc<RefCell<Vec<IColorPresentation>>>) -> Promise<bool> {
        let colors = Rc::clone(colors);
        provider.provide_color_presentations(model, &self.color_info, &CancellationToken::None).then(move |document_colors| match document_colors {
            Some(presentations) => {
                colors.borrow_mut().extend(presentations);
                true
            }
            None => false,
        })
    }
}

/// `getColors(colorProviderRegistry, model, token, defaultColorDecoratorsEnablement = 'auto')`.
pub fn get_colors(
    color_provider_registry: &LanguageFeatureRegistry<dyn DocumentColorProvider>,
    model: &Rc<TextModel>,
    token: &CancellationToken,
    default_color_decorators_enablement: DefaultColorDecoratorsEnablement,
) -> Promise<Vec<IColorData>> {
    find_color_data(Rc::new(ColorDataCollector), color_provider_registry, model, token, default_color_decorators_enablement)
}

/// `getColorPresentations(model, colorInfo, provider, token)`.
pub fn get_color_presentations(
    model: &Rc<TextModel>,
    color_info: &IColorInformation,
    provider: &Rc<dyn DocumentColorProvider>,
    token: &CancellationToken,
) -> Promise<Option<Vec<IColorPresentation>>> {
    provider.provide_color_presentations(model, color_info, token)
}

/// `_findColorData(collector, colorProviderRegistry, model, token, defaultColorDecoratorsEnablement)`.
pub fn find_color_data<T: Clone + 'static>(
    collector: Rc<dyn DataCollector<T>>,
    color_provider_registry: &LanguageFeatureRegistry<dyn DocumentColorProvider>,
    model: &Rc<TextModel>,
    token: &CancellationToken,
    default_color_decorators_enablement: DefaultColorDecoratorsEnablement,
) -> Promise<Vec<T>> {
    let document_color_providers = color_provider_registry.ordered(model, false);
    let (model, token) = (Rc::clone(model), token.clone());
    async_fn(async move {
        let mut valid_document_color_provider_found = false;
        let mut default_provider: Option<Rc<dyn DocumentColorProvider>> = None;
        let color_data: Rc<RefCell<Vec<T>>> = Rc::default();
        for provider in document_color_providers.iter().rev() {
            if default_color_decorators_enablement != DefaultColorDecoratorsEnablement::Always && provider.is_default_document_color_provider() {
                default_provider = Some(Rc::clone(provider));
            } else {
                match collector.compute(provider, &model, &token, &color_data).await {
                    Ok(true) => valid_document_color_provider_found = true,
                    Ok(false) => {}
                    Err(err) => on_unexpected_external_error(&err),
                }
            }
        }
        if valid_document_color_provider_found {
            return Ok(color_data.take());
        }
        if let Some(default_provider) = default_provider.filter(|_| default_color_decorators_enablement != DefaultColorDecoratorsEnablement::Never) {
            collector.compute(&default_provider, &model, &token, &color_data).await?;
            return Ok(color_data.take());
        }
        Ok(Vec::new())
    })
}
