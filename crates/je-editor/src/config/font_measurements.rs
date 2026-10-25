//! Porte de `src/vs/editor/browser/config/fontMeasurements.ts`: `FontMeasurementsImpl` (cache de medições por janela
//! e por `BareFontInfo`, `readFontInfo`, `restoreFontInfo`, `serializeFontInfo`, `clearAllFontInfos`, o evento de
//! mudança e a reavaliação das leituras não confiáveis depois de 5 segundos) e o `FontMeasurementsCache`.
//!
//! Decisões de tradução:
//! - `FontMeasurementsImpl extends Disposable` é composição com o [`Disposable`] base; `dispose` cancela o timer de
//!   evicção antes. Os callbacks que capturam `this` guardam um `Weak` (preenchido por `Rc::new_cyclic`).
//! - `Window` e o `getWindowId` são o [`WindowId`] (inteiro). `onDidUnregisterWindow`, `PixelRatio.getInstance(window).value`
//!   e `readCharWidths` (o canvas do navegador) vêm do hospedeiro, no trait [`FontMeasurementsEnvironment`]: a medição
//!   de fonte é a do rasterizador/shaper nativo, que cumpre cada [`CharWidthRequest`].
//! - `targetWindow.setTimeout` é o `set_timeout` do loop de eventos do thread (uma só `TimeApi`); o efeito observável
//!   (reler depois de 5000 ms, e não fazer nada se a janela fechou) é o mesmo.
//! - `FontInfo` é compartilhado entre o cache e quem lê (`Rc<FontInfo>`), porque o original devolve o mesmo objeto a cada
//!   leitura. `ISerializedFontInfo` é [`SerializedFontInfo`], convertido nas opções do construtor de `FontInfo`.
//! - O `FontMeasurements` global (`new FontMeasurementsImpl()` do módulo) não existe: o hospedeiro cria e guarda a
//!   instância com o seu ambiente.
//! - `_cache` é inspecionado pelo teste do original com acesso por colchetes; aqui é [`FontMeasurementsImpl::has_window_cache`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::runtime::{TimerId, clear_timeout, set_timeout};

use super::char_width_reader::{CharWidthRequest, CharWidthRequestType};
use super::editor_options::font_ligatures::EditorFontLigatures;
use super::font_info::{BareFontInfo, BareFontInfoOptions, FontInfo, FontInfoOptions, SERIALIZED_FONT_INFO_VERSION};

/// O identificador de uma janela (`getWindowId(targetWindow)`).
pub type WindowId = u32;

/// `ISerializedFontInfo`: informação de fonte serializável.
#[derive(Clone, Debug, PartialEq)]
pub struct SerializedFontInfo {
    pub version: f64,
    pub pixel_ratio: f64,
    pub font_family: String,
    pub font_weight: String,
    pub font_size: f64,
    pub font_feature_settings: String,
    pub font_variation_settings: String,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub is_monospace: bool,
    pub typical_halfwidth_character_width: f64,
    pub typical_fullwidth_character_width: f64,
    pub can_use_halfwidth_rightwards_arrow: bool,
    pub space_width: f64,
    pub middot_width: f64,
    pub wsmiddot_width: f64,
    pub max_digit_width: f64,
}

impl From<&SerializedFontInfo> for FontInfoOptions {
    fn from(saved: &SerializedFontInfo) -> FontInfoOptions {
        FontInfoOptions {
            bare: BareFontInfoOptions {
                pixel_ratio: saved.pixel_ratio,
                font_family: saved.font_family.clone(),
                font_weight: saved.font_weight.clone(),
                font_size: saved.font_size,
                font_feature_settings: saved.font_feature_settings.clone(),
                font_variation_settings: saved.font_variation_settings.clone(),
                line_height: saved.line_height,
                letter_spacing: saved.letter_spacing,
            },
            is_monospace: saved.is_monospace,
            typical_halfwidth_character_width: saved.typical_halfwidth_character_width,
            typical_fullwidth_character_width: saved.typical_fullwidth_character_width,
            can_use_halfwidth_rightwards_arrow: saved.can_use_halfwidth_rightwards_arrow,
            space_width: saved.space_width,
            middot_width: saved.middot_width,
            wsmiddot_width: saved.wsmiddot_width,
            max_digit_width: saved.max_digit_width,
        }
    }
}

/// O que o `FontMeasurements` pede ao hospedeiro (ver "Decisões de tradução" no topo do módulo).
pub trait FontMeasurementsEnvironment {
    /// `PixelRatio.getInstance(targetWindow).value`.
    fn pixel_ratio(&self, target_window: WindowId) -> f64;

    /// `onDidUnregisterWindow`: o `vscodeWindowId` da janela que saiu.
    fn on_did_unregister_window(&self) -> Event<WindowId>;

    /// `readCharWidths(targetWindow, bareFontInfo, requests)`: mede cada pedido com a fonte e o aplica com
    /// [`CharWidthRequest::fulfill`].
    fn read_char_widths(&self, target_window: WindowId, bare_font_info: &BareFontInfo, requests: &[Rc<CharWidthRequest>]);
}

/// Os pedidos de medição de uma leitura: todos, e o subconjunto do teste de monoespaçamento.
#[derive(Default)]
struct Requests {
    all: Vec<Rc<CharWidthRequest>>,
    monospace: Vec<Rc<CharWidthRequest>>,
}

impl Requests {
    /// `_createRequest(chr, type, all, monospace)`.
    fn create(&mut self, chr: &str, request_type: CharWidthRequestType, in_monospace_test: bool) -> Rc<CharWidthRequest> {
        let result = Rc::new(CharWidthRequest::new(chr, request_type));
        self.all.push(result.clone());
        if in_monospace_test {
            self.monospace.push(result.clone());
        }
        result
    }
}

/// `FontMeasurementsImpl`.
pub struct FontMeasurementsImpl {
    base: Rc<Disposable>,
    me: Weak<FontMeasurementsImpl>,
    environment: Rc<dyn FontMeasurementsEnvironment>,
    cache: RefCell<HashMap<WindowId, FontMeasurementsCache>>,
    evict_untrusted_readings_timeout: Cell<Option<TimerId>>,
    on_did_change: Rc<Emitter<()>>,
}

impl FontMeasurementsImpl {
    pub fn new(environment: Rc<dyn FontMeasurementsEnvironment>) -> Rc<FontMeasurementsImpl> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        let this = Rc::new_cyclic(|me| FontMeasurementsImpl {
            base,
            me: me.clone(),
            environment: environment.clone(),
            cache: RefCell::new(HashMap::new()),
            evict_untrusted_readings_timeout: Cell::new(None),
            on_did_change,
        });
        let weak = Rc::downgrade(&this);
        this.base.register(environment.on_did_unregister_window().listen(move |vscode_window_id| {
            if let Some(this) = weak.upgrade() {
                this.cache.borrow_mut().remove(vscode_window_id);
            }
        }));
        this
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// Se a janela tem cache, sem criar um (o que o teste do original confere com `_cache.has`).
    pub fn has_window_cache(&self, target_window: WindowId) -> bool {
        self.cache.borrow().contains_key(&target_window)
    }

    /// `clearAllFontInfos()`: limpa toda a informação em cache e dispara a mudança.
    pub fn clear_all_font_infos(&self) {
        self.cache.borrow_mut().clear();
        self.on_did_change.fire(());
    }

    /// `_ensureCache(targetWindow)` e a operação sobre ele.
    fn with_cache<R>(&self, target_window: WindowId, f: impl FnOnce(&mut FontMeasurementsCache) -> R) -> R {
        f(self.cache.borrow_mut().entry(target_window).or_default())
    }

    /// `_writeToCache(targetWindow, item, value, isRestored)`.
    fn write_to_cache(&self, target_window: WindowId, item: &BareFontInfo, value: Rc<FontInfo>, is_restored: bool) {
        let is_trusted = value.is_trusted;
        self.with_cache(target_window, |cache| cache.put(item, value, is_restored));

        if !is_trusted && self.evict_untrusted_readings_timeout.get().is_none() {
            // Tenta ler de novo depois de um tempo.
            let weak = self.me.clone();
            self.evict_untrusted_readings_timeout.set(Some(set_timeout(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.evict_untrusted_readings_timeout.set(None);
                        this.evict_untrusted_readings(target_window);
                    }
                },
                5000,
            )));
        }
    }

    /// `_evictUntrustedReadings(targetWindow)`.
    fn evict_untrusted_readings(&self, target_window: WindowId) {
        let values = match self.cache.borrow().get(&target_window) {
            Some(cache) => cache.get_values(),
            // A janela fechou ou o cache de fontes foi limpo durante a espera.
            None => return,
        };
        let mut something_removed = false;
        for item in &values {
            if !item.is_trusted {
                something_removed = true;
                self.with_cache(target_window, |cache| cache.remove(item));
            }
        }
        if something_removed {
            self.on_did_change.fire(());
        }
    }

    /// `serializeFontInfo(targetWindow)`: as informações confiáveis em cache, ou `None` quando o cache só tem leituras
    /// restauradas.
    pub fn serialize_font_info(&self, target_window: WindowId) -> Option<Vec<Rc<FontInfo>>> {
        self.with_cache(target_window, |cache| {
            if cache.should_preserve_persisted_values() {
                return None;
            }
            Some(cache.get_values().into_iter().filter(|item| item.is_trusted).collect())
        })
    }

    /// `restoreFontInfo(targetWindow, savedFontInfos)`: restaura informações de fonte serializadas antes.
    pub fn restore_font_info(&self, target_window: WindowId, saved_font_infos: &[SerializedFontInfo]) {
        // Insere toda informação salva no cache sem a marca de confiável: uma fonte pode ter sido instalada no sistema
        // nesse meio tempo.
        for saved_font_info in saved_font_infos {
            if saved_font_info.version != SERIALIZED_FONT_INFO_VERSION {
                // Não dá para usar uma versão mais antiga.
                continue;
            }
            let font_info = Rc::new(FontInfo::new(FontInfoOptions::from(saved_font_info), false));
            self.write_to_cache(target_window, &font_info, font_info.clone(), true);
        }
    }

    /// `readFontInfo(targetWindow, bareFontInfo)`.
    pub fn read_font_info(&self, target_window: WindowId, bare_font_info: &BareFontInfo) -> Rc<FontInfo> {
        if !self.with_cache(target_window, |cache| cache.has(bare_font_info)) {
            let mut read_config = self.actual_read_font_info(target_window, bare_font_info);

            if read_config.typical_halfwidth_character_width <= 2.0
                || read_config.typical_fullwidth_character_width <= 2.0
                || read_config.space_width <= 2.0
                || read_config.max_digit_width <= 2.0
            {
                // É o Bug 14341: não deu para ler.
                read_config = Rc::new(FontInfo::new(
                    FontInfoOptions {
                        bare: Self::bare_options(&read_config, self.environment.pixel_ratio(target_window)),
                        is_monospace: read_config.is_monospace,
                        typical_halfwidth_character_width: read_config.typical_halfwidth_character_width.max(5.0),
                        typical_fullwidth_character_width: read_config.typical_fullwidth_character_width.max(5.0),
                        can_use_halfwidth_rightwards_arrow: read_config.can_use_halfwidth_rightwards_arrow,
                        space_width: read_config.space_width.max(5.0),
                        middot_width: read_config.middot_width.max(5.0),
                        wsmiddot_width: read_config.wsmiddot_width.max(5.0),
                        max_digit_width: read_config.max_digit_width.max(5.0),
                    },
                    false,
                ));
            }

            self.write_to_cache(target_window, bare_font_info, read_config, false);
        }
        self.with_cache(target_window, |cache| cache.get(bare_font_info))
    }

    /// Os campos de [`BareFontInfo`] com a razão de pixels da janela.
    fn bare_options(bare: &BareFontInfo, pixel_ratio: f64) -> BareFontInfoOptions {
        BareFontInfoOptions {
            pixel_ratio,
            font_family: bare.font_family.clone(),
            font_weight: bare.font_weight.clone(),
            font_size: bare.font_size,
            font_feature_settings: bare.font_feature_settings.clone(),
            font_variation_settings: bare.font_variation_settings.clone(),
            line_height: bare.line_height,
            letter_spacing: bare.letter_spacing,
        }
    }

    /// `_actualReadFontInfo(targetWindow, bareFontInfo)`.
    fn actual_read_font_info(&self, target_window: WindowId, bare_font_info: &BareFontInfo) -> Rc<FontInfo> {
        use CharWidthRequestType::{Bold, Italic, Regular};

        let mut requests = Requests::default();

        let typical_halfwidth_character = requests.create("n", Regular, true);
        let typical_fullwidth_character = requests.create("\u{ff4d}", Regular, false);
        let space = requests.create(" ", Regular, true);
        let digits: Vec<Rc<CharWidthRequest>> = ('0'..='9').map(|digit| requests.create(&digit.to_string(), Regular, true)).collect();

        // Teste de monoespaçamento: usado na renderização de espaços em branco.
        let rightwards_arrow = requests.create("\u{2192}", Regular, true);
        let halfwidth_rightwards_arrow = requests.create("\u{ffeb}", Regular, false);

        // U+00B7 - MIDDLE DOT
        let middot = requests.create("\u{b7}", Regular, true);

        // U+2E31 - WORD SEPARATOR MIDDLE DOT
        let wsmiddot_width = requests.create("\u{2e31}", Regular, false);

        // Teste de monoespaçamento: alguns caracteres.
        for chr in "|/-_ilm%".chars() {
            let chr = chr.to_string();
            requests.create(&chr, Regular, true);
            requests.create(&chr, Italic, true);
            requests.create(&chr, Bold, true);
        }

        if bare_font_info.font_feature_settings != EditorFontLigatures::OFF {
            for test_string in ["fi", "<=", "=>", "==="] {
                requests.create(test_string, Regular, true);
            }
        }

        self.environment.read_char_widths(target_window, bare_font_info, &requests.all);

        let max_digit_width = digits.iter().map(|digit| digit.width()).fold(f64::NEG_INFINITY, f64::max);

        let mut is_monospace = true;
        let reference_width = requests.monospace[0].width();
        for request in &requests.monospace[1..] {
            let diff = reference_width * request.chr_length() as f64 - request.width();
            if !(-0.005..=0.005).contains(&diff) {
                is_monospace = false;
                break;
            }
        }

        let mut can_use_halfwidth_rightwards_arrow = true;
        if is_monospace && halfwidth_rightwards_arrow.width() != reference_width {
            // Usar uma seta para a direita de meia largura quebraria o monoespaçamento...
            can_use_halfwidth_rightwards_arrow = false;
        }
        if halfwidth_rightwards_arrow.width() > rightwards_arrow.width() {
            // Usar uma seta de meia largura pintaria uma seta maior que a seta regular.
            can_use_halfwidth_rightwards_arrow = false;
        }

        Rc::new(FontInfo::new(
            FontInfoOptions {
                bare: Self::bare_options(bare_font_info, self.environment.pixel_ratio(target_window)),
                is_monospace,
                typical_halfwidth_character_width: typical_halfwidth_character.width(),
                typical_fullwidth_character_width: typical_fullwidth_character.width(),
                can_use_halfwidth_rightwards_arrow,
                space_width: space.width(),
                middot_width: middot.width(),
                wsmiddot_width: wsmiddot_width.width(),
                max_digit_width,
            },
            true,
        ))
    }
}

impl IDisposable for FontMeasurementsImpl {
    fn dispose(&self) {
        if let Some(timeout) = self.evict_untrusted_readings_timeout.take() {
            clear_timeout(timeout);
        }
        self.base.dispose();
    }
}

/// `FontMeasurementsCache`: as chaves (o `getId()` do `BareFontInfo`) na ordem de inserção, como o objeto do original.
#[derive(Default)]
struct FontMeasurementsCache {
    entries: Vec<(String, Rc<FontInfo>)>,
    was_populated_with_restored_values: bool,
    was_populated_with_current_session_values: bool,
}

impl FontMeasurementsCache {
    fn has(&self, item: &BareFontInfo) -> bool {
        let item_id = item.get_id();
        self.entries.iter().any(|(id, _)| *id == item_id)
    }

    /// `get(item)`: o original devolve `undefined` sem o item, mas só é chamado depois de `has` ou de `put`.
    fn get(&self, item: &BareFontInfo) -> Rc<FontInfo> {
        let item_id = item.get_id();
        self.entries
            .iter()
            .find(|(id, _)| *id == item_id)
            .map(|(_, value)| value.clone())
            .expect("leitura sem entrada no cache")
    }

    fn put(&mut self, item: &BareFontInfo, value: Rc<FontInfo>, is_restored: bool) {
        let item_id = item.get_id();
        match self.entries.iter_mut().find(|(id, _)| *id == item_id) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((item_id, value)),
        }
        if is_restored {
            self.was_populated_with_restored_values = true;
        } else {
            self.was_populated_with_current_session_values = true;
        }
    }

    fn remove(&mut self, item: &FontInfo) {
        let item_id = item.get_id();
        self.entries.retain(|(id, _)| *id != item_id);
    }

    fn get_values(&self) -> Vec<Rc<FontInfo>> {
        self.entries.iter().map(|(_, value)| value.clone()).collect()
    }

    fn should_preserve_persisted_values(&self) -> bool {
        self.was_populated_with_restored_values && !self.was_populated_with_current_session_values
    }
}
