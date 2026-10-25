//! Porte de `src/vs/workbench/contrib/terminal/test/browser/xterm/xtermTestUtils.ts` (`TestWebglAddon` e
//! `TestXtermAddonImporter`) e de `src/vs/platform/theme/test/common/testThemeService.ts` (`TestColorTheme` e
//! `TestThemeService`, com o `IThemeService` do `je-editor`).
//!
//! Divergências:
//! - **`TestWebglAddon`.** O `WebglAddon` do xterm.js é o renderizador por GPU do porte ([`IXtermGpuRenderer`]); os campos
//!   estáticos (`shouldThrow`, `isEnabled` e `customGlyphOptions`) são `thread_local`, porque cada teste roda numa thread e o
//!   `static` do original vale por processo do mocha. Os eventos de atlas de textura (`onChangeTextureAtlas` e companhia) não têm
//!   leitor no porte e ficam de fora.
//! - **`TestXtermAddonImporter.importAddon('webgl')`.** É [`IGpuRendererImporter::import_gpu_renderer`], que devolve o construtor
//!   do [`TestWebglAddon`] numa promessa já resolvida.
//! - **`TestColorTheme`.** Guarda o mapa `{ [id]: string | undefined }`; `getColor` devolve `Color.fromHex(value)` para um valor
//!   não vazio e `undefined` para o resto, sem o padrão do registro (como o original).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_theme::color::Color;
use je_workbench::contrib::terminal::browser::xterm::xterm_terminal::{
    GpuRendererCtor, GpuRendererOptions, IGpuRendererImporter, IXtermGpuRenderer, IXtermRaw,
};

thread_local! {
    static SHOULD_THROW: Cell<bool> = const { Cell::new(false) };
    static IS_ENABLED: Cell<bool> = const { Cell::new(false) };
    static CUSTOM_GLYPH_OPTIONS: RefCell<Vec<Option<bool>>> = const { RefCell::new(Vec::new()) };
}

/// `TestWebglAddon`.
pub struct TestWebglAddon {
    on_context_loss: Rc<Emitter<()>>,
}

impl TestWebglAddon {
    /// `constructor(options)`: registra o `customGlyphs` recebido.
    pub fn new(options: GpuRendererOptions) -> Rc<Self> {
        CUSTOM_GLYPH_OPTIONS.with(|custom_glyph_options| custom_glyph_options.borrow_mut().push(options.custom_glyphs));
        Rc::new(Self { on_context_loss: Emitter::new() })
    }

    /// `TestWebglAddon.shouldThrow = value`.
    pub fn set_should_throw(value: bool) {
        SHOULD_THROW.with(|should_throw| should_throw.set(value));
    }

    /// `TestWebglAddon.isEnabled = value`.
    pub fn set_is_enabled(value: bool) {
        IS_ENABLED.with(|is_enabled| is_enabled.set(value));
    }

    /// `TestWebglAddon.isEnabled`.
    pub fn is_enabled() -> bool {
        IS_ENABLED.with(Cell::get)
    }

    /// `TestWebglAddon.customGlyphOptions.length = 0`.
    pub fn clear_custom_glyph_options() {
        CUSTOM_GLYPH_OPTIONS.with(|custom_glyph_options| custom_glyph_options.borrow_mut().clear());
    }

    /// `TestWebglAddon.customGlyphOptions`.
    pub fn custom_glyph_options() -> Vec<Option<bool>> {
        CUSTOM_GLYPH_OPTIONS.with(|custom_glyph_options| custom_glyph_options.borrow().clone())
    }
}

impl IXtermGpuRenderer for TestWebglAddon {
    fn activate(&self, _terminal: Rc<dyn IXtermRaw>) -> Result<(), Error> {
        let should_throw = SHOULD_THROW.with(Cell::get);
        IS_ENABLED.with(|is_enabled| is_enabled.set(!should_throw));
        if should_throw {
            return Err(Error::new("Test webgl set to throw"));
        }
        Ok(())
    }

    fn on_context_loss(&self) -> Event<()> {
        self.on_context_loss.event()
    }
}

impl IDisposable for TestWebglAddon {
    fn dispose(&self) {
        IS_ENABLED.with(|is_enabled| is_enabled.set(false));
        self.on_context_loss.dispose();
    }
}

/// `TestXtermAddonImporter`.
pub struct TestXtermAddonImporter;

impl IGpuRendererImporter for TestXtermAddonImporter {
    fn import_gpu_renderer(&self) -> Promise<GpuRendererCtor> {
        let ctor: GpuRendererCtor = Rc::new(|options: GpuRendererOptions| -> Rc<dyn IXtermGpuRenderer> { TestWebglAddon::new(options) });
        Promise::resolved(ctor)
    }
}

/// `TestColorTheme`.
pub struct TestColorTheme {
    colors: Vec<(String, Option<String>)>,
}

impl TestColorTheme {
    pub fn new(colors: Vec<(&str, Option<&str>)>) -> Rc<Self> {
        Rc::new(Self { colors: colors.into_iter().map(|(id, value)| (id.to_owned(), value.map(str::to_owned))).collect() })
    }
}

impl IColorTheme for TestColorTheme {
    fn get_color(&self, color: &str, _use_default: Option<bool>) -> Option<Color> {
        let value = self.colors.iter().find(|(id, _)| id == color).and_then(|(_, value)| value.as_deref())?;
        if value.is_empty() { None } else { Some(Color::from_hex(value)) }
    }
}

/// `TestThemeService`, só com o tema de cores.
pub struct TestThemeService {
    color_theme: RefCell<Rc<dyn IColorTheme>>,
    on_theme_change: Rc<Emitter<Rc<dyn IColorTheme>>>,
}

impl TestThemeService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { color_theme: RefCell::new(TestColorTheme::new(Vec::new())), on_theme_change: Emitter::new() })
    }

    /// `setTheme(theme)`.
    pub fn set_theme(&self, theme: Rc<dyn IColorTheme>) {
        *self.color_theme.borrow_mut() = theme.clone();
        self.on_theme_change.fire(theme);
    }
}

impl IThemeService for TestThemeService {
    fn get_color_theme(&self) -> Rc<dyn IColorTheme> {
        self.color_theme.borrow().clone()
    }

    fn on_did_color_theme_change(&self) -> Event<Rc<dyn IColorTheme>> {
        self.on_theme_change.event()
    }
}

impl IDisposable for TestThemeService {
    fn dispose(&self) {
        self.on_theme_change.dispose();
    }
}
