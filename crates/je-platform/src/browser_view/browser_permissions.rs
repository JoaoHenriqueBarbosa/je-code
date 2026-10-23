//! Porte de `src/vs/platform/browserView/common/browserPermissions.ts`: o modelo de permissões por
//! origem do navegador integrado, sem interface e sem Electron, no formato das categorias de
//! "Configurações do site" do Chromium e não das strings cruas de permissão do Electron.
//!
//! A lista de strings de permissão do Electron vem do fonte do próprio Electron
//! (`shell/common/gin_converters/content_converter.cc`), a fonte única das strings entregues aos
//! handlers de permissão. Todo `blink::PermissionType` que o Electron não nomeia chega como `'unknown'`.
//!
//! Divergências:
//! - `Record<PermissionCategory, ...>` e `Readonly<Record<...>>` são listas na ordem de
//!   `Object.keys` do literal original ([`PermissionCategory::ALL`]); `getOrigin` devolve
//!   [`PermissionStates`], com a mesma ordem.
//! - `PERMISSION_CATEGORY_DESCRIPTORS` é construído uma vez por thread (os textos passam por
//!   `localize`, que no original roda ao carregar o módulo) e lido por [`permission_category_descriptors`]
//!   e [`permission_category_descriptor`].
//! - `Codicon.<nome>` é o `ThemeIcon` com o id do codicon (`ThemeIcon::from_id("device-camera")`).
//! - `BrowserPermissionStore extends Disposable` guarda um [`Disposable`] em `base`; o `Map` do
//!   JavaScript, que preserva a ordem de inserção, é um `IndexMap` (a remoção usa `shift_remove`,
//!   que mantém a ordem dos demais, como o `Map.delete`).
//! - `hydrate` recebe o valor dinâmico (`ISerializedBrowserPermissionsSnapshot | undefined` vindo do
//!   armazenamento, sem garantia de forma), como o original, que valida `typeof` em cada nível;
//!   [`SerializedBrowserPermissionsSnapshot::to_dynamic`] faz o caminho inverso do `serialize`.
//! - `toOriginKey` usa o `URL` do padrão WHATWG de [`crate::url::whatwg`]. O `trim()` do JavaScript é o
//!   [`je_base::js_string::is_js_whitespace`] (U+FEFF conta, U+0085 não), que não é o
//!   `str::trim` do Rust. A origem de um `blob:` é a da URL interna e a de esquema não especial é opaca,
//!   como no `URL#origin`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, Event};
use je_base::js_string::is_js_whitespace;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::themables::ThemeIcon;

use crate::url::whatwg::try_new_url;

/// `PermissionDecision`: as únicas duas decisões que se persistem. Limpar uma decisão a remove.
///   - `Allow`: concede sem perguntar
///   - `Deny`: rejeita sem perguntar
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
}

impl PermissionDecision {
    /// O texto persistido (`'allow'` ou `'deny'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "allow" => Some(Self::Allow),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

/// `PermissionState`: a decisão gravada ou `Ask` quando não há nenhuma. `Ask` só existe como valor
/// padrão/efetivo, nunca é gravado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    Allow,
    Deny,
    Ask,
}

impl From<PermissionDecision> for PermissionState {
    fn from(decision: PermissionDecision) -> Self {
        match decision {
            PermissionDecision::Allow => Self::Allow,
            PermissionDecision::Deny => Self::Deny,
        }
    }
}

/// `PermissionCategory`: as categorias que o usuário vê, mais grossas e mais significativas que as
/// strings cruas do Electron. A `media` do Electron, por exemplo, vira `Camera` e `Microphone`, e as
/// várias permissões de área de transferência viram `Clipboard`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionCategory {
    Location,
    Camera,
    Microphone,
    Notifications,
    Sensors,
    Clipboard,
    Devices,
}

impl PermissionCategory {
    /// `ALL_PERMISSION_CATEGORIES`: todas as categorias, na ordem estável de exibição (a ordem das
    /// chaves de `PERMISSION_CATEGORY_DESCRIPTORS`, que não é a ordem da declaração do `enum`).
    pub const ALL: [PermissionCategory; 7] = [
        Self::Location,
        Self::Camera,
        Self::Microphone,
        Self::Sensors,
        Self::Clipboard,
        Self::Notifications,
        Self::Devices,
    ];

    /// O valor do `const enum` (`'location'`, `'camera'`...), a chave do que se persiste.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Location => "location",
            Self::Camera => "camera",
            Self::Microphone => "microphone",
            Self::Notifications => "notifications",
            Self::Sensors => "sensors",
            Self::Clipboard => "clipboard",
            Self::Devices => "devices",
        }
    }

    /// O inverso de [`PermissionCategory::as_str`]; `VALID_CATEGORIES.has(text)`.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|category| category.as_str() == text)
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|category| *category == self).expect("toda categoria está em ALL")
    }
}

/// `BrowserDeviceType`: os tipos de fluxo de seleção de dispositivo que a categoria
/// [`PermissionCategory::Devices`] libera. Cada um vem de um evento distinto do Electron, mas todos
/// chegam ao usuário por um único fluxo de pedido e seleção.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserDeviceType {
    Usb,
    Serial,
    Hid,
    Bluetooth,
}

/// `IBrowserDeviceCandidate`: um dispositivo oferecido ao usuário no seletor. Só dados simples e
/// apresentáveis cruzam a fronteira de IPC; o `device_id` opaco volta intacto para escolher o
/// dispositivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserDeviceCandidate {
    /// Identificador opaco, específico do tipo de dispositivo, devolvido para selecioná-lo.
    pub device_id: String,
    /// Rótulo principal, voltado ao usuário (por exemplo o nome do produto).
    pub label: String,
    /// Detalhe secundário opcional (por exemplo fabricante ou ids de fornecedor:produto).
    pub detail: Option<String>,
}

/// `IPermissionCategoryDescriptor`: metadados estáticos de uma categoria e do mapa para as strings
/// cruas do Electron. Uma interface itera [`permission_category_descriptors`] para montar a lista de
/// configurações direto dos dados.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionCategoryDescriptor {
    pub category: PermissionCategory,
    /// Rótulo curto e legível, próprio para uma linha de configuração.
    pub label: String,
    /// Descrição de uma linha do que conceder a categoria habilita.
    pub description: String,
    /// O ícone da categoria.
    pub icon: ThemeIcon,
    /// As strings de permissão do Electron que levam a esta categoria.
    pub permissions: Vec<&'static str>,
    /// O estado assumido quando a origem não gravou decisão.
    pub default_state: PermissionState,
}

/// O descritor de uma categoria. As permissões fora desta lista são sempre concedidas
/// ([`ALWAYS_ALLOWED_PERMISSIONS`]) ou, por padrão, sempre negadas:
///
/// Sem efeito no Electron por falta de backend
///   - Smart Cards (`smart-card`)
///   - NFC (`nfc`)
///   - Conteúdo protegido (`mediaKeySystem`)
///   - Realidade aumentada/virtual, rastreio de mãos (`ar`, `vr`, `hand-tracking`)
///   - Payment Handlers (`payment-handler`)
///   - Sincronização em segundo plano (`background-sync`, `periodic-background-sync`, `background-fetch`)
///   - Impressão (`web-printing`)
///   - Instalação de app (`web-app-installation`)
///   - Acesso ao armazenamento (`storage-access`, `top-level-storage-access`)
///
/// Ainda não implementadas (em ordem aproximada de "talvez queiramos")
///   - Captura de tela, Captured Surface Control (`display-capture`, `captured-surface-control`)
///   - Escrita de arquivo (`fileSystem`)
///   - Abrir externo (`openExternal`)
///   - MIDI (`midi`, `midiSysex`)
///   - Armazenamento persistente (`persistent-storage`)
///   - Atividade do dispositivo (`idle-detection`)
///   - Saída de áudio (`speaker-selection`)
///   - Wake Lock (`screen-wake-lock`, `system-wake-lock`)
///   - Gerenciamento de janelas (`window-management`)
///   - Fontes (`local-fonts`)
///   - Tela cheia automática (`automatic-fullscreen`)
fn build_descriptor(category: PermissionCategory) -> PermissionCategoryDescriptor {
    let (label, description, icon, permissions, default_state) = match category {
        PermissionCategory::Location => (
            localize("browserPermission.location.label", "Location", &[]),
            localize("browserPermission.location.description", "Access this device's geographic location", &[]),
            "location",
            vec!["geolocation", "geolocation-approximate"],
            PermissionState::Ask,
        ),
        PermissionCategory::Camera => (
            localize("browserPermission.camera.label", "Camera", &[]),
            localize("browserPermission.camera.description", "Capture video from cameras", &[]),
            "device-camera",
            // `media` é compartilhada com o Microphone; desambiguada por mediaType/mediaTypes.
            vec!["media"],
            PermissionState::Ask,
        ),
        PermissionCategory::Microphone => (
            localize("browserPermission.microphone.label", "Microphone", &[]),
            localize("browserPermission.microphone.description", "Capture audio from microphones", &[]),
            "mic",
            vec!["media"],
            PermissionState::Ask,
        ),
        PermissionCategory::Sensors => (
            localize("browserPermission.sensors.label", "Sensors", &[]),
            localize("browserPermission.sensors.description", "Read motion and environmental sensors", &[]),
            "pulse",
            vec!["sensors"],
            PermissionState::Allow,
        ),
        PermissionCategory::Clipboard => (
            localize("browserPermission.clipboard.label", "Clipboard", &[]),
            localize("browserPermission.clipboard.description", "Read from and write to the system clipboard", &[]),
            "clippy",
            vec!["clipboard-read"],
            PermissionState::Ask,
        ),
        PermissionCategory::Notifications => (
            localize("browserPermission.notifications.label", "Notifications", &[]),
            localize("browserPermission.notifications.description", "Display desktop notifications", &[]),
            "bell",
            vec!["notifications"],
            PermissionState::Ask,
        ),
        PermissionCategory::Devices => (
            localize("browserPermission.devices.label", "Devices", &[]),
            localize("browserPermission.devices.description", "Request access to USB, serial, HID, and Bluetooth devices", &[]),
            "plug",
            // Cada tipo de dispositivo tem o seu seletor nativo; esta decisão só controla se o seletor
            // pode aparecer. O Bluetooth não tem string de permissão no Electron (é barrado direto no
            // handler do seletor).
            vec!["usb", "serial", "hid"],
            PermissionState::Allow,
        ),
    };
    PermissionCategoryDescriptor { category, label, description, icon: ThemeIcon::from_id(icon), permissions, default_state }
}

thread_local! {
    /// `PERMISSION_CATEGORY_DESCRIPTORS`, na ordem de [`PermissionCategory::ALL`].
    static DESCRIPTORS: Vec<PermissionCategoryDescriptor> = PermissionCategory::ALL.into_iter().map(build_descriptor).collect();

    /// Tabela inversa montada uma vez a partir dos descritores: string do Electron para as categorias
    /// que a possuem. A `media` fica de fora de propósito: pede `details` para desambiguar e é tratada
    /// à parte em [`electron_permission_to_categories`].
    static PERMISSION_TO_CATEGORIES: HashMap<&'static str, Vec<PermissionCategory>> = {
        let mut map: HashMap<&'static str, Vec<PermissionCategory>> = HashMap::new();
        DESCRIPTORS.with(|descriptors| {
            for descriptor in descriptors {
                for permission in &descriptor.permissions {
                    if *permission != "media" {
                        map.entry(*permission).or_default().push(descriptor.category);
                    }
                }
            }
        });
        map
    };
}

/// `PERMISSION_CATEGORY_DESCRIPTORS` como lista, na ordem estável de exibição.
pub fn permission_category_descriptors() -> Vec<PermissionCategoryDescriptor> {
    DESCRIPTORS.with(Clone::clone)
}

/// `PERMISSION_CATEGORY_DESCRIPTORS[category]`.
pub fn permission_category_descriptor(category: PermissionCategory) -> PermissionCategoryDescriptor {
    DESCRIPTORS.with(|descriptors| descriptors[category.index()].clone())
}

/// `ALWAYS_ALLOWED_PERMISSIONS`: as strings cruas do Electron concedidas sem condição, sem estado
/// gravado e sem controle de gerenciamento.
pub const ALWAYS_ALLOWED_PERMISSIONS: &[&str] = &[
    // O Electron desliga o LocalNetworkAccessChecks, então o acesso à rede local e ao loopback já é
    // permitido. Informe esse estado efetivo também ao permissions.query(): uma negação falsa quebra
    // sites que consultam antes, como o Okta FastPass.
    // https://github.com/electron/electron/issues/48655
    "local-network-access",
    "local-network",
    "loopback-network",
    // São capacidades de baixo risco que o próprio Chrome também concede automaticamente.
    "pointerLock",
    "keyboardLock",
    "fullscreen",
    "clipboard-sanitized-write",
];

/// `isAlwaysAllowedPermission`: se uma string crua do Electron é concedida sem condição.
pub fn is_always_allowed_permission(permission: &str) -> bool {
    ALWAYS_ALLOWED_PERMISSIONS.contains(&permission)
}

/// Um item de `mediaKinds` (`'video' | 'audio'`), a dica normalizada que o chamador extrai dos
/// detalhes do Electron.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Audio,
}

/// `electronPermissionToCategories`: leva uma string crua do Electron (de qualquer um dos handlers,
/// ou de um tipo de dispositivo) à categoria ou categorias que ela representa.
///
/// - `media` resolve para `Camera`, `Microphone` ou as duas, conforme a dica `media_kinds` que o
///   chamador extraiu dos detalhes do Electron. Sem dica, resolve conservadoramente para as duas, para
///   o chamador exigir a decisão mais estrita.
/// - `unknown` (e qualquer coisa não reconhecida) resolve para uma lista vazia.
pub fn electron_permission_to_categories(permission: &str, media_kinds: Option<&[MediaKind]>) -> Vec<PermissionCategory> {
    if permission == "media" {
        return resolve_media_categories(media_kinds);
    }
    PERMISSION_TO_CATEGORIES.with(|map| map.get(permission).cloned().unwrap_or_default())
}

fn resolve_media_categories(media_kinds: Option<&[MediaKind]>) -> Vec<PermissionCategory> {
    // O `Set` do original: sem repetição, na ordem em que as categorias entram.
    let mut categories = Vec::new();
    for kind in media_kinds.unwrap_or_default() {
        let category = match kind {
            MediaKind::Video => PermissionCategory::Camera,
            MediaKind::Audio => PermissionCategory::Microphone,
        };
        if !categories.contains(&category) {
            categories.push(category);
        }
    }
    // Sem dica utilizável: assume as duas, para os chamadores aplicarem a decisão mais estrita.
    if categories.is_empty() {
        return vec![PermissionCategory::Camera, PermissionCategory::Microphone];
    }
    categories
}

/// `String.prototype.trim` do JavaScript.
fn trim_js(text: &str) -> &str {
    text.trim_matches(|c: char| u16::try_from(c as u32).is_ok_and(is_js_whitespace))
}

/// `toOriginKey`: normaliza uma URL para uma chave de origem estável, mantendo a URL inteira (sem
/// consulta nem fragmento) para arquivos. Origens opacas não têm chave; entradas que não são URL caem
/// no próprio valor sem espaços nas pontas.
pub fn to_origin_key(url: Option<&str>) -> String {
    // Apara primeiro, para espaços nas pontas não jogarem URLs válidas no caminho do erro. O Electron
    // também informa origens opacas/únicas (frames em sandbox, URLs `data:` etc.) como a string literal
    // "null"; isso não é um host de verdade, então vale como "sem origem", para os padrões das categorias
    // não se aplicarem a ela.
    let trimmed = trim_js(url.unwrap_or_default());
    if trimmed.is_empty() || trimmed == "null" {
        return String::new();
    }
    let Some(mut parsed) = try_new_url(&JsString::from(trimmed)) else {
        return trimmed.to_owned();
    };
    if parsed.scheme() == schemas::FILE {
        parsed.set_query(None);
        parsed.set_fragment(None);
        return parsed.into();
    }
    // `origin === 'null'` (origem opaca) vira "sem chave".
    let origin = parsed.origin();
    if origin.is_tuple() { origin.ascii_serialization() } else { String::new() }
}

/// `IPermissionGrant`: uma concessão gravada (qual origem, qual categoria, qual decisão).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionGrant {
    pub origin: String,
    pub category: PermissionCategory,
    pub state: PermissionDecision,
}

/// `IPermissionCategoryState`: um par (categoria, decisão) para uma origem implícita, a carga da API
/// de escrita. `state: None` (o `null` do original) limpa a escolha gravada e volta ao padrão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionCategoryState {
    pub category: PermissionCategory,
    pub state: Option<PermissionDecision>,
}

/// O retorno de `getOrigin` (`Record<PermissionCategory, PermissionState>`): todas as categorias, na
/// ordem de [`PermissionCategory::ALL`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionStates(Vec<(PermissionCategory, PermissionState)>);

impl PermissionStates {
    /// `record[category]`.
    pub fn get(&self, category: PermissionCategory) -> PermissionState {
        self.0.iter().find(|(candidate, _)| *candidate == category).map(|(_, state)| *state).expect("o registro tem todas as categorias")
    }

    /// As entradas na ordem das chaves do `Record`.
    pub fn iter(&self) -> impl Iterator<Item = (PermissionCategory, PermissionState)> + '_ {
        self.0.iter().copied()
    }
}

/// `ISerializedBrowserPermissionsSnapshot`: o formato em disco do armazenamento inteiro,
/// `{ origin: { category: decisão } }`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SerializedBrowserPermissionsSnapshot {
    pub origins: IndexMap<String, IndexMap<PermissionCategory, PermissionDecision>>,
}

impl SerializedBrowserPermissionsSnapshot {
    /// O objeto `{ origins: { origem: { categoria: decisão } } }`, o que `hydrate` lê.
    pub fn to_dynamic(&self) -> Dynamic {
        let origins = self.origins.iter().map(|(origin, categories)| {
            let entry = categories.iter().map(|(category, state)| (category.as_str(), Dynamic::string(state.as_str())));
            (origin.as_str(), Dynamic::object(entry))
        });
        Dynamic::object([("origins", Dynamic::object(origins))])
    }
}

/// `typeof value === 'object'` já descontado o `null`/`undefined`: o que `hydrate` aceita descer.
fn is_object_like(value: &Dynamic) -> bool {
    value.is_truthy() && value.type_of() == "object"
}

/// `BrowserPermissionStore`: o rastreador em memória e serializável do estado de permissão por
/// (origem, categoria). O processo principal guarda a instância autoritativa; o workbench mantém um
/// espelho de leitura hidratado a partir do armazenamento. Mude com `set`/`set_many`, observe com
/// `on_did_change` e persista com `serialize`/`hydrate`.
pub struct BrowserPermissionStore {
    base: Rc<Disposable>,
    data: RefCell<IndexMap<String, IndexMap<PermissionCategory, PermissionDecision>>>,
    on_did_change: Rc<Emitter<()>>,
}

impl BrowserPermissionStore {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change = Emitter::new();
        base.register(on_did_change.clone());
        Rc::new(Self { base, data: RefCell::new(IndexMap::new()), on_did_change })
    }

    /// `onDidChange`.
    pub fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    /// O estado padrão assumido para uma categoria quando a origem não gravou decisão.
    pub fn default_state_for(&self, category: PermissionCategory) -> PermissionState {
        DESCRIPTORS.with(|descriptors| descriptors[category.index()].default_state)
    }

    /// A decisão gravada para um par (origem, categoria).
    pub fn get_decision(&self, origin: &str, category: PermissionCategory) -> Option<PermissionDecision> {
        self.data.borrow().get(&to_origin_key(Some(origin)))?.get(&category).copied()
    }

    /// Resolve a decisão booleana efetiva de um par (origem, categoria), aplicando
    /// [`BrowserPermissionStore::default_state_for`] quando o estado gravado é `ask`.
    pub fn is_allowed(&self, origin: &str, category: PermissionCategory) -> bool {
        let state = self.get_decision(origin, category).map_or_else(|| self.default_state_for(category), PermissionState::from);
        state == PermissionState::Allow
    }

    /// Grava (ou limpa, com `None`) a decisão de um par (origem, categoria).
    pub fn set(&self, origin: &str, category: PermissionCategory, decision: Option<PermissionDecision>) {
        let key = to_origin_key(Some(origin));
        if key.is_empty() {
            return;
        }
        let changed = {
            let mut data = self.data.borrow_mut();
            match decision {
                None => match data.get_mut(&key) {
                    Some(categories) if categories.shift_remove(&category).is_some() => {
                        if categories.is_empty() {
                            data.shift_remove(&key);
                        }
                        true
                    }
                    // nada mudou
                    _ => false,
                },
                Some(decision) => {
                    let unchanged = data.get(&key).and_then(|categories| categories.get(&category)) == Some(&decision);
                    if !unchanged {
                        data.entry(key).or_default().insert(category, decision);
                    }
                    !unchanged
                }
            }
        };
        if changed {
            self.on_did_change.fire(());
        }
    }

    /// Grava (ou limpa) a decisão de várias categorias de uma origem de uma vez.
    pub fn set_many(&self, origin: &str, grants: impl IntoIterator<Item = PermissionCategoryState>) {
        for PermissionCategoryState { category, state } in grants {
            self.set(origin, category, state);
        }
    }

    /// Remove todo o estado gravado de uma origem.
    pub fn clear_origin(&self, origin: &str) {
        let removed = self.data.borrow_mut().shift_remove(&to_origin_key(Some(origin))).is_some();
        if removed {
            self.on_did_change.fire(());
        }
    }

    /// Remove todo o estado gravado de todas as origens.
    pub fn clear(&self) {
        {
            let mut data = self.data.borrow_mut();
            if data.is_empty() {
                return;
            }
            data.clear();
        }
        self.on_did_change.fire(());
    }

    /// O mapa categoria->estado completo de uma origem, incluindo as categorias sem decisão gravada.
    /// Ideal para desenhar a página de configurações de um site.
    pub fn get_origin(&self, origin: &str) -> PermissionStates {
        let data = self.data.borrow();
        let recorded = data.get(&to_origin_key(Some(origin)));
        PermissionStates(
            PermissionCategory::ALL
                .into_iter()
                .map(|category| {
                    let state = recorded.and_then(|categories| categories.get(&category)).map_or_else(|| self.default_state_for(category), |decision| (*decision).into());
                    (category, state)
                })
                .collect(),
        )
    }

    /// Todas as origens com pelo menos uma decisão gravada.
    pub fn origins(&self) -> Vec<String> {
        self.data.borrow().keys().cloned().collect()
    }

    /// A lista plana de todas as concessões gravadas.
    pub fn list(&self) -> Vec<PermissionGrant> {
        self.data
            .borrow()
            .iter()
            .flat_map(|(origin, categories)| categories.iter().map(|(category, state)| PermissionGrant { origin: origin.clone(), category: *category, state: *state }))
            .collect()
    }

    pub fn serialize(&self) -> SerializedBrowserPermissionsSnapshot {
        SerializedBrowserPermissionsSnapshot { origins: self.data.borrow().clone() }
    }

    /// Substitui o estado pelo do `snapshot` (`undefined` esvazia). Ignora o que não tem a forma
    /// esperada: origens sem chave, categorias desconhecidas e decisões que não são `allow`/`deny`.
    pub fn hydrate(&self, snapshot: &Dynamic) {
        let mut data = self.data.borrow_mut();
        data.clear();
        let origins = if snapshot.is_nullish() { Dynamic::Undefined } else { snapshot.get_str("origins") };
        if is_object_like(&origins) {
            for (origin, categories) in origins.own_entries() {
                if !is_object_like(&categories) {
                    continue;
                }
                let key = to_origin_key(Some(&origin.to_string_lossy()));
                if key.is_empty() {
                    continue;
                }
                let mut target: Option<IndexMap<PermissionCategory, PermissionDecision>> = None;
                for (category, state) in categories.own_entries() {
                    let category = PermissionCategory::parse(&category.to_string_lossy());
                    let decision = match &state {
                        Dynamic::String(text) => PermissionDecision::parse(&text.to_string_lossy()),
                        _ => None,
                    };
                    let (Some(category), Some(decision)) = (category, decision) else { continue };
                    target.get_or_insert_with(IndexMap::new).insert(category, decision);
                }
                if let Some(target) = target {
                    // `this._data.set(key, target)` acontece na primeira categoria válida: duas origens que
                    // normalizam para a mesma chave se sobrescrevem, como no `Map`.
                    data.insert(key, target);
                }
            }
        }
        drop(data);
        self.on_did_change.fire(());
    }
}

impl IDisposable for BrowserPermissionStore {
    fn dispose(&self) {
        self.base.dispose();
    }
}
