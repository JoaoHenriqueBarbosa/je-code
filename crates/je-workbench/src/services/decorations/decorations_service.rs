//! Porte de `src/vs/workbench/services/decorations/browser/decorationsService.ts`: o `DecorationsService`, que junta os
//! dados dos provedores de decorações por recurso (com as pesquisas assíncronas e o cancelamento das pendentes), calcula
//! o que sobe das pastas filhas (`bubble`) e entrega, para cada recurso, a [`IDecoration`] com os nomes de classe e a dica.
//!
//! Divergências:
//! - **Folha de estilo.** `createStyleSheet` e `createCSSRule` (`domStylesheets.ts`) são o [`StyleSheet`]: a lista, em
//!   memória, das regras `seletor {texto}`, com a inserção no início (`insertRule(..., 0)`) e a remoção das regras cujo
//!   seletor contém um nome (`removeCSSRulesContainingSelector`). O renderizador nativo lê as regras por
//!   [`DecorationsService::style_rules`]; a medição de estilo computado no DOM (`getComputedStyle`) não existe.
//! - **Tema de ícones de produto.** O `IThemeService` do porte não tem `getProductIconTheme`, e o registro de ícones
//!   (`getIconRegistry().getIcon(id)`) não foi portado. A única leitura que o serviço faz do tema é o ícone da letra de uma
//!   decoração, que é o [`IProductIconTheme::get_icon`] (recebe o id do ícone e devolve `None` quando o registro não o
//!   conhece ou o tema não o define, como os dois `return` do original). O construtor recebe o tema de ícones no lugar do
//!   `IThemeService`, e não há `registerSingleton` (o serviço de tema ainda não é injetável).
//! - **`IDecorationData | IDecorationData[]`.** `DecorationRule` só é criada pelo `asDecoration`, que sempre passa a lista;
//!   o ramo de um dado só (`_appendForOne`) é inalcançável e não existe aqui. `keyOf` aceita as duas formas.
//! - **`provider.onDidChange(uris => if (!uris) removeAll())`.** O evento do porte é `Event<Vec<URI>>` e não tem o
//!   valor ausente do original (o "flush"), então só o ramo das mudanças seletivas existe; o `removeAll` roda no descarte
//!   do registro.
//! - **`DecorationDataRequest.thenable`.** Ninguém lê a promessa no original (a cadeia `then/catch/finally` é que
//!   importa), então a requisição guarda só a fonte do cancelamento.
//! - **Identidade dos provedores.** As chaves dos `Map` por provedor são o endereço do `Rc` do provedor.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::arrays::distinct;
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::Dynamic;
use je_base::errors::is_cancellation_error;
use je_base::event::{DebounceEmitter, Emitter, Event};
use je_base::hash::hash;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::linked_list::LinkedList;
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_base::strings::is_falsy_or_whitespace;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::themables::{ColorIdentifier, ThemeIcon};
use je_base::uri::URI;
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_theme::registry::as_css_variable_name;

use crate::services::decorations::decorations::{
    DecorationData, DecorationLetter, IDecoration, IDecorationsProvider, IDecorationsService, IResourceDecorationChangeEvent,
};

/// `IconDefinition` do tema de ícones de produto: o caractere da fonte e o id da fonte (`definition.font?.id`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconDefinition {
    pub font_character: String,
    pub font_id: Option<String>,
}

/// O que o serviço lê do `IProductIconTheme` (ver as divergências no topo do módulo).
pub trait IProductIconTheme {
    /// `getIcon(getIconRegistry().getIcon(icon_id))`.
    fn get_icon(&self, icon_id: &str) -> Option<IconDefinition>;
}

/// `asCssVariable`.
fn as_css_variable(color: &str) -> String {
    format!("var({})", as_css_variable_name(color))
}

/// `asCssVariableWithDefault`.
fn as_css_variable_with_default(color: &str, default_css_value: &str) -> String {
    format!("var({}, {default_css_value})", as_css_variable_name(color))
}

/// `getColor`.
fn get_color(color: Option<&ColorIdentifier>) -> String {
    color.map_or_else(|| "inherit".to_string(), |color| as_css_variable(color))
}

/// `cssValue.stringValue`.
fn string_value(value: &str) -> String {
    format!("'{}'", value.replace('\'', "\\000027"))
}

/// `(number).toString(36)` de um inteiro de 32 bits.
fn to_base36(value: i32) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let negative = value < 0;
    let mut remaining = i64::from(value).unsigned_abs();
    let mut digits = Vec::new();
    while remaining > 0 {
        digits.push(DIGITS[(remaining % 36) as usize]);
        remaining /= 36;
    }
    if negative {
        digits.push(b'-');
    }
    digits.reverse();
    String::from_utf8(digits).expect("os dígitos são ASCII")
}

/// A folha de estilo da [`DecorationStyles`] (`createStyleSheet`), em memória.
#[derive(Default)]
struct StyleSheet {
    /// `(seletor, texto)` de cada regra, a mais nova primeiro.
    rules: RefCell<Vec<(String, String)>>,
}

impl StyleSheet {
    /// `createCSSRule(selector, cssText, element)`: não faz nada sem texto.
    fn create_css_rule(&self, selector: &str, css_text: &str) {
        if css_text.is_empty() {
            return;
        }
        self.rules.borrow_mut().insert(0, (selector.to_string(), css_text.to_string()));
    }

    /// `removeCSSRulesContainingSelector(ruleName, element)`.
    fn remove_css_rules_containing_selector(&self, rule_name: &str) {
        self.rules.borrow_mut().retain(|(selector, _)| !selector.contains(rule_name));
    }
}

/// `DecorationRule.keyOf` de um dado só.
fn key_of_one(data: &DecorationData) -> String {
    let color = data.color.as_deref().unwrap_or("undefined");
    match &data.letter {
        Some(DecorationLetter::Icon(icon)) => format!("{color}+{}", icon.id),
        Some(DecorationLetter::Text(letter)) => format!("{color}/{letter}"),
        None => format!("{color}/undefined"),
    }
}

/// `DecorationRule.keyOf` da lista.
fn key_of(data: &[DecorationData]) -> String {
    data.iter().map(key_of_one).collect::<Vec<_>>().join(",")
}

/// `DecorationRule`.
struct DecorationRule {
    product_icon_theme: Rc<dyn IProductIconTheme>,
    data: Vec<DecorationData>,
    item_color_class_name: String,
    item_badge_class_name: String,
    icon_badge_class_name: String,
    bubble_badge_class_name: String,
    ref_counter: Cell<i32>,
}

impl DecorationRule {
    const CLASS_NAMES_PREFIX: &'static str = "monaco-decoration";

    fn new(product_icon_theme: Rc<dyn IProductIconTheme>, data: Vec<DecorationData>, key: &str) -> Self {
        let suffix = to_base36(hash(&Dynamic::from(key)));
        let prefix = Self::CLASS_NAMES_PREFIX;
        Self {
            product_icon_theme,
            data,
            item_color_class_name: format!("{prefix}-itemColor-{suffix}"),
            item_badge_class_name: format!("{prefix}-itemBadge-{suffix}"),
            bubble_badge_class_name: format!("{prefix}-bubbleBadge-{suffix}"),
            icon_badge_class_name: format!("{prefix}-iconBadge-{suffix}"),
            ref_counter: Cell::new(0),
        }
    }

    fn acquire(&self) {
        self.ref_counter.set(self.ref_counter.get() + 1);
    }

    fn release(&self) -> bool {
        self.ref_counter.set(self.ref_counter.get() - 1);
        self.ref_counter.get() == 0
    }

    /// `appendCSSRules` (`_appendForMany`).
    fn append_css_rules(&self, element: &StyleSheet) {
        // label
        let color = self
            .data
            .iter()
            .rev()
            .fold("inherit".to_string(), |fallback, decoration| match &decoration.color {
                Some(color) if !color.is_empty() => as_css_variable_with_default(color, &fallback),
                _ => fallback,
            });
        element.create_css_rule(&format!(".{}", self.item_color_class_name), &format!("color: {color};"));

        // badge or icon
        let mut letters: Vec<&str> = Vec::new();
        let mut icon: Option<&ThemeIcon> = None;

        for decoration in &self.data {
            match &decoration.letter {
                Some(DecorationLetter::Icon(letter)) => {
                    icon = Some(letter);
                    break;
                }
                Some(DecorationLetter::Text(letter)) if !letter.is_empty() => letters.push(letter),
                _ => {}
            }
        }

        if let Some(icon) = icon {
            self.create_icon_css_rule(icon, &color, element);
        } else {
            if !letters.is_empty() {
                element.create_css_rule(&format!(".{}::after", self.item_badge_class_name), &format!("content: \"{}\"; color: {color};", letters.join(", ")));
            }

            // bubble badge
            element.create_css_rule(
                &format!(".{}::after", self.bubble_badge_class_name),
                &format!("content: \"\u{ea71}\"; color: {color}; font-family: codicon; font-size: 14px; margin-right: 14px; opacity: 0.4;"),
            );
        }
    }

    fn create_icon_css_rule(&self, icon: &ThemeIcon, color: &str, element: &StyleSheet) {
        let modifier = ThemeIcon::get_modifier(icon).map(str::to_string);
        let icon = if modifier.is_some() { ThemeIcon::modify(icon, None) } else { icon.clone() };
        let Some(definition) = self.product_icon_theme.get_icon(&icon.id) else {
            return;
        };
        let icon_color = icon.color.as_ref().map_or_else(|| color.to_string(), |theme_color| get_color(Some(&theme_color.id)));
        let spin = if modifier.as_deref() == Some("spin") {
            "animation: codicon-spin 1.5s steps(30) infinite; font-style: normal !important; transform-origin: center center;"
        } else {
            ""
        };
        element.create_css_rule(
            &format!(".{}::after", self.icon_badge_class_name),
            &format!(
                "content: '{}';\n\t\t\tcolor: {icon_color};\n\t\t\tfont-family: {};\n\t\t\tfont-size: 16px;\n\t\t\tmargin-right: 14px;\n\t\t\tfont-weight: normal;\n\t\t\t{spin};\n\t\t\t",
                definition.font_character,
                string_value(definition.font_id.as_deref().unwrap_or("codicon"))
            ),
        );
    }

    fn remove_css_rules(&self, element: &StyleSheet) {
        element.remove_css_rules_containing_selector(&self.item_color_class_name);
        element.remove_css_rules_containing_selector(&self.item_badge_class_name);
        element.remove_css_rules_containing_selector(&self.bubble_badge_class_name);
        element.remove_css_rules_containing_selector(&self.icon_badge_class_name);
    }
}

/// `DecorationStyles`.
struct DecorationStyles {
    product_icon_theme: Rc<dyn IProductIconTheme>,
    style_element: StyleSheet,
    decoration_rules: RefCell<HashMap<String, Rc<DecorationRule>>>,
}

impl DecorationStyles {
    fn new(product_icon_theme: Rc<dyn IProductIconTheme>) -> Rc<Self> {
        Rc::new(Self { product_icon_theme, style_element: StyleSheet::default(), decoration_rules: RefCell::new(HashMap::new()) })
    }

    fn as_decoration(self: &Rc<Self>, mut data: Vec<DecorationData>, only_children: bool) -> Rc<dyn IDecoration> {
        // sort by weight
        data.sort_by(|a, b| b.weight.unwrap_or(0).cmp(&a.weight.unwrap_or(0)));

        let key = key_of(&data);
        let existing = self.decoration_rules.borrow().get(&key).cloned();
        let rule = match existing {
            Some(rule) => rule,
            None => {
                // new css rule
                let rule = Rc::new(DecorationRule::new(self.product_icon_theme.clone(), data.clone(), &key));
                self.decoration_rules.borrow_mut().insert(key.clone(), rule.clone());
                rule.append_css_rules(&self.style_element);
                rule
            }
        };

        rule.acquire();

        let label_class_name = rule.item_color_class_name.clone();
        let mut badge_class_name = rule.item_badge_class_name.clone();
        let icon_class_name = rule.icon_badge_class_name.clone();
        let tooltips: Vec<String> = data
            .iter()
            .filter_map(|decoration| decoration.tooltip.clone())
            .filter(|tooltip| {
                let units: Vec<u16> = tooltip.encode_utf16().collect();
                !is_falsy_or_whitespace(Some(units.as_slice()))
            })
            .collect();
        let mut tooltip = distinct(&tooltips, |tooltip| tooltip.clone()).join(" \u{2022} ");
        let strikethrough = data.iter().any(|decoration| decoration.strikethrough.unwrap_or(false));
        let is_text_badge = !only_children
            && !data.iter().any(|decoration| matches!(decoration.letter, Some(DecorationLetter::Icon(_))))
            && data.iter().any(|decoration| matches!(&decoration.letter, Some(DecorationLetter::Text(letter)) if !letter.is_empty()));

        if only_children {
            // show items from its children only
            badge_class_name = rule.bubble_badge_class_name.clone();
            tooltip = localize("bubbleTitle", "Contains emphasized items", &[]);
        }

        Rc::new(Decoration {
            label_class_name,
            badge_class_name,
            icon_class_name,
            strikethrough,
            is_text_badge,
            tooltip,
            rule: RefCell::new(Some(rule)),
            key,
            styles: self.clone(),
        })
    }
}

/// O objeto que `asDecoration` devolve.
struct Decoration {
    label_class_name: String,
    badge_class_name: String,
    icon_class_name: String,
    strikethrough: bool,
    is_text_badge: bool,
    tooltip: String,
    /// O `rule` da closure do `dispose`, que vira `undefined` quando a última referência é solta.
    rule: RefCell<Option<Rc<DecorationRule>>>,
    key: String,
    styles: Rc<DecorationStyles>,
}

impl IDisposable for Decoration {
    fn dispose(&self) {
        let rule = self.rule.borrow().clone();
        if rule.is_some_and(|rule| rule.release()) {
            self.styles.decoration_rules.borrow_mut().remove(&self.key);
            if let Some(rule) = self.rule.borrow_mut().take() {
                rule.remove_css_rules(&self.styles.style_element);
            }
        }
    }
}

impl IDecoration for Decoration {
    fn tooltip(&self) -> String {
        self.tooltip.clone()
    }

    fn strikethrough(&self) -> bool {
        self.strikethrough
    }

    fn label_class_name(&self) -> String {
        self.label_class_name.clone()
    }

    fn badge_class_name(&self) -> String {
        self.badge_class_name.clone()
    }

    fn icon_class_name(&self) -> String {
        self.icon_class_name.clone()
    }

    fn is_text_badge(&self) -> bool {
        self.is_text_badge
    }
}

/// `FileDecorationChangeEvent`.
struct FileDecorationChangeEvent {
    data: TernarySearchTree<URI, bool>,
}

impl FileDecorationChangeEvent {
    fn new(all: &[URI]) -> Self {
        // events ignore all path casings
        let mut data = TernarySearchTree::for_uris(Rc::new(|_: &URI| true), Rc::new(|_: &URI| false));
        data.fill_keys(true, all);
        Self { data }
    }
}

impl IResourceDecorationChangeEvent for FileDecorationChangeEvent {
    fn affects_resource(&self, uri: &URI) -> bool {
        self.data.has_element_or_subtree(uri)
    }
}

/// `{ affectsResource() { return true; } }`: tudo pode ter mudado.
struct EverythingChanged;

impl IResourceDecorationChangeEvent for EverythingChanged {
    fn affects_resource(&self, _uri: &URI) -> bool {
        true
    }
}

/// `DecorationDataRequest` (ver as divergências no topo do módulo).
struct DecorationDataRequest {
    source: CancellationTokenSource,
}

/// O valor de um provedor no `DecorationEntry`: `DecorationDataRequest | IDecorationData | null`.
#[derive(Clone)]
enum EntryValue {
    Request(Rc<DecorationDataRequest>),
    /// `IDecorationData`, ou `null` quando o provedor não tem nada para o recurso.
    Data(Option<DecorationData>),
}

/// `DecorationEntry`: `Map<IDecorationsProvider, ...>`, na ordem de inserção do `Map` (regravar uma chave existente
/// mantém a posição; apagar e gravar de novo leva ao fim).
#[derive(Default)]
struct DecorationEntry {
    values: Vec<(usize, EntryValue)>,
}

impl DecorationEntry {
    fn get(&self, provider: usize) -> Option<EntryValue> {
        self.values.iter().find(|(key, _)| *key == provider).map(|(_, value)| value.clone())
    }

    fn set(&mut self, provider: usize, value: EntryValue) {
        match self.values.iter_mut().find(|(key, _)| *key == provider) {
            Some((_, existing)) => *existing = value,
            None => self.values.push((provider, value)),
        }
    }

    fn delete(&mut self, provider: usize) -> bool {
        let before = self.values.len();
        self.values.retain(|(key, _)| *key != provider);
        self.values.len() != before
    }
}

type EntryRef = Rc<RefCell<DecorationEntry>>;

/// A identidade do provedor (a chave do `Map`).
fn provider_key(provider: &Rc<dyn IDecorationsProvider>) -> usize {
    Rc::as_ptr(provider).cast::<()>() as usize
}

/// `DecorationsService`.
pub struct DecorationsService {
    me: Weak<DecorationsService>,
    store: Rc<DisposableStore>,
    on_did_change_decorations_delayed: Rc<DebounceEmitter<Vec<URI>>>,
    on_did_change_decorations: Rc<Emitter<Rc<dyn IResourceDecorationChangeEvent>>>,
    provider: LinkedList<Rc<dyn IDecorationsProvider>>,
    decoration_styles: Rc<DecorationStyles>,
    data: RefCell<TernarySearchTree<URI, EntryRef>>,
}

impl DecorationsService {
    pub fn new(uri_identity_service: Rc<dyn IUriIdentityService>, product_icon_theme: Rc<dyn IProductIconTheme>) -> Rc<Self> {
        let store = DisposableStore::new();
        let on_did_change_decorations_delayed = store.add_rc(DebounceEmitter::new(|all: Vec<Vec<URI>>| all.into_iter().flatten().collect(), None));
        let on_did_change_decorations = store.add_rc(Emitter::<Rc<dyn IResourceDecorationChangeEvent>>::new());
        let ext_uri = uri_identity_service.ext_uri();
        let data = TernarySearchTree::for_uris(Rc::new(move |key: &URI| ext_uri.ignore_path_casing(key)), Rc::new(|_: &URI| false));

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            store: store.clone(),
            on_did_change_decorations_delayed: on_did_change_decorations_delayed.clone(),
            on_did_change_decorations: on_did_change_decorations.clone(),
            provider: LinkedList::new(),
            decoration_styles: DecorationStyles::new(product_icon_theme),
            data: RefCell::new(data),
        });

        let emitter = on_did_change_decorations;
        store.add(on_did_change_decorations_delayed.event().listen(move |event| {
            emitter.fire(Rc::new(FileDecorationChangeEvent::new(event)));
        }));

        this
    }

    /// As regras de estilo geradas, `(seletor, texto)`, a mais nova primeiro (ver as divergências no topo do módulo).
    pub fn style_rules(&self) -> Vec<(String, String)> {
        self.decoration_styles.style_element.rules.borrow().clone()
    }

    /// Remove tudo o que veio do provedor (`removeAll`).
    fn remove_all(&self, provider: usize) {
        let mut uris: Vec<URI> = Vec::new();
        let entries = self.data.borrow().iter_entries();
        for (uri, map) in entries {
            if map.borrow_mut().delete(provider) {
                uris.push(uri);
            }
        }
        if !uris.is_empty() {
            self.on_did_change_decorations_delayed.fire(uris);
        }
    }

    fn ensure_entry(&self, uri: &URI) -> EntryRef {
        let existing = self.data.borrow().get(uri).cloned();
        match existing {
            Some(map) => map,
            None => {
                // nothing known about this uri
                let map: EntryRef = Rc::new(RefCell::new(DecorationEntry::default()));
                self.data.borrow_mut().set(uri.clone(), map.clone());
                map
            }
        }
    }

    fn fetch_data(&self, map: &EntryRef, uri: &URI, provider: &Rc<dyn IDecorationsProvider>) -> Option<DecorationData> {
        let key = provider_key(provider);

        // check for pending request and cancel it
        let pending_request = map.borrow().get(key);
        if let Some(EntryValue::Request(pending_request)) = pending_request {
            pending_request.source.cancel();
            map.borrow_mut().delete(key);
        }

        let cts = CancellationTokenSource::new(None);
        match provider.provide_decorations(uri, cts.token()) {
            MaybePromise::Value(data) => {
                // sync -> we have a result now
                cts.dispose();
                self.keep_item(map, provider, uri, data)
            }
            MaybePromise::Promise(promise) => {
                // async -> we have a result soon
                let request = Rc::new(DecorationDataRequest { source: cts.clone() });
                let is_current = {
                    let map = map.clone();
                    let request = request.clone();
                    move || matches!(map.borrow().get(key), Some(EntryValue::Request(current)) if Rc::ptr_eq(&current, &request))
                };

                let on_fulfilled = {
                    let (this, map, provider, uri, is_current) = (self.me.clone(), map.clone(), provider.clone(), uri.clone(), is_current.clone());
                    move |data: Option<DecorationData>| {
                        if is_current() && let Some(this) = this.upgrade() {
                            this.keep_item(&map, &provider, &uri, data);
                        }
                    }
                };
                let on_rejected = {
                    let map = map.clone();
                    move |err: je_base::errors::Error| {
                        if !is_cancellation_error(&err) && is_current() {
                            map.borrow_mut().delete(key);
                        }
                    }
                };
                promise.then(on_fulfilled).catch(on_rejected).finally(move || cts.dispose());

                map.borrow_mut().set(key, EntryValue::Request(request));
                None
            }
        }
    }

    fn keep_item(&self, map: &EntryRef, provider: &Rc<dyn IDecorationsProvider>, uri: &URI, data: Option<DecorationData>) -> Option<DecorationData> {
        let key = provider_key(provider);
        let old = map.borrow().get(key);
        map.borrow_mut().set(key, EntryValue::Data(data.clone()));
        if data.is_some() || old.is_some() {
            // only fire event when something changed
            self.on_did_change_decorations_delayed.fire(vec![uri.clone()]);
        }
        data
    }
}

impl IDisposable for DecorationsService {
    fn dispose(&self) {
        self.store.dispose();
        self.data.borrow_mut().clear();
    }
}

impl IDecorationsService for DecorationsService {
    fn on_did_change_decorations(&self) -> Event<Rc<dyn IResourceDecorationChangeEvent>> {
        self.on_did_change_decorations.event()
    }

    fn register_decorations_provider(&self, provider: Rc<dyn IDecorationsProvider>) -> DisposableRef {
        let removal = self.provider.unshift(provider.clone());

        self.on_did_change_decorations.fire(Rc::new(EverythingChanged));

        let key = provider_key(&provider);
        let listener = {
            let (this, changed_provider) = (self.me.clone(), provider.clone());
            provider.on_did_change().listen(move |uris| {
                let Some(this) = this.upgrade() else { return };
                // selective changes -> drop for resource, fetch again, send event
                for uri in uris {
                    let map = this.ensure_entry(uri);
                    this.fetch_data(&map, uri, &changed_provider);
                }
            })
        };

        let this = self.me.clone();
        to_disposable(move || {
            removal.remove();
            listener.dispose();
            // remove everything what came from this provider
            if let Some(this) = this.upgrade() {
                this.remove_all(key);
            }
        })
    }

    fn get_decoration(&self, uri: &URI, include_children: bool) -> Option<Rc<dyn IDecoration>> {
        let mut all: Vec<DecorationData> = Vec::new();
        let mut contains_children = false;

        let map = self.ensure_entry(uri);

        let providers: Vec<Rc<dyn IDecorationsProvider>> = self.provider.iter().collect();
        for provider in providers {
            let value = map.borrow().get(provider_key(&provider));
            let data = match value {
                // sets data if fetch is sync
                None => self.fetch_data(&map, uri, &provider),
                Some(EntryValue::Data(data)) => data,
                Some(EntryValue::Request(_)) => None,
            };

            // having data
            if let Some(data) = data {
                all.push(data);
            }
        }

        if include_children {
            // (resolved) children
            let iter = self.data.borrow().find_superstr(uri);
            if let Some(iter) = iter {
                for (_, children) in iter {
                    let values: Vec<EntryValue> = children.borrow().values.iter().map(|(_, value)| value.clone()).collect();
                    for value in values {
                        if let EntryValue::Data(Some(data)) = value
                            && data.bubble.unwrap_or(false)
                        {
                            all.push(data);
                            contains_children = true;
                        }
                    }
                }
            }
        }

        if all.is_empty() { None } else { Some(self.decoration_styles.as_decoration(all, contains_children)) }
    }
}
