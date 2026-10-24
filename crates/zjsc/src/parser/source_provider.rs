//! Tradução de `parser/SourceProvider.{h,cpp}`.
//!
//! Modelo: a classe virtual vira o trait `SourceProvider`. O estado que o C++ guarda na classe base
//! (`m_sourceOrigin`, `m_sourceURL`, `m_id` etc.) fica em `SourceProviderBase`, que cada
//! implementação carrega e expõe por `base()`; os métodos não virtuais viram métodos fornecidos do
//! trait. O `ThreadSafeRefCounted` vira `Rc<dyn SourceProvider>` (o porte roda numa thread só, e os
//! `std::atomic` e o `Lock` viram `Cell` e `RefCell`).
//!
//! Fora desta fatia, e por quê:
//!
//! - `cachedBytecode`, `cacheBytecode`, `updateCache`, `commitCachedBytecode`: dependem de
//!   `CachedBytecode`, `UnlinkedFunctionExecutable` e `UnlinkedFunctionCodeBlock` (camada do
//!   bytecode); os padrões do C++ não fazem nada ou devolvem nulo, então nada observável some;
//! - `didGenerateUnlinkedCodeBlock` (Bun): depende de `VM`, `SourceCodeKey` e `UnlinkedCodeBlock`;
//!   o padrão é vazio;
//! - `codeBlockHashConcurrently`: depende de `CodeBlockHash` (`bytecode/CodeBlockHash.cpp`);
//! - `SyntheticSourceProvider`: depende de `JSGlobalObject`, `Identifier`, `MarkedArgumentBuffer` e
//!   `JSObject` (camada do runtime).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};

use crate::parser::position_map::PositionMap;
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::runtime::host::{host, HostFile};
use crate::runtime::source_origin::SourceOrigin;
use crate::wtf::text::string_impl::StringImpl;
use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::text_position::TextPosition;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url::URL;

/// `SourceID` (`intptr_t`).
pub type SourceID = isize;

/// `SourceProviderSourceType`, com `BunTranspiledModule` (`USE(BUN_JSC_ADDITIONS)` ligado).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceProviderSourceType {
    Program = 0,
    Module = 1,
    WebAssembly = 2,
    JSON = 3,
    Text = 4,
    Synthetic = 5,
    ImportMap = 6,
    BunTranspiledModule = 7,
}

/// `SourceProvider::nullID`.
pub const NULL_ID: SourceID = 1;

/// O estado da classe base `SourceProvider`.
#[derive(Debug)]
pub struct SourceProviderBase {
    locking_count: Cell<u32>,
    source_type: SourceProviderSourceType,
    source_origin: SourceOrigin,
    source_url: WtfString,
    source_url_stripped: RefCell<WtfString>,
    pre_redirect_url: WtfString,
    source_url_directive: RefCell<WtfString>,
    source_mapping_url_directive: RefCell<WtfString>,
    start_position: TextPosition,
    id: Cell<SourceID>,
    taintedness: Cell<SourceTaintedOrigin>,
    /// `m_sourceCodeDumped` e `m_sourceCodeDumpFilePath`: `None` enquanto não houve despejo.
    source_code_dump_file_path: RefCell<Option<Vec<u8>>>,
    /// Mapa do texto executado para o original (`position_map.rs`); só o harness dos goldens o põe.
    position_map: RefCell<Option<Rc<PositionMap>>>,
    /// O fonte não passou pelo transpilador do bun (o que `vm.runInThisContext` avalia): a coluna das frames fica a
    /// crua do JSC, sem o recuo `callee_back_offset`. Só o harness dos goldens o liga.
    raw_columns: Cell<bool>,
    /// Fonte do próprio hospedeiro que a página não pode enxergar (o DOM do navegador escrito em JavaScript): toda
    /// função dele nasce `ImplementationVisibility::Private`, como as builtins do JSC.
    hidden: Cell<bool>,
    /// O fonte passou pelo transpilador do bun, que tira os comentários: as diretivas `//# sourceURL=` e
    /// `//# sourceMappingURL=` não chegam ao motor (medido no bun 1.4.2, o arquivo principal não se renomeia).
    comments_stripped: Cell<bool>,
    /// No formato de pilha do V8, o fonte de `eval` e de `new Function` guarda de onde veio (`eval at f (url:L:C)`),
    /// que os frames dele escrevem no lugar da URL.
    eval_origin: RefCell<Option<String>>,
    /// O fonte veio de um `eval` indireto: a pilha mostra a origem do chamador, mas o `sourceURL` próprio dos erros não.
    eval_code: Cell<bool>,
}

impl SourceProviderBase {
    /// `SourceProvider::SourceProvider(origin, sourceURL, preRedirectURL, taintedness, startPosition, sourceType)`.
    pub fn new(
        source_origin: SourceOrigin,
        source_url: WtfString,
        pre_redirect_url: WtfString,
        taintedness: SourceTaintedOrigin,
        start_position: TextPosition,
        source_type: SourceProviderSourceType,
    ) -> SourceProviderBase {
        SourceProviderBase {
            locking_count: Cell::new(0),
            source_type,
            source_origin,
            source_url,
            source_url_stripped: RefCell::new(WtfString::default()),
            pre_redirect_url,
            source_url_directive: RefCell::new(WtfString::default()),
            source_mapping_url_directive: RefCell::new(WtfString::default()),
            start_position,
            id: Cell::new(0),
            taintedness: Cell::new(taintedness),
            source_code_dump_file_path: RefCell::new(None),
            position_map: RefCell::new(None),
            raw_columns: Cell::new(false),
            hidden: Cell::new(false),
            comments_stripped: Cell::new(false),
            eval_origin: RefCell::new(None),
            eval_code: Cell::new(false),
        }
    }
}

thread_local! {
    /// `std::atomic<SourceID> nextProviderID = nullID` de `SourceProvider::getID`. Um contador por thread (o motor é
    /// de uma thread), que recomeça a cada programa: o primeiro script é o 2, como no processo novo do bun 1.4.2
    /// (`CallSite.getScriptId`).
    static NEXT_PROVIDER_ID: Cell<SourceID> = const { Cell::new(NULL_ID) };
}

/// Fim do programa (`cell_registry::reset_program_state`): o próximo programa numera os scripts de novo.
pub(crate) fn reset_for_program() {
    let _ = NEXT_PROVIDER_ID.try_with(|next| next.set(NULL_ID));
}

/// `class SourceProvider`.
pub trait SourceProvider: std::fmt::Debug {
    /// O estado da classe base.
    fn base(&self) -> &SourceProviderBase;

    /// `hash()`, virtual puro.
    fn hash(&self) -> u32;

    /// `source()`, virtual puro. O `StringView` vira `String` (compartilha o `StringImpl`).
    fn source(&self) -> WtfString;

    /// Mapa de posições para o texto original (o `SavedSourceMap` do bun); `None` quando o texto é o original.
    fn position_map(&self) -> Option<Rc<PositionMap>> {
        self.base().position_map.borrow().clone()
    }

    /// `true` quando o fonte não passou pelo transpilador (ver `raw_columns`).
    fn has_raw_columns(&self) -> bool {
        self.base().raw_columns.get()
    }

    /// Marca o fonte como não transpilado.
    fn set_raw_columns(&self) {
        self.base().raw_columns.set(true);
    }

    /// `true` quando o fonte é do hospedeiro e se comporta como builtin (ver `hidden`).
    fn is_hidden(&self) -> bool {
        self.base().hidden.get()
    }

    /// Marca o fonte como builtin do hospedeiro.
    fn set_hidden(&self) {
        self.base().hidden.set(true);
    }

    /// Instala o mapa de posições.
    fn set_position_map(&self, map: Rc<PositionMap>) {
        *self.base().position_map.borrow_mut() = Some(map);
    }

    /// De onde veio o fonte de `eval` ou `new Function` (`eval at f (url:L:C)`), no formato de pilha do V8.
    fn eval_origin(&self) -> Option<String> {
        self.base().eval_origin.borrow().clone()
    }

    /// O fonte veio de um `eval` indireto (ver `mark_eval_code`).
    fn is_eval_code(&self) -> bool {
        self.base().eval_code.get()
    }

    /// Marca o fonte como código de `eval` indireto.
    fn mark_eval_code(&self) {
        self.base().eval_code.set(true);
    }

    /// Grava a origem do `eval` (ver `eval_origin`).
    fn set_eval_origin(&self, origin: String) {
        *self.base().eval_origin.borrow_mut() = Some(origin);
    }

    /// `memoryCost()` (Bun).
    fn memory_cost(&self) -> usize {
        0
    }

    /// `isScriptBufferSourceProvider()`.
    fn is_script_buffer_source_provider(&self) -> bool {
        false
    }

    /// `lockUnderlyingBufferImpl()`, virtual privado.
    fn lock_underlying_buffer_impl(&self) {}

    /// `unlockUnderlyingBufferImpl()`, virtual privado.
    fn unlock_underlying_buffer_impl(&self) {}

    /// `getRange(int start, int end)`.
    fn get_range(&self, start: i32, end: i32) -> WtfString {
        self.source().substring(start as u32, end.wrapping_sub(start) as u32)
    }

    /// `sourceOrigin()`.
    fn source_origin(&self) -> &SourceOrigin {
        &self.base().source_origin
    }

    /// `sourceURL()`. This is NOT the path that should be used for computing relative paths from a
    /// script. Use SourceOrigin's URL for that, the values may or may not be the same.
    fn source_url(&self) -> &WtfString {
        &self.base().source_url
    }

    /// `sourceURLStripped()`.
    fn source_url_stripped(&self) -> WtfString {
        let base = self.base();
        if base.source_url.is_null() {
            return base.source_url_stripped.borrow().clone();
        }
        if !base.source_url_stripped.borrow().is_null() {
            return base.source_url_stripped.borrow().clone();
        }
        let stripped = URL::from_string(&base.source_url).stripped_for_use_as_report();
        *base.source_url_stripped.borrow_mut() = stripped.clone();
        stripped
    }

    /// `preRedirectURL()`.
    fn pre_redirect_url(&self) -> &WtfString {
        &self.base().pre_redirect_url
    }

    /// `sourceURLDirective()`.
    fn source_url_directive(&self) -> WtfString {
        self.base().source_url_directive.borrow().clone()
    }

    /// `sourceMappingURLDirective()`.
    fn source_mapping_url_directive(&self) -> WtfString {
        self.base().source_mapping_url_directive.borrow().clone()
    }

    /// `startPosition()`.
    fn start_position(&self) -> TextPosition {
        self.base().start_position
    }

    /// `sourceType()`.
    fn source_type(&self) -> SourceProviderSourceType {
        self.base().source_type
    }

    /// `isModuleType()`.
    fn is_module_type(&self) -> bool {
        matches!(
            self.base().source_type,
            SourceProviderSourceType::Module
                | SourceProviderSourceType::JSON
                | SourceProviderSourceType::Text
                | SourceProviderSourceType::BunTranspiledModule
        )
    }

    /// `asID()`.
    fn as_id(&self) -> SourceID {
        let base = self.base();
        if base.id.get() == 0 {
            // getID()
            let id = NEXT_PROVIDER_ID.with(|next| {
                next.set(next.get() + 1);
                next.get()
            });
            base.id.set(id);
            assert!(id != 0);
        }
        base.id.get()
    }

    /// `setSourceURLDirective(const String&)`.
    fn set_source_url_directive(&self, source_url_directive: &WtfString) {
        if !self.base().comments_stripped.get() {
            *self.base().source_url_directive.borrow_mut() = source_url_directive.clone();
        }
    }

    /// `setSourceMappingURLDirective(const String&)`.
    fn set_source_mapping_url_directive(&self, source_mapping_url_directive: &WtfString) {
        if !self.base().comments_stripped.get() {
            *self.base().source_mapping_url_directive.borrow_mut() = source_mapping_url_directive.clone();
        }
    }

    /// O fonte veio do transpilador do bun (ver `comments_stripped`): as diretivas de comentário são ignoradas.
    fn set_comments_stripped(&self) {
        self.base().comments_stripped.set(true);
    }

    /// `setSourceTaintedOrigin(SourceTaintedOrigin)`.
    fn set_source_tainted_origin(&self, taintedness: SourceTaintedOrigin) {
        self.base().taintedness.set(taintedness);
    }

    /// `sourceTaintedOrigin()`.
    fn source_tainted_origin(&self) -> SourceTaintedOrigin {
        self.base().taintedness.get()
    }

    /// `couldBeTainted()`.
    fn could_be_tainted(&self) -> bool {
        self.base().taintedness.get() != SourceTaintedOrigin::Untainted
    }

    /// `lockUnderlyingBuffer()`.
    fn lock_underlying_buffer(&self) {
        let count = self.base().locking_count.get();
        self.base().locking_count.set(count + 1);
        if count == 0 {
            self.lock_underlying_buffer_impl();
        }
    }

    /// `unlockUnderlyingBuffer()`.
    fn unlock_underlying_buffer(&self) {
        let count = self.base().locking_count.get() - 1;
        self.base().locking_count.set(count);
        if count == 0 {
            self.unlock_underlying_buffer_impl();
        }
    }

    /// `sourceCodeDumpFilePath(const CString& dumpDirectory)`. O `CString` vira bytes; o `CString`
    /// nulo (sem diretório) é `None`.
    fn source_code_dump_file_path(&self, dump_directory: Option<&[u8]>) -> Vec<u8> {
        let base = self.base();
        if let Some(path) = base.source_code_dump_file_path.borrow().as_ref() {
            return path.clone();
        }

        let local_path = try_extract_local_path(self.source_url());

        let mut dump_file_path: Vec<u8> = Vec::new();
        if !local_path.is_null() {
            dump_file_path = local_path.utf8(ConversionMode::LenientConversion);
        } else {
            let base_name = format!("source-{}-{}", self.as_id(), host().pid());
            let opened = match dump_directory {
                None => open_temporary_file(&base_name, ".js"),
                Some(directory) => {
                    let file_path = format!("{}/{}.js", String::from_utf8_lossy(directory), base_name);
                    host().create_file(&file_path, false).ok().map(|handle| (file_path, handle))
                }
            };
            if let Some((file_path, mut handle)) = opened {
                let source_text = self.source().utf8(ConversionMode::LenientConversion);
                let _ = handle.write_all(&source_text);
                dump_file_path = file_path.into_bytes();
            }
        }

        *base.source_code_dump_file_path.borrow_mut() = Some(dump_file_path.clone());
        dump_file_path
    }
}

/// O lambda `tryExtractLocalPath` de `sourceCodeDumpFilePath`.
fn try_extract_local_path(url_string: &WtfString) -> WtfString {
    if url_string.is_null() {
        return WtfString::default();
    }
    if url_string.length() > 0 && url_string.code_unit_at(0) == '/' as u16 {
        return url_string.clone();
    }
    let prefix = b"file://";
    if url_string.length() as usize >= prefix.len()
        && prefix.iter().enumerate().all(|(i, c)| url_string.code_unit_at(i as u32) == *c as u16)
    {
        return URL::from_string(url_string).file_system_path();
    }
    WtfString::default()
}

/// `FileSystem::openTemporaryFile(prefix, suffix)`: cria um arquivo novo e exclusivo no diretório
/// temporário (`prefix`, seis caracteres únicos e `suffix`).
fn open_temporary_file(prefix: &str, suffix: &str) -> Option<(String, Box<dyn HostFile>)> {
    static COUNTER: AtomicIsize = AtomicIsize::new(0);
    let directory = host().getenv("TMPDIR").filter(|directory| !directory.is_empty()).unwrap_or_else(|| String::from("/tmp"));
    for _ in 0..64 {
        let serial = COUNTER.fetch_add(1, Ordering::SeqCst) as u64;
        let nanos = crate::wtf::date_math::current_time_in_nanoseconds().rem_euclid(1_000_000_000) as u64;
        let mut mix = serial.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ nanos;
        let alphabet = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let mut token = String::new();
        for _ in 0..6 {
            token.push(alphabet[(mix % alphabet.len() as u64) as usize] as char);
            mix /= alphabet.len() as u64;
            mix ^= mix.rotate_left(7) ^ nanos;
        }
        let path = format!("{}/{}{}{}", directory, prefix, token, suffix);
        if let Ok(handle) = host().create_file(&path, true) {
            return Some((path, handle));
        }
    }
    None
}

/// `class StringSourceProvider`.
#[derive(Debug)]
pub struct StringSourceProvider {
    base: SourceProviderBase,
    source: Rc<StringImpl>,
}

impl StringSourceProvider {
    /// `StringSourceProvider::create(source, sourceOrigin, sourceURL, taintedness, startPosition, sourceType)`.
    /// Os argumentos padrão do C++ (`TextPosition()` e `SourceProviderSourceType::Program`) são
    /// passados por quem chama.
    pub fn create(
        source: &WtfString,
        source_origin: &SourceOrigin,
        source_url: WtfString,
        taintedness: SourceTaintedOrigin,
        start_position: TextPosition,
        source_type: SourceProviderSourceType,
    ) -> Rc<StringSourceProvider> {
        let source = match source.impl_() {
            Some(string) => Rc::clone(string),
            None => StringImpl::empty(),
        };
        Rc::new(StringSourceProvider {
            base: SourceProviderBase::new(
                source_origin.clone(),
                source_url,
                WtfString::default(),
                taintedness,
                start_position,
                source_type,
            ),
            source,
        })
    }
}

impl SourceProvider for StringSourceProvider {
    fn base(&self) -> &SourceProviderBase {
        &self.base
    }

    fn hash(&self) -> u32 {
        self.source.hash()
    }

    fn source(&self) -> WtfString {
        WtfString::from(Rc::clone(&self.source))
    }
}

/// `class BaseWebAssemblySourceProvider` (`ENABLE(WEBASSEMBLY)`).
pub trait BaseWebAssemblySourceProvider: SourceProvider {
    /// `data()`.
    fn data(&self) -> &[u8];

    /// `size()`.
    fn size(&self) -> usize;
}

/// O construtor protegido de `BaseWebAssemblySourceProvider`.
pub fn base_web_assembly_source_provider_base(source_origin: &SourceOrigin, source_url: WtfString) -> SourceProviderBase {
    SourceProviderBase::new(
        source_origin.clone(),
        source_url,
        WtfString::default(),
        SourceTaintedOrigin::Untainted,
        TextPosition::default(),
        SourceProviderSourceType::WebAssembly,
    )
}

/// `class WebAssemblySourceProvider`.
#[derive(Debug)]
pub struct WebAssemblySourceProvider {
    base: SourceProviderBase,
    source: WtfString,
    data: Vec<u8>,
}

impl WebAssemblySourceProvider {
    /// `WebAssemblySourceProvider::create(Vector<uint8_t>&&, sourceOrigin, sourceURL)`.
    pub fn create(data: Vec<u8>, source_origin: &SourceOrigin, source_url: WtfString) -> Rc<WebAssemblySourceProvider> {
        Rc::new(WebAssemblySourceProvider {
            base: base_web_assembly_source_provider_base(source_origin, source_url),
            source: WtfString::from_latin1(b"[WebAssembly source]"),
            data,
        })
    }

    /// `dataVector()`.
    pub fn data_vector(&self) -> &Vec<u8> {
        &self.data
    }
}

impl SourceProvider for WebAssemblySourceProvider {
    fn base(&self) -> &SourceProviderBase {
        &self.base
    }

    fn hash(&self) -> u32 {
        self.source.hash()
    }

    fn source(&self) -> WtfString {
        self.source.clone()
    }
}

impl BaseWebAssemblySourceProvider for WebAssemblySourceProvider {
    fn data(&self) -> &[u8] {
        &self.data
    }

    fn size(&self) -> usize {
        self.data.len()
    }
}

/// `WebAssemblySourceProviderBufferGuard`: RAII do buffer de um provedor wasm.
pub struct WebAssemblySourceProviderBufferGuard {
    source_provider: Option<Rc<dyn BaseWebAssemblySourceProvider>>,
}

impl WebAssemblySourceProviderBufferGuard {
    pub fn new(source_provider: Option<Rc<dyn BaseWebAssemblySourceProvider>>) -> WebAssemblySourceProviderBufferGuard {
        if let Some(provider) = &source_provider {
            provider.lock_underlying_buffer();
        }
        WebAssemblySourceProviderBufferGuard { source_provider }
    }
}

impl Drop for WebAssemblySourceProviderBufferGuard {
    fn drop(&mut self) {
        if let Some(provider) = &self.source_provider {
            provider.unlock_underlying_buffer();
        }
    }
}

/// `SourceProviderBufferGuard`: RAII do buffer de um provedor. O C++ guarda ponteiro cru de
/// propósito (o compilador concorrente garante a vida por outro meio); aqui o `Rc` garante.
pub struct SourceProviderBufferGuard {
    source_provider: Option<Rc<dyn SourceProvider>>,
}

impl SourceProviderBufferGuard {
    pub fn new(source_provider: Option<Rc<dyn SourceProvider>>) -> SourceProviderBufferGuard {
        if let Some(provider) = &source_provider {
            provider.lock_underlying_buffer();
        }
        SourceProviderBufferGuard { source_provider }
    }

    /// `provider()`.
    pub fn provider(&self) -> Option<&Rc<dyn SourceProvider>> {
        self.source_provider.as_ref()
    }
}

impl Drop for SourceProviderBufferGuard {
    fn drop(&mut self) {
        if let Some(provider) = &self.source_provider {
            provider.unlock_underlying_buffer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> WtfString {
        WtfString::from_latin1(value.as_bytes())
    }

    fn make(source: &str) -> Rc<StringSourceProvider> {
        StringSourceProvider::create(
            &text(source),
            &SourceOrigin::default(),
            text("https://u:p@example.com/a.js?x=1"),
            SourceTaintedOrigin::Untainted,
            TextPosition::default(),
            SourceProviderSourceType::Program,
        )
    }

    #[test]
    fn range_and_hash() {
        let provider = make("var a = 1;");
        assert_eq!(provider.get_range(4, 5), text("a"));
        assert_eq!(provider.hash(), text("var a = 1;").hash());
        assert!(!provider.is_module_type());
        assert!(!provider.could_be_tainted());
    }

    #[test]
    fn ids_are_unique_and_stable() {
        let a = make("a");
        let b = make("b");
        assert!(a.as_id() > NULL_ID);
        assert_eq!(a.as_id(), a.as_id());
        assert_ne!(a.as_id(), b.as_id());
    }

    #[test]
    fn stripped_url() {
        let provider = make("");
        assert_eq!(provider.source_url_stripped(), text("https://example.com/a.js"));
    }

    #[test]
    fn null_source_is_empty() {
        let provider = StringSourceProvider::create(
            &WtfString::default(),
            &SourceOrigin::default(),
            WtfString::default(),
            SourceTaintedOrigin::Untainted,
            TextPosition::default(),
            SourceProviderSourceType::Module,
        );
        assert!(provider.source().is_empty());
        assert!(provider.is_module_type());
    }

    #[test]
    fn buffer_guard_counts() {
        let provider = make("x");
        let dynamic: Rc<dyn SourceProvider> = provider.clone();
        {
            let _outer = SourceProviderBufferGuard::new(Some(dynamic.clone()));
            let _inner = SourceProviderBufferGuard::new(Some(dynamic.clone()));
            assert_eq!(provider.base().locking_count.get(), 2);
        }
        assert_eq!(provider.base().locking_count.get(), 0);
    }
}
