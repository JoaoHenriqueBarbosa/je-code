//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalWordLinkDetector.ts`: detecta, numa linha
//! (possivelmente quebrada) do buffer, as palavras separadas pelos `terminal.integrated.wordSeparators`, que viram links
//! de busca (ou links do protocolo de URL do produto).
//!
//! Decisões de tradução:
//! - `Terminal` do `@xterm/xterm` é o [`ITerminal`]; `IConfigurationService` e `IProductService` são parâmetros do
//!   [`TerminalWordLinkDetector::new`].
//! - O `detect` do original é síncrono; o contrato [`ITerminalLinkDetector`] devolve sempre um [`Promise`], então o
//!   resultado sai já resolvido.
//! - `_separatorRegex` (`new RegExp('[...]', 'g')`) é um [`zyarr::RegExp`] montado do mesmo jeito, e o `text.split(regex)` é
//!   uma varredura com o `exec` a partir do fim do casamento anterior (cada casamento da classe tem uma unidade).
//! - O texto da linha é UTF-16 ([`JsString`]) e os índices contam unidades UTF-16, como o `string` do original.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::network::{UriOrString, matches_scheme};
use je_base::promise::Promise;
use je_base::strings::escape_reg_exp_characters;
use je_base::uri::URI;
use je_editor::range::Range;
use je_platform::configuration::service::IConfigurationService;
use je_platform::product::IProductService;
use je_platform::terminal::terminal_settings::TerminalSettingId;
use zyarr::RegExp;

use super::links::{ITerminalLinkDetector, ITerminalSimpleLink, TerminalBuiltinLinkType};
use super::terminal_link_helpers::{convert_link_range_to_buffer, get_xterm_line_content};
use crate::contrib::terminal::common::terminal::TERMINAL_CONFIG_SECTION;
use crate::contrib::terminal::xterm_buffer::{IBufferLine, ITerminal};

/// `Constants.MaxLineLength`: the max line length to try extract word links from.
const MAX_LINE_LENGTH: usize = 2000;

/// `interface Word`.
struct Word {
    start_index: i32,
    end_index: i32,
    text: JsString,
}

/// `TerminalWordLinkDetector`.
pub struct TerminalWordLinkDetector {
    base: Rc<Disposable>,
    xterm: Rc<dyn ITerminal>,
    configuration_service: Rc<dyn IConfigurationService>,
    product_service: Rc<dyn IProductService>,
    separator_regex: RefCell<Option<RegExp>>,
}

impl TerminalWordLinkDetector {
    pub const ID: &'static str = "word";

    // Word links typically search the workspace so it makes sense that their maximum link length is
    // quite small.
    pub const MAX_LINK_LENGTH: i32 = 100;

    pub fn new(xterm: Rc<dyn ITerminal>, configuration_service: Rc<dyn IConfigurationService>, product_service: Rc<dyn IProductService>) -> Rc<Self> {
        let this = Rc::new(Self { base: Disposable::new(), xterm, configuration_service, product_service, separator_regex: RefCell::new(None) });

        this.refresh_separator_codes();
        let weak: Weak<Self> = Rc::downgrade(&this);
        let subscription = this.configuration_service.on_did_change_configuration().listen(move |event| {
            let Some(this) = weak.upgrade() else { return };
            if event.affects_configuration(TerminalSettingId::WordSeparators.as_str(), None) {
                this.refresh_separator_codes();
            }
        });
        this.base.register(subscription);
        this
    }

    fn parse_words(&self, text: &JsString) -> Vec<Word> {
        let regex = self.separator_regex.borrow();
        let regex = regex.as_ref().expect("os separadores são carregados no construtor");
        let units = text.units();
        let mut words = Vec::new();
        let mut running_index = 0usize;
        let mut position = 0usize;
        // `text.split(this._separatorRegex)`: cada casamento é um separador de uma unidade.
        loop {
            let separator = regex.exec(units, position);
            let word_end = separator.as_ref().map_or(units.len(), |found| found.start());
            let word = JsString::from(&units[position..word_end]);
            words.push(Word { start_index: running_index as i32, end_index: (running_index + word.len()) as i32, text: word.clone() });
            running_index += word.len() + 1;
            match separator {
                Some(found) => position = found.end(),
                None => break,
            }
        }
        words
    }

    fn refresh_separator_codes(&self) {
        let configuration = self.configuration_service.get_value(Some(TERMINAL_CONFIG_SECTION), None);
        let separators = match configuration.get_str("wordSeparators") {
            je_base::dynamic::Dynamic::String(separators) => separators,
            _ => JsString::new(),
        };
        let mut powerline_symbols = String::new();
        for code in 0xe0b0u32..=0xe0bf {
            powerline_symbols.push(char::from_u32(code).expect("a faixa powerline é BMP"));
        }
        let pattern = format!("[{}{}]", escape_reg_exp_characters(separators.units()).to_string_lossy(), powerline_symbols);
        *self.separator_regex.borrow_mut() = Some(RegExp::from_str(&pattern, "g").expect("a classe de separadores é válida"));
    }
}

impl IDisposable for TerminalWordLinkDetector {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ITerminalLinkDetector for TerminalWordLinkDetector {
    fn xterm(&self) -> Rc<dyn ITerminal> {
        self.xterm.clone()
    }

    fn max_link_length(&self) -> i32 {
        Self::MAX_LINK_LENGTH
    }

    fn detect(&self, lines: &[Rc<dyn IBufferLine>], start_line: i32, end_line: i32) -> Promise<Vec<ITerminalSimpleLink>> {
        let mut links: Vec<ITerminalSimpleLink> = Vec::new();

        // Get the text representation of the wrapped line
        let cols = self.xterm.cols();
        let buffer = self.xterm.buffer_active();
        let text = get_xterm_line_content(buffer.as_ref(), start_line, end_line, cols);
        if text.is_empty() || text.len() > MAX_LINE_LENGTH {
            return Promise::resolved(Vec::new());
        }

        // Parse out all words from the wrapped line
        let words = self.parse_words(&text);

        // Map the words to ITerminalLink objects
        for mut word in words {
            if word.text.is_empty() {
                continue;
            }
            if word.text.units().last() == Some(&(b':' as u16)) {
                word.text = word.text.slice(0, word.text.len() - 1);
                word.end_index -= 1;
            }
            let buffer_range = convert_link_range_to_buffer(
                lines,
                cols,
                &Range { start_column: word.start_index + 1, start_line_number: 1, end_column: word.end_index + 1, end_line_number: 1 },
                start_line,
            );

            // Support this product's URL protocol
            let word_text = word.text.to_string_lossy();
            if matches_scheme(UriOrString::Str(&word_text), self.product_service.url_protocol()) {
                let uri = URI::parse(word_text.as_str(), false);
                let mut link = ITerminalSimpleLink::new(word_text, buffer_range, TerminalBuiltinLinkType::Url);
                link.uri = Some(uri);
                links.push(link);
                continue;
            }

            // Search links
            let mut link = ITerminalSimpleLink::new(word_text, buffer_range, TerminalBuiltinLinkType::Search);
            link.context_line = Some(text.to_string_lossy());
            links.push(link);
        }

        Promise::resolved(links)
    }
}
