//! A view do editor: estado (rolagem, cursor, camadas de linha em cache) e pintura.
//!
//! Porte do que no Monaco mora em `viewParts` (`ViewLines`, `LineNumbersOverlay`, `CurrentLineHighlightOverlay`,
//! `SelectionsOverlay`, `IndentGuidesOverlay`, `ViewCursors`, `DecorationsOverlay` dos colchetes) e em
//! `viewModel`, sem DOM: cada parte vira um trecho da pintura, que escreve numa [`DisplayList`]. A ordem de
//! pintura é a das camadas do Monaco: fundo, realce da linha, seleção, colchetes casando, guias, texto, cursor,
//! e a margem (números de linha) por cima.
//!
//! Medida: o texto de cada linha é feito por trecho de token (um `<span class="mtkN">` do Monaco), e a largura
//! de cada trecho sai arredondada para cima em 1/64 px (a métrica do Chromium), de modo que as posições batem
//! com as do oráculo. O layout fica em cache por linha e é invalidado pelos eventos de conteúdo, de tokens e
//! de colchetes do modelo.

use je_editor::cursor_common::CursorSimpleModel;
use je_editor::view_model_types::IViewModel;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::runtime;
use je_base::scrollable::{INewScrollDimensions, INewScrollPosition, IScrollableOptions, Scrollable};
use je_editor::attached_views::AttachedView;
use je_editor::encoded_token_attributes::{TokenMetadata, font_style};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_render::kurbo::{Rect, RoundedRect, RoundedRectRadii};
use je_render::{DisplayList, Rgba};
use je_text::{TextLine, with_text_system};
use web_time::{Duration, Instant};

use crate::clipboard::Clipboard;
use crate::colors::EditorColors;
use crate::core::EditorCore;
use crate::shortcuts::Shortcuts;
use crate::layout::{DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT, FontMetrics, ViewLayout};
use crate::text;

/// O cursor pisca a cada 500 ms (`cursorBlinking: blink`).
pub const BLINK_PERIOD: Duration = Duration::from_millis(500);
/// O Monaco para de desenhar uma linha depois de 10 000 colunas (`stopRenderingLineAfter`).
const STOP_RENDERING_LINE_AFTER: usize = 10_000;
/// O raio dos cantos da seleção (`border-radius: 3px` de `.selected-text`).
const SELECTION_RADIUS: f64 = 3.0;
/// `scrollBeyondLastColumn`.
const SCROLL_BEYOND_LAST_COLUMN: f64 = 4.0;

/// Um trecho de uma linha com estilo único (um `span` de token).
pub(crate) struct Segment {
    /// O início em px lógicos, relativo ao início do texto da linha.
    pub x: f64,
    pub line: Rc<TextLine>,
    pub color: Rgba,
    pub start: usize,
    pub end: usize,
    pub blank: bool,
    pub underline: bool,
    pub strike: bool,
}

/// O layout em cache de uma linha da view (uma linha do modelo, ou um pedaço dela com a quebra de linha).
pub(crate) struct LineLayout {
    pub segments: Vec<Segment>,
    /// A largura do texto em px lógicos.
    pub width: f64,
    /// O texto da linha da view (numa continuação quebrada, com o recuo de `wrappingIndent` na frente).
    pub units: Vec<u16>,
    pub ascii: bool,
    /// A linha do modelo de onde a linha da view vem.
    pub model_line: i32,
    /// É a primeira linha da view da linha do modelo (onde aparece o número da linha).
    pub first: bool,
    /// A linha do modelo continua na linha da view seguinte (`continuesWithWrappedLine`).
    pub continues: bool,
    /// A menor coluna da linha da view (depois do recuo de uma continuação quebrada).
    pub min_column: i32,
}

/// O que os ouvintes do modelo deixam para a view tratar no próximo quadro.
#[derive(Default)]
pub(crate) struct Shared {
    pub invalid_all: Cell<bool>,
    pub invalid_lines: RefCell<Vec<(i32, i32)>>,
    pub dirty: Cell<bool>,
}

/// O tipo de uma mudança contra a versão do repositório.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GutterKind {
    Added,
    Modified,
    /// Linhas removidas: o triângulo fica na fronteira antes de `start_line`.
    Deleted,
}

/// Uma barra do diff na margem: de `start_line` a `end_line` (inclusive, 1-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GutterChange {
    pub start_line: i32,
    pub end_line: i32,
    pub kind: GutterKind,
}

/// O último casamento de colchetes: a posição e a versão do modelo para as quais valeu, e o resultado.
type BracketMatchCache = ((Position, i64), Option<(Range, Range)>);

/// O que um arrasto do mouse está estendendo.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DragMode {
    Chars,
    Words,
    Lines,
    /// Os mesmos três, estendendo o último cursor criado (arrasto com Alt).
    LastCursorChars,
    LastCursorWords,
    LastCursorLines,
    /// Arrastando o botão da barra de rolagem; guarda o deslocamento do ponteiro dentro do botão.
    Scrollbar(f64),
    /// Arrastando no minimapa.
    Minimap,
}

pub struct EditorView {
    pub(crate) model: Rc<TextModel>,
    pub(crate) metrics: FontMetrics,
    pub(crate) layout: ViewLayout,
    pub(crate) colors: EditorColors,
    /// A cor de cada id do mapa de cores de token (índice 0: sem cor).
    token_colors: Vec<Option<Rgba>>,
    pub(crate) scrollable: Rc<Scrollable>,
    attached: Rc<AttachedView>,
    /// O widget de edição: modelo, cursores, comandos e desfazer.
    pub(crate) core: EditorCore,
    pub(crate) shortcuts: Shortcuts,
    /// O que a última cópia levou (`ClipboardStoredMetadata`), para o colar saber se veio de seleção vazia.
    pub(crate) copied: Option<crate::input::CopiedText>,
    pub(crate) shared: Rc<Shared>,
    lines: HashMap<i32, Rc<LineLayout>>,
    layout_scale: f64,
    pub(crate) focused: bool,
    blink_epoch: Instant,
    pub(crate) drag: Option<DragMode>,
    pub(crate) clipboard: Clipboard,
    max_line_width: f64,
    last_visible: Option<(i32, i32)>,
    /// A contagem de linhas da view do último quadro: mudou, as linhas deslocaram e o cache inteiro cai.
    view_line_count: i32,
    /// `editor.wordWrap`.
    pub(crate) word_wrap: crate::folding::WordWrap,
    /// A dobra de código (regiões, linhas escondidas, chevrons da margem).
    pub(crate) folding: crate::folding::Folding,
    /// O ponteiro está sobre a margem (os chevrons de dobra aparecem com `showFoldingControls: mouseover`).
    pub(crate) hover_margin: bool,
    bracket_match: RefCell<Option<BracketMatchCache>>,
    /// As ocorrências da busca (o widget de busca do shell) e o índice da atual.
    /// As barras de diff da margem (o `quickDiff` do SCM): linhas e tipo.
    gutter_changes: Vec<GutterChange>,
    pub(crate) find_matches: Vec<Range>,
    find_current: Option<usize>,
    /// As configurações do usuário em vigor (as do `settings.json`, resolvidas para a linguagem do modelo).
    pub(crate) settings: crate::settings::EditorSettings,
    /// O `settings.json` de onde vieram e a linguagem para a qual foram resolvidas (trocar a linguagem reaplica).
    user_settings: Option<Rc<serde_json::Map<String, serde_json::Value>>>,
    settings_language: String,
    settings_applied: bool,
    /// Os comandos de atalho que não são do editor, para o shell (`take_workbench_command`).
    pub(crate) workbench_commands: std::collections::VecDeque<(String, je_base::dynamic::Dynamic)>,
    subscriptions: Vec<DisposableRef>,
}

fn parse_hex_color(text: &str) -> Option<Rgba> {
    let hex = text.strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 => Some(Rgba::opaque(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Rgba::new(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

/// O agendador de quadro do `Scrollable` (a rolagem suave fica desligada, mas o contrato pede um).
fn schedule_frame(f: Box<dyn FnOnce()>) -> DisposableRef {
    let id = runtime::set_timeout(f, 16);
    to_disposable(move || runtime::clear_timeout(id))
}

impl EditorView {
    /// Cria a view de um modelo com o tamanho lógico dado.
    pub fn new(model: Rc<TextModel>, width: f64, height: f64, colors: EditorColors) -> Self {
        let metrics = FontMetrics::measure(DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT);
        let layout = ViewLayout::new(metrics.clone(), model.get_options().tab_size, width, height, model.get_line_count());
        let scrollable = Scrollable::new(IScrollableOptions {
            force_integer_values: true,
            smooth_scroll_duration: 0.0,
            schedule_at_next_animation_frame: Rc::new(schedule_frame),
        });
        let attached = model.on_before_attached();
        let shared = Rc::new(Shared::default());
        shared.invalid_all.set(true);

        let mut subscriptions: Vec<DisposableRef> = Vec::new();
        let s = shared.clone();
        subscriptions.push(model.on_did_change_content_internal().listen(move |e| {
            let content = &e.content_changed_event;
            // Edição dentro de uma linha, sem quebra: só essa linha muda (as outras mantêm o número e o
            // layout). Qualquer outra coisa desloca as linhas e invalida tudo.
            let within_lines = !content.is_flush
                && !content.is_eol_change
                && content.changes.iter().all(|c| c.range.start_line_number == c.range.end_line_number && !c.text.units().contains(&10) && !c.text.units().contains(&13));
            if within_lines {
                s.invalid_lines.borrow_mut().extend(content.changes.iter().map(|c| (c.range.start_line_number, c.range.start_line_number)));
            } else {
                s.invalid_all.set(true);
            }
            s.dirty.set(true);
        }));
        let s = shared.clone();
        subscriptions.push(model.on_did_change_tokens().listen(move |e| {
            s.invalid_lines.borrow_mut().extend(e.ranges.iter().map(|r| (r.from_line_number, r.to_line_number)));
            s.dirty.set(true);
        }));
        let s = shared.clone();
        subscriptions.push(model.bracket_pairs().on_did_change().listen(move |_| {
            s.invalid_all.set(true);
            s.dirty.set(true);
        }));
        let s = shared.clone();
        subscriptions.push(model.on_did_change_language().listen(move |_| {
            s.invalid_all.set(true);
            s.dirty.set(true);
        }));

        let core = EditorCore::new(model.clone(), &metrics, width, height);
        let folding = crate::folding::Folding::new(&model, &shared);
        Self {
            model,
            metrics,
            layout,
            colors,
            token_colors: Vec::new(),
            scrollable,
            attached,
            core,
            shortcuts: Shortcuts::default(),
            copied: None,
            shared,
            lines: HashMap::new(),
            layout_scale: 1.0,
            focused: true,
            blink_epoch: Instant::now(),
            drag: None,
            clipboard: Clipboard::new(),
            max_line_width: 0.0,
            last_visible: None,
            view_line_count: 0,
            word_wrap: crate::folding::WordWrap::Off,
            folding,
            hover_margin: false,
            bracket_match: RefCell::new(None),
            gutter_changes: Vec::new(),
            find_matches: Vec::new(),
            find_current: None,
            settings: crate::settings::EditorSettings::default(),
            user_settings: None,
            settings_language: String::new(),
            settings_applied: false,
            workbench_commands: std::collections::VecDeque::new(),
            subscriptions,
        }
    }

    /// Aplica o `settings.json` do usuário (o objeto inteiro: as chaves gerais e os blocos de linguagem). O editor
    /// guarda o objeto e resolve de novo quando a linguagem do modelo muda.
    pub fn apply_configuration(&mut self, user: &serde_json::Map<String, serde_json::Value>) {
        let user = Rc::new(user.clone());
        let language = self.model.get_language_id();
        self.user_settings = Some(user.clone());
        self.settings_language = language.clone();
        self.apply_settings(crate::settings::EditorSettings::resolve(&user, &language));
    }

    /// As configurações em vigor.
    pub fn settings(&self) -> &crate::settings::EditorSettings {
        &self.settings
    }

    /// Aplica configurações já resolvidas: a fonte, a indentação do modelo, a quebra, a margem e o resto da pintura,
    /// e as opções do núcleo (`updateOptions` do widget). Só o que mudou é refeito.
    pub fn apply_settings(&mut self, settings: crate::settings::EditorSettings) {
        if settings == self.settings && self.settings_applied {
            return;
        }
        self.settings_applied = true;
        let previous = std::mem::replace(&mut self.settings, settings);
        let s = &self.settings;
        let metrics_changed = s.font_family != self.metrics.families || s.font_size != self.metrics.size || s.font_weight != self.metrics.weight || s.line_height != self.metrics.line_height;
        if metrics_changed {
            self.metrics = FontMetrics::measure_weighted(&s.font_family, s.font_size, s.font_weight, s.line_height);
            self.layout.metrics = self.metrics.clone();
            self.max_line_width = 0.0;
        }
        self.layout.show_line_numbers = s.line_numbers != crate::settings::LineNumbers::Off;
        self.layout.minimap_enabled = s.minimap;
        // As chaves que saíram do `settings.json` voltam ao padrão (`undefined` no `applyUpdate`).
        let removed: Vec<String> = previous.editor_keys().filter(|k| !self.settings.editor_keys().any(|n| n == *k)).cloned().collect();
        let options = self.settings.editor_options(&removed);
        let wrap = self.settings.word_wrap;
        self.core.update_options(&self.metrics, &options);
        self.word_wrap = wrap;
        if wrap != crate::folding::WordWrap::Off {
            let top = self.scroll_top();
            self.scroll_by(-self.scroll_left(), 0.0);
            self.set_scroll_top(top);
        }
        // A indentação do modelo (`ModelService._setModelOptionsForModel`).
        let s = &self.settings;
        let bracket = je_editor::model::BracketPairColorizationOptions { enabled: s.bracket_pair_colorization, independent_color_pool_per_bracket_type: false };
        if s.detect_indentation {
            self.model.detect_indentation(s.insert_spaces, s.tab_size);
            self.model.update_options(je_editor::model::TextModelUpdateOptions { bracket_colorization_options: Some(bracket), ..Default::default() });
        } else {
            let indent_size = Some(s.indent_size.map_or(je_editor::model::IndentSize::TabSize, je_editor::model::IndentSize::Size));
            self.model.update_options(je_editor::model::TextModelUpdateOptions { tab_size: Some(s.tab_size), insert_spaces: Some(s.insert_spaces), indent_size, bracket_colorization_options: Some(bracket), ..Default::default() });
        }
        self.layout.tab_size = self.model.get_options().tab_size;
        self.reset_blink();
        self.shared.invalid_all.set(true);
        self.reveal_cursor();
        self.mark_dirty();
    }

    /// A linguagem do modelo mudou depois da configuração: os blocos `[linguagem]` são outros.
    fn refresh_language_settings(&mut self) {
        let Some(user) = self.user_settings.clone() else { return };
        let language = self.model.get_language_id();
        if language != self.settings_language {
            self.settings_language = language.clone();
            self.apply_settings(crate::settings::EditorSettings::resolve(&user, &language));
        }
    }

    /// Define as ocorrências da busca a destacar (`FindDecorations`): todas com o fundo de realce e a atual por cima.
    pub fn set_find_matches(&mut self, matches: Vec<Range>, current: Option<usize>) {
        self.find_matches = matches;
        self.find_current = current;
        self.mark_dirty();
    }

    /// Define as barras de diff da margem (calculadas pelo shell contra o HEAD).
    pub fn set_gutter_changes(&mut self, changes: Vec<GutterChange>) {
        if self.gutter_changes != changes {
            self.gutter_changes = changes;
            self.mark_dirty();
        }
    }

    /// Troca a área de transferência (os testes usam só a cópia interna).
    pub fn set_clipboard(&mut self, clipboard: Clipboard) {
        self.clipboard = clipboard;
    }

    pub fn model(&self) -> &Rc<TextModel> {
        &self.model
    }

    pub fn layout(&self) -> &ViewLayout {
        &self.layout
    }

    pub fn colors(&self) -> &EditorColors {
        &self.colors
    }

    /// Troca as cores do editor (troca de tema).
    pub fn set_colors(&mut self, colors: EditorColors) {
        self.colors = colors;
        self.shared.invalid_all.set(true);
        self.shared.dirty.set(true);
    }

    /// O mapa de cores de token do tema (`TokenizationRegistry.getColorMap()`, `#RRGGBB` por id).
    pub fn set_token_colors(&mut self, color_map: &[String]) {
        self.token_colors = color_map.iter().map(|c| parse_hex_color(c)).collect();
        self.shared.invalid_all.set(true);
        self.shared.dirty.set(true);
    }

    pub fn resize(&mut self, width: f64, height: f64) {
        if (self.layout.width, self.layout.height) != (width, height) {
            self.layout.width = width;
            self.layout.height = height;
            self.core.resize(width, height);
            self.shared.dirty.set(true);
        }
    }

    /// Um quadro novo é preciso (conteúdo, tokens, cursor ou rolagem mudaram desde a última leitura).
    pub fn take_dirty(&self) -> bool {
        self.shared.dirty.replace(false)
    }

    pub fn mark_dirty(&self) {
        self.shared.dirty.set(true);
    }

    pub fn set_focus(&mut self, focused: bool) {
        if self.focused != focused {
            self.focused = focused;
            self.core.set_focus(focused);
            self.reset_blink();
            self.mark_dirty();
        }
    }

    pub fn reset_blink(&mut self) {
        self.blink_epoch = Instant::now();
    }

    /// O cursor está na fase visível do piscar em `now`.
    pub fn cursor_visible_at(&self, now: Instant) -> bool {
        if self.settings.cursor_blinking == crate::settings::CursorBlinking::Solid {
            return true;
        }
        let phase = now.saturating_duration_since(self.blink_epoch).as_millis() / BLINK_PERIOD.as_millis();
        phase.is_multiple_of(2)
    }

    /// Quando o cursor muda de fase pela próxima vez (para o loop de eventos dormir até lá).
    pub fn next_blink_deadline(&self, now: Instant) -> Instant {
        if self.settings.cursor_blinking == crate::settings::CursorBlinking::Solid {
            return now + Duration::from_secs(3600);
        }
        let elapsed = now.saturating_duration_since(self.blink_epoch).as_millis();
        let period = BLINK_PERIOD.as_millis();
        let next = (elapsed / period + 1) * period;
        self.blink_epoch + Duration::from_millis(next as u64)
    }

    // #region rolagem

    pub fn scroll_top(&self) -> f64 {
        self.scrollable.get_current_scroll_position().scroll_top
    }

    pub fn scroll_left(&self) -> f64 {
        self.scrollable.get_current_scroll_position().scroll_left
    }

    pub fn set_scroll_top(&mut self, top: f64) {
        self.sync_scroll_dimensions();
        self.scrollable.set_scroll_position_now(&INewScrollPosition { scroll_top: Some(top.clamp(0.0, self.layout.max_scroll_top())), scroll_left: None });
        self.mark_dirty();
    }

    pub fn scroll_by(&mut self, dx: f64, dy: f64) {
        self.sync_scroll_dimensions();
        let current = self.scrollable.get_current_scroll_position();
        self.scrollable.set_scroll_position_now(&INewScrollPosition {
            scroll_top: Some((current.scroll_top + dy).clamp(0.0, self.layout.max_scroll_top())),
            scroll_left: Some((current.scroll_left + dx).max(0.0)),
        });
        self.mark_dirty();
    }

    /// As dimensões de rolagem com as linhas da view atuais (fora do quadro: depois de dobrar ou de quebrar).
    pub(crate) fn sync_scroll_dimensions_now(&mut self) {
        self.folding.update(&self.core);
        self.sync_scroll_dimensions();
    }

    fn sync_scroll_dimensions(&mut self) {
        self.layout.line_count = self.core.view_model.get_line_count();
        self.layout.model_line_count = self.model.get_line_count();
        self.layout.tab_size = self.model.get_options().tab_size;
        let scroll_width = (self.max_line_width + SCROLL_BEYOND_LAST_COLUMN * self.metrics.char_width).max(self.layout.content_width());
        self.scrollable.set_scroll_dimensions(
            &INewScrollDimensions {
                width: Some(self.layout.content_width()),
                scroll_width: Some(scroll_width),
                height: Some(self.layout.height),
                scroll_height: Some(self.layout.scroll_height()),
            },
            false,
        );
    }

    // #endregion

    // #region geometria de posição

    pub(crate) fn line_layout(&mut self, line_number: i32) -> Rc<LineLayout> {
        if let Some(cached) = self.lines.get(&line_number) {
            return cached.clone();
        }
        let built = Rc::new(self.build_line(line_number));
        if built.width > self.max_line_width {
            self.max_line_width = built.width;
        }
        if self.lines.len() > 2000 {
            self.lines.clear();
        }
        self.lines.insert(line_number, built.clone());
        built
    }

    pub(crate) fn token_color(&self, metadata: u32) -> Rgba {
        let foreground = TokenMetadata::get_foreground(metadata) as usize;
        self.token_colors.get(foreground).copied().flatten().filter(|_| foreground != 0).unwrap_or(self.colors.foreground)
    }

    fn build_line(&self, line_number: i32) -> LineLayout {
        let scale = self.layout_scale;
        let data = self.core.view_model.get_view_line_data(line_number);
        let model_start = self.to_model(Position::new(line_number, data.min_column));
        let units = data.content.units().to_vec();
        let ascii = text::is_basic_ascii(&units);
        let tab = self.layout.tab_size;
        let limit = units.len().min(STOP_RENDERING_LINE_AFTER);

        // Pedaços coloridos: os tokens, partidos nas bordas dos colchetes colorizados.
        struct Piece {
            start: usize,
            end: usize,
            color: Rgba,
            bold: bool,
            italic: bool,
            underline: bool,
            strike: bool,
        }
        let tokens = data.tokens.clone();
        let mut brackets: Vec<(usize, usize, Rgba)> = Vec::new();
        if self.settings.bracket_pair_colorization && !self.colors.bracket_colors.is_empty() && !units.is_empty() {
            // Os colchetes da linha do modelo que caem nesta linha da view, em colunas da view.
            let model_line = model_start.line_number;
            let range = Range::new(model_line, 1, model_line, self.model.get_line_max_column(model_line));
            for b in self.model.bracket_pairs().get_brackets_in_range(range, true).to_array() {
                if b.range.start_line_number != model_line || b.range.end_line_number != model_line {
                    continue;
                }
                let Some(color) = self.colors.bracket_color(b.nesting_level, b.is_invalid) else { continue };
                let view = self.to_view_range(b.range);
                if view.start_line_number == line_number && view.end_line_number == line_number {
                    brackets.push(((view.start_column - 1) as usize, (view.end_column - 1) as usize, color));
                }
            }
            brackets.sort_by_key(|b| b.0);
        }
        let mut pieces: Vec<Piece> = Vec::new();
        let mut start = 0usize;
        for index in 0..tokens.get_count() {
            let end = (tokens.get_end_offset(index) as usize).min(units.len());
            let metadata = tokens.get_metadata(index);
            let style = TokenMetadata::get_font_style(metadata);
            let token_color = self.token_color(metadata);
            let mut cursor = start;
            for &(bs, be, bc) in brackets.iter().filter(|b| b.1 > start && b.0 < end) {
                let (bs, be) = (bs.max(start), be.min(end));
                if bs > cursor {
                    pieces.push(Piece { start: cursor, end: bs, color: token_color, bold: style & font_style::BOLD != 0, italic: style & font_style::ITALIC != 0, underline: style & font_style::UNDERLINE != 0, strike: style & font_style::STRIKETHROUGH != 0 });
                }
                pieces.push(Piece { start: bs, end: be, color: bc, bold: style & font_style::BOLD != 0, italic: style & font_style::ITALIC != 0, underline: style & font_style::UNDERLINE != 0, strike: style & font_style::STRIKETHROUGH != 0 });
                cursor = be;
            }
            if cursor < end {
                pieces.push(Piece { start: cursor, end, color: token_color, bold: style & font_style::BOLD != 0, italic: style & font_style::ITALIC != 0, underline: style & font_style::UNDERLINE != 0, strike: style & font_style::STRIKETHROUGH != 0 });
            }
            start = end;
        }

        let mut segments = Vec::with_capacity(pieces.len());
        let mut x = 0.0f64;
        let mut visual = 0i32;
        for piece in pieces {
            if piece.start >= limit {
                break;
            }
            let end = piece.end.min(limit);
            let slice = &units[piece.start..end];
            let (shown, next_visual) = text::expand_tabs(slice, visual, tab);
            visual = next_visual;
            let style = self.metrics.style(scale, piece.bold, piece.italic);
            let line = with_text_system(|t| t.line(&shown, &style));
            let width = f64::from(line.width) / scale;
            segments.push(Segment {
                x,
                line,
                color: piece.color,
                start: piece.start,
                end,
                blank: shown.chars().all(|c| c == ' '),
                underline: piece.underline,
                strike: piece.strike,
            });
            x += width;
        }
        LineLayout { segments, width: x, units, ascii, model_line: model_start.line_number, first: model_start.column == 1, continues: data.continues_with_wrapped_line, min_column: data.min_column }
    }

    /// A posição do modelo como posição da view (linha da view com quebra e dobra).
    pub(crate) fn to_view(&self, position: Position) -> Position {
        self.core.view_model.coordinates_converter.convert_model_position_to_view_position(position, None, None, None)
    }

    pub(crate) fn to_view_range(&self, range: Range) -> Range {
        self.core.view_model.coordinates_converter.convert_model_range_to_view_range(range, None)
    }

    pub(crate) fn to_model(&self, position: Position) -> Position {
        self.core.view_model.coordinates_converter.convert_view_position_to_model_position(position)
    }

    /// As seleções dos cursores em coordenadas da view.
    pub(crate) fn view_selections(&self) -> Vec<je_editor::selection::Selection> {
        self.core.view_model.get_cursor_states().into_iter().map(|c| c.view_state.selection).collect()
    }

    /// A posição horizontal (px lógicos, relativa ao início do texto) de uma coluna da linha.
    pub(crate) fn column_x(&mut self, line_number: i32, column: i32) -> f64 {
        let layout = self.line_layout(line_number);
        self.column_x_in(&layout, column)
    }

    pub(crate) fn column_x_in(&self, layout: &LineLayout, column: i32) -> f64 {
        if layout.ascii {
            return self.layout.x_of_visual_column(self.layout.visual_column(&layout.units, column));
        }
        let offset = ((column - 1).max(0) as usize).min(layout.units.len());
        for seg in &layout.segments {
            if offset <= seg.end {
                let before = &layout.units[seg.start..offset.max(seg.start)];
                let visual = self.layout.visual_column(&layout.units[..seg.start], seg.start as i32 + 1);
                let (shown, _) = text::expand_tabs(before, visual, self.layout.tab_size);
                let style = self.metrics.style(1.0, false, false);
                return seg.x + f64::from(with_text_system(|t| t.line(&shown, &style)).width);
            }
        }
        layout.width
    }

    /// A coluna do modelo mais próxima de um `x` relativo ao início do texto, na linha.
    pub(crate) fn column_at_x(&mut self, line_number: i32, x: f64) -> i32 {
        let layout = self.line_layout(line_number);
        let max_column = layout.units.len() as i32 + 1;
        if layout.ascii {
            let visual = self.layout.visual_column_at_x(x);
            return je_editor::cursor_columns::column_from_visible_column(&layout.units, visual, self.layout.tab_size).min(max_column);
        }
        // Linha com caractere largo ou par substituto: a borda de caractere mais próxima, pela largura medida.
        let mut best = (1, f64::MAX);
        let limit = (max_column as usize).min(STOP_RENDERING_LINE_AFTER);
        for column in 1..=limit as i32 {
            let unit_before = column > 1 && (0xDC00..0xE000).contains(&layout.units[(column - 2) as usize]);
            if unit_before {
                continue;
            }
            let distance = (self.column_x_in(&layout, column) - x).abs();
            if distance < best.1 {
                best = (column, distance);
            }
        }
        best.0
    }

    /// A posição do modelo sob um ponto da view (px lógicos relativos ao canto da view).
    pub fn position_at(&mut self, x: f64, y: f64) -> Position {
        let view = self.view_position_at(x, y);
        self.to_model(view)
    }

    /// A posição da view sob um ponto (px lógicos relativos ao canto da view).
    pub(crate) fn view_position_at(&mut self, x: f64, y: f64) -> Position {
        self.sync_scroll_dimensions();
        let line = self.layout.line_at_y(y, self.scroll_top());
        let local_x = x - self.layout.content_left() + self.scroll_left();
        let column = self.column_at_x(line, local_x);
        // A coluna não passa do recuo de uma continuação quebrada (`minColumn`).
        let min = self.core.view_model.get_line_min_column(line);
        Position::new(line, column.max(min))
    }

    /// O retângulo do cursor em px lógicos relativos à view (para testes e para o reveal).
    pub fn cursor_rect(&mut self) -> Rect {
        let p = self.core.view_model.get_primary_cursor_state().view_state.position;
        let x = self.layout.content_left() + self.column_x(p.line_number, p.column) - self.scroll_left();
        let y = self.layout.line_top(p.line_number, self.scroll_top());
        Rect::new(x, y, x + 2.0, y + self.layout.line_height())
    }

    // #endregion

    fn bracket_match(&self) -> Option<(Range, Range)> {
        let selections = self.core.selections();
        let primary = self.core.primary();
        if selections.len() != 1 || !primary.is_empty() {
            return None;
        }
        // Em arquivo grande o porte só tem a busca antiga de colchetes (linha a linha, sem árvore), que custa
        // vários ms por quadro: lá só se procura quando há um colchete comum ao lado do cursor.
        if self.model.is_too_large_for_tokenization() && !self.bracket_adjacent() {
            return None;
        }
        let key = (primary.get_position(), self.model.get_version_id());
        let mut cache = self.bracket_match.borrow_mut();
        if let Some((k, v)) = cache.as_ref()
            && *k == key
        {
            return *v;
        }
        // O orçamento da busca (ms): em arquivo grande, curto, para a digitação não perder quadro.
        let budget = if self.model.is_too_large_for_tokenization() { 1 } else { 20 };
        let found = self.model.bracket_pairs().match_bracket(key.0, Some(budget));
        *cache = Some((key, found));
        found
    }

    fn bracket_adjacent(&self) -> bool {
        let p = self.core.primary().get_position();
        let line = self.model.get_line_content(p.line_number);
        let at = |i: i32| usize::try_from(i).ok().and_then(|i| line.units().get(i)).copied();
        [at(p.column - 2), at(p.column - 1)].into_iter().flatten().any(|u| matches!(u, 0x28 | 0x29 | 0x5B | 0x5D | 0x7B | 0x7D))
    }

    /// Prepara um quadro: aplica as invalidações, ajusta as dimensões de rolagem e liga as linhas visíveis
    /// ao tokenizador.
    fn prepare(&mut self, scale: f64) {
        self.refresh_language_settings();
        if (scale - self.layout_scale).abs() > f64::EPSILON {
            self.layout_scale = scale;
            self.shared.invalid_all.set(true);
        }
        self.folding.update(&self.core);
        let view_line_count = self.core.view_model.get_line_count();
        if view_line_count != self.view_line_count {
            // Linhas da view entraram ou saíram (quebra, dobra, edição): os números deslocaram.
            self.view_line_count = view_line_count;
            self.shared.invalid_all.set(true);
        }
        if self.shared.invalid_all.replace(false) {
            self.lines.clear();
            self.shared.invalid_lines.borrow_mut().clear();
        } else {
            let invalid: Vec<(i32, i32)> = self.shared.invalid_lines.borrow_mut().drain(..).collect();
            let model_lines = self.model.get_line_count();
            for (from, to) in invalid {
                let (from, to) = (from.clamp(1, model_lines), to.clamp(1, model_lines));
                let start = self.to_view(Position::new(from, 1)).line_number;
                let end = self.to_view(Position::new(to, self.model.get_line_max_column(to))).line_number;
                for line in start..=end {
                    self.lines.remove(&line);
                }
            }
        }
        self.sync_scroll_dimensions();
        // O scroll_top pode ter ficado acima do máximo (arquivo encolheu): o Scrollable valida.
        let (first, last) = self.layout.visible_lines(self.scroll_top());
        if self.last_visible != Some((first, last)) {
            self.last_visible = Some((first, last));
            let model_first = self.to_model(Position::new(first, 1)).line_number;
            let model_last = self.to_model(Position::new(last, self.core.view_model.get_line_max_column(last))).line_number;
            self.attached.set_visible_lines(&[(model_first, model_last)], true);
        }
    }

    /// Pinta o editor em `list`. `origin` é o canto superior esquerdo da view na janela (px lógicos) e
    /// `scale` o fator de escala da tela.
    pub fn paint(&mut self, list: &mut DisplayList, origin: (f64, f64), scale: f64, now: Instant) {
        self.prepare(scale);
        let scroll_top = self.scroll_top();
        let scroll_left = self.scroll_left();
        let (first, last) = self.layout.visible_lines(scroll_top);
        let lh = self.layout.line_height();
        let content_left = self.layout.content_left();
        let view_width = self.layout.width;
        let text_right = self.layout.text_right();
        let view_height = self.layout.height;
        let snap = |v: f64| v.round();
        // Converte px lógicos da view em px de dispositivo, com as bordas no pixel (o Chromium alinha as caixas).
        let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
            Rect::new(snap((origin.0 + x0) * scale), snap((origin.1 + y0) * scale), snap((origin.0 + x1) * scale), snap((origin.1 + y1) * scale))
        };

        let colors = self.colors.clone();
        // Seleções, cursores e decorações em coordenadas da view (com quebra de linha e dobra).
        let selections = self.view_selections();
        let primary = self.core.view_model.get_primary_cursor_state().view_state.selection;
        let cursor_line = primary.position_line_number;
        let primary_model_line = self.core.primary().position_line_number;
        let all_empty = selections.iter().all(|s| s.is_empty());

        let laid_out: Vec<(i32, Rc<LineLayout>)> = (first..=last).map(|l| (l, self.line_layout(l))).collect();
        self.sync_scroll_dimensions();

        // Fundo da view inteira.
        list.fill_rect(rect(0.0, 0.0, view_width, view_height), colors.background);
        list.push_clip(je_render::Shape::Rect(rect(0.0, 0.0, view_width, view_height)));

        // Área de conteúdo, recortada (a rolagem horizontal não pode invadir a margem).
        list.push_clip(je_render::Shape::Rect(rect(content_left, 0.0, text_right, view_height)));

        // 1. Realce da linha de cada cursor (só com todas as seleções vazias, como o `CurrentLineHighlightOverlay`), no
        // conteúdo com `renderLineHighlight` `line` ou `all` e na margem com `gutter` ou `all`.
        let highlight = self.settings.render_line_highlight;
        let mut highlighted_lines: Vec<i32> = Vec::new();
        if all_empty && highlight != crate::settings::RenderLineHighlight::None {
            // Com a quebra de linha todas as linhas da view da linha do modelo do cursor recebem o realce.
            let mut lines: Vec<i32> = Vec::new();
            for selection in &selections {
                let model_line = self.to_model(Position::new(selection.position_line_number, 1)).line_number;
                let from = self.to_view(Position::new(model_line, 1)).line_number;
                let to = self.to_view(Position::new(model_line, self.model.get_line_max_column(model_line))).line_number;
                lines.extend((from.max(first)..=to.min(last)).filter(|l| (first..=last).contains(l)));
            }
            lines.sort_unstable();
            lines.dedup();
            highlighted_lines.clone_from(&lines);
            for line in lines.into_iter().filter(|_| highlight.content()) {
                let top = self.layout.line_top(line, scroll_top);
                if let Some(bg) = colors.line_highlight {
                    list.fill_rect(rect(content_left, top, text_right, top + lh), bg);
                }
                if let Some(border) = colors.line_highlight_border {
                    list.stroke(je_render::Shape::Rect(rect(content_left + 1.0, top + 1.0, text_right - 1.0, top + lh - 1.0)), 2.0 * scale, border);
                }
            }
        }

        // 2. Seleção: um retângulo por linha, com os cantos externos arredondados (`roundedSelection`), e as
        // colunas selecionadas guardadas para desenhar os espaços e tabulações (`renderWhitespace: selection`).
        let mut selected: Vec<(i32, i32, i32)> = Vec::new();
        for selection in selections.iter().filter(|s| !s.is_empty()) {
            let color = if self.focused { colors.selection } else { colors.selection_inactive };
            let mut spans: Vec<(i32, f64, f64)> = Vec::new();
            for (line_number, layout) in &laid_out {
                let l = *line_number;
                if l < selection.start_line_number || l > selection.end_line_number {
                    continue;
                }
                let start_col = if l == selection.start_line_number { selection.start_column } else { layout.min_column };
                let end_col = if l == selection.end_line_number { selection.end_column } else { layout.units.len() as i32 + 1 };
                selected.push((l, start_col, end_col));
                let x0 = self.column_x_in(layout, start_col);
                let mut x1 = self.column_x_in(layout, end_col);
                if l < selection.end_line_number && !layout.continues {
                    // A quebra de linha selecionada aparece como uma célula a mais.
                    x1 += self.metrics.char_width;
                }
                if x1 > x0 {
                    spans.push((l, content_left + x0 - scroll_left, content_left + x1 - scroll_left));
                }
            }
            for (i, &(l, x0, x1)) in spans.iter().enumerate() {
                let above = i.checked_sub(1).map(|j| spans[j]).filter(|p| p.0 == l - 1);
                let below = spans.get(i + 1).copied().filter(|p| p.0 == l + 1);
                let radius = SELECTION_RADIUS * scale;
                let corner = |neighbor: Option<(i32, f64, f64)>, outside: &dyn Fn(&(i32, f64, f64)) -> bool| if neighbor.as_ref().is_none_or(outside) { radius } else { 0.0 };
                let top_left = corner(above, &|p| p.1 > x0 + 0.5);
                let top_right = corner(above, &|p| p.2 < x1 - 0.5);
                let bottom_right = corner(below, &|p| p.2 < x1 - 0.5);
                let bottom_left = corner(below, &|p| p.1 > x0 + 0.5);
                let top = self.layout.line_top(l, scroll_top);
                let r = rect(x0, top, x1, top + lh);
                list.fill(je_render::Shape::RoundedRect(RoundedRect::from_rect(r, RoundedRectRadii::new(top_left, top_right, bottom_right, bottom_left))), color);
            }
        }

        // 2b. Ocorrências da busca (`.findMatch` e `.currentFindMatch`), recortadas às linhas visíveis.
        let find_matches: Vec<Range> = self.find_matches.iter().map(|m| self.to_view_range(*m)).collect();
        for (i, m) in find_matches.iter().enumerate() {
            if m.end_line_number < first || m.start_line_number > last {
                continue;
            }
            let color = if self.find_current == Some(i) { colors.find_match } else { colors.find_match_highlight };
            for l in m.start_line_number.max(first)..=m.end_line_number.min(last) {
                let layout = &laid_out[(l - first) as usize].1;
                let start_col = if l == m.start_line_number { m.start_column } else { layout.min_column };
                let end_col = if l == m.end_line_number { m.end_column } else { layout.units.len() as i32 + 1 };
                let top = self.layout.line_top(l, scroll_top);
                let x0 = content_left + self.column_x_in(layout, start_col) - scroll_left;
                let x1 = content_left + self.column_x_in(layout, end_col) - scroll_left;
                if x1 > x0 {
                    list.fill_rect(rect(x0, top, x1, top + lh), color);
                }
            }
        }

        // 3. Colchetes casando.
        if let Some((a, b)) = self.bracket_match() {
            for r in [self.to_view_range(a), self.to_view_range(b)] {
                if !(first..=last).contains(&r.start_line_number) || r.start_line_number != r.end_line_number {
                    continue;
                }
                let layout = &laid_out[(r.start_line_number - first) as usize].1;
                let x0 = content_left + self.column_x_in(layout, r.start_column) - scroll_left;
                let x1 = content_left + self.column_x_in(layout, r.end_column) - scroll_left;
                let top = self.layout.line_top(r.start_line_number, scroll_top);
                let outer = rect(x0, top, x1, top + lh);
                list.fill_rect(outer, colors.bracket_match_background);
                list.stroke(je_render::Shape::Rect(outer.inset(-0.5 * scale)), scale, colors.bracket_match_border);
            }
        }

        // 4. Guias de indentação (`editor.guides.indentation`).
        if self.settings.indent_guides && !self.model.is_too_large_for_tokenization() {
            let levels = self.core.view_model.get_lines_indent_guides(first, last);
            let active = self.core.view_model.get_active_indent_guide(cursor_line, first, last);
            let indent_size = self.model.get_options().indent_size.max(1);
            let width = f64::from(indent_size) * self.metrics.char_width;
            for (i, &level) in levels.iter().enumerate() {
                let l = first + i as i32;
                let top = self.layout.line_top(l, scroll_top);
                for lvl in 1..=level {
                    let is_active = active.start_line_number <= l && l <= active.end_line_number && lvl == active.indent;
                    let x = content_left + f64::from(lvl - 1) * width - scroll_left;
                    let color = if is_active { colors.indent_guide_active } else { colors.indent_guide };
                    // Uma linha de 1 px lógico (o `box-shadow: 1px 0 0 0 inset` da guia).
                    let r = rect(x, top, x, top + lh);
                    list.fill_rect(Rect::new(r.x0, r.y0, r.x0 + scale.round().max(1.0), r.y1), color);
                }
            }
        }

        // 4a. As réguas (`editor.rulers`): 1 px na coluna vezes a largura do caractere típico, a altura inteira.
        for ruler in &self.settings.rulers {
            let x = content_left + ruler.column * self.metrics.char_width - scroll_left;
            let color = ruler.color.as_deref().and_then(parse_css_color).unwrap_or(colors.ruler);
            let r = rect(x, 0.0, x, view_height);
            list.fill_rect(Rect::new(r.x0, r.y0, r.x0 + scale.round().max(1.0), r.y1), color);
        }

        // 4b. O fundo da primeira linha das regiões dobradas (`folded-background`).
        self.paint_fold_backgrounds(list, &rect, &laid_out, scroll_top);

        // 5. Texto.
        for (line_number, layout) in &laid_out {
            let top = self.layout.line_top(*line_number, scroll_top);
            for seg in &layout.segments {
                let x_dev = snap((origin.0 + content_left + seg.x - scroll_left) * scale) as f32;
                // A linha de base cai no pixel (o Skia do Chromium não posiciona glifo em fração de pixel na vertical).
                let top_dev = (origin.1 + top) * scale;
                let baseline = text_baseline(top_dev, lh * scale, &seg.line);
                let text_top = (baseline - f64::from(seg.line.baseline)) as f32;
                if !seg.blank {
                    seg.line.paint(list, x_dev, text_top, seg.color);
                }
                let width_dev = f64::from(seg.line.width);
                if seg.underline {
                    list.fill_rect(Rect::new(f64::from(x_dev), baseline + scale, f64::from(x_dev) + width_dev, baseline + 2.0 * scale), seg.color);
                }
                if seg.strike {
                    list.fill_rect(Rect::new(f64::from(x_dev), baseline - 4.0 * scale, f64::from(x_dev) + width_dev, baseline - 3.0 * scale), seg.color);
                }
            }
        }

        // 5b. Espaços e tabulações (`editor.renderWhitespace`): `·` e `→` na cor de `editorWhitespace.foreground`.
        let mode = self.settings.render_whitespace;
        if mode != crate::settings::RenderWhitespace::None {
            let style = self.metrics.style(scale, false, false);
            for (l, layout) in &laid_out {
                let l = *l;
                let in_selection: Vec<(i32, i32)> = selected.iter().filter(|s| s.0 == l).map(|s| (s.1, s.2)).collect();
                if mode == crate::settings::RenderWhitespace::Selection && in_selection.is_empty() {
                    continue;
                }
                let top = self.layout.line_top(l, scroll_top);
                for column in whitespace_columns(&layout.units, layout.min_column, mode, &in_selection) {
                    let unit = layout.units[(column - 1) as usize];
                    let mark = if unit == 0x20 { "\u{00B7}" } else { "\u{2192}" };
                    let line = with_text_system(|t| t.line(mark, &style));
                    let x0 = content_left + self.column_x_in(layout, column) - scroll_left;
                    let cell = (self.column_x_in(layout, column + 1) - self.column_x_in(layout, column)) * scale;
                    let x_dev = snap((origin.0 + x0) * scale + if unit == 0x20 { (cell - f64::from(line.width)) / 2.0 } else { 0.0 }) as f32;
                    let baseline = text_baseline((origin.1 + top) * scale, lh * scale, &line);
                    line.paint(list, x_dev, (baseline - f64::from(line.baseline)) as f32, colors.whitespace);
                }
            }
        }

        // 5c. A reticência depois da linha dobrada (`inline-folded`).
        self.paint_fold_ellipsis(list, origin, scale, &laid_out, scroll_top, scroll_left);

        // 6. Cursores (um por seleção), na forma de `editor.cursorStyle` (`ViewCursor`).
        if self.focused && self.cursor_visible_at(now) {
            use crate::settings::CursorStyle;
            let thin = scale.round().max(1.0);
            for selection in &selections {
                let p = selection.get_position();
                let p = Position::new(p.line_number, p.column);
                if !(first..=last).contains(&p.line_number) {
                    continue;
                }
                let layout = &laid_out[(p.line_number - first) as usize].1;
                let x = content_left + self.column_x_in(layout, p.column) - scroll_left;
                let top = self.layout.line_top(p.line_number, scroll_top);
                // A largura do caractere sob o cursor (no fim da linha, a de um caractere típico).
                let index = (p.column - 1) as usize;
                let under = layout.units.get(index).copied();
                let char_width = if under.is_some() { self.column_x_in(layout, p.column + 1) - self.column_x_in(layout, p.column) } else { self.metrics.char_width };
                let r = rect(x, top, x + char_width, top + lh);
                match self.settings.cursor_style {
                    CursorStyle::Line => list.fill_rect(Rect::new(r.x0, r.y0, r.x0 + 2.0 * scale, r.y1), colors.cursor),
                    CursorStyle::LineThin => list.fill_rect(Rect::new(r.x0, r.y0, r.x0 + thin, r.y1), colors.cursor),
                    CursorStyle::Underline => list.fill_rect(Rect::new(r.x0, r.y1 - 2.0 * scale, r.x1, r.y1), colors.cursor),
                    CursorStyle::UnderlineThin => list.fill_rect(Rect::new(r.x0, r.y1 - thin, r.x1, r.y1), colors.cursor),
                    CursorStyle::BlockOutline => list.stroke(je_render::Shape::Rect(r.inset(-0.5 * thin)), thin, colors.cursor),
                    CursorStyle::Block => {
                        list.fill_rect(r, colors.cursor);
                        // O caractere sob o bloco sai na cor de `editorCursor.background` (o fundo do editor).
                        if let Some(unit) = under.filter(|u| *u != 0x20 && *u != 0x09) {
                            let text = String::from_utf16_lossy(&[unit]);
                            let line = with_text_system(|t| t.line(&text, &self.metrics.style(scale, false, false)));
                            let baseline = text_baseline((origin.1 + top) * scale, lh * scale, &line);
                            line.paint(list, r.x0 as f32, (baseline - f64::from(line.baseline)) as f32, colors.cursor_text);
                        }
                    }
                }
            }
        }
        list.pop_clip();

        // 7. Margem: números de linha.
        let ln_left = self.layout.line_numbers_left();
        let ln_width = self.layout.line_numbers_width();
        list.fill_rect(rect(0.0, 0.0, content_left, view_height), colors.gutter_background);
        // O realce da linha do cursor na margem (`CurrentLineMarginHighlightOverlay`, de 0 até o `contentLeft`).
        if highlight.gutter() {
            for &line in &highlighted_lines {
                let top = self.layout.line_top(line, scroll_top);
                if let Some(bg) = colors.line_highlight {
                    list.fill_rect(rect(0.0, top, content_left, top + lh), bg);
                }
                if let Some(border) = colors.line_highlight_border {
                    list.stroke(je_render::Shape::Rect(rect(1.0, top + 1.0, content_left - 1.0, top + lh - 1.0)), 2.0 * scale, border);
                }
            }
        }
        let style = self.metrics.style(scale, false, false);
        let numbers = self.settings.line_numbers;
        let last_model_line = self.model.get_line_count();
        for (l, layout) in &laid_out {
            let l = *l;
            // Só a primeira linha da view de cada linha do modelo leva o número.
            if !layout.first {
                continue;
            }
            // `_getLineRenderLineNumber`: o texto conforme `editor.lineNumbers`.
            let current = layout.model_line == primary_model_line;
            let label = match numbers {
                crate::settings::LineNumbers::Off => continue,
                crate::settings::LineNumbers::On => layout.model_line.to_string(),
                crate::settings::LineNumbers::Relative if current => layout.model_line.to_string(),
                crate::settings::LineNumbers::Relative => (layout.model_line - primary_model_line).abs().to_string(),
                crate::settings::LineNumbers::Interval if current || layout.model_line % 10 == 0 || layout.model_line == last_model_line => layout.model_line.to_string(),
                crate::settings::LineNumbers::Interval => continue,
            };
            let line = with_text_system(|t| t.line(&label, &style));
            let top = self.layout.line_top(l, scroll_top);
            let color = if current { colors.line_number_active } else { colors.line_number };
            // A linha do cursor no modo relativo fica alinhada à esquerda (`.relative-current-line-number`).
            let left_aligned = current && numbers == crate::settings::LineNumbers::Relative;
            let x_dev = if left_aligned { snap((origin.0 + ln_left) * scale) as f32 } else { snap((origin.0 + ln_left + ln_width) * scale - f64::from(line.width)) as f32 };
            let top_dev = (origin.1 + top) * scale;
            let baseline = text_baseline(top_dev, lh * scale, &line);
            line.paint(list, x_dev, (baseline - f64::from(line.baseline)) as f32, color);
        }
        // As barras do diff contra o HEAD: 3 px de largura, 5 px à direita da margem dos números.
        let bar_left = ln_left + ln_width + 5.0;
        let model_lines = self.model.get_line_count();
        let gutter_changes: Vec<GutterChange> = self
            .gutter_changes
            .iter()
            .map(|c| {
                let start = c.start_line.clamp(1, model_lines);
                let end = c.end_line.clamp(1, model_lines);
                let view_start = self.to_view(Position::new(start, 1)).line_number;
                let view_end = self.to_view(Position::new(end, self.model.get_line_max_column(end))).line_number;
                // Linhas removidas no fim do arquivo: a fronteira fica depois da última linha.
                let view_start = if c.kind == GutterKind::Deleted && c.start_line > model_lines { self.core.view_model.get_line_count() + 1 } else { view_start };
                GutterChange { start_line: view_start, end_line: view_end, kind: c.kind }
            })
            .collect();
        for change in &gutter_changes {
            let (color, deleted) = match change.kind {
                GutterKind::Added => (colors.gutter_added, false),
                GutterKind::Modified => (colors.gutter_modified, false),
                GutterKind::Deleted => (colors.gutter_deleted, true),
            };
            if deleted {
                // Um triângulo na fronteira entre as linhas, apontando para a direita.
                let boundary = self.layout.line_top(change.start_line, scroll_top);
                if boundary < -lh || boundary > view_height + lh {
                    continue;
                }
                for step in 0..5 {
                    let y = boundary - 2.5 + f64::from(step);
                    let span = if step < 3 { f64::from(step) + 1.0 } else { 5.0 - f64::from(step) };
                    list.fill_rect(rect(bar_left, y, bar_left + span, y + 1.0), color);
                }
                continue;
            }
            let (from, to) = (change.start_line.max(first), change.end_line.min(last));
            if from > to {
                continue;
            }
            let (top, bottom) = (self.layout.line_top(from, scroll_top), self.layout.line_top(to, scroll_top) + lh);
            let r = rect(bar_left, top, bar_left + 3.0, bottom);
            if change.kind == GutterKind::Modified {
                // O azul tracejado das linhas modificadas: pixels alternados.
                let step = scale.round().max(1.0);
                let mut y = r.y0;
                let mut row = 0;
                while y < r.y1 {
                    let mut x = r.x0 + if row % 2 == 0 { 0.0 } else { step };
                    while x < r.x1 {
                        list.fill_rect(Rect::new(x, y, (x + step).min(r.x1), (y + step).min(r.y1)), color);
                        x += 2.0 * step;
                    }
                    y += step;
                    row += 1;
                }
            } else {
                list.fill_rect(r, color);
            }
        }
        self.paint_fold_chevrons(list, origin, scale, &laid_out, scroll_top);
        self.paint_minimap(list, origin, scale);
        self.paint_scrollbar(list, origin, scale);
        self.paint_overview_ruler(list, origin, scale);
        list.pop_clip();
    }
}

/// As colunas (1) dos espaços e tabulações a desenhar numa linha da view (`_applyRenderWhitespace`): `units` é o
/// texto da linha da view, `min_column` a primeira coluna depois do recuo de uma continuação quebrada e `selected`
/// as faixas `[início, fim)` selecionadas na linha (o modo `selection`).
fn whitespace_columns(units: &[u16], min_column: i32, mode: crate::settings::RenderWhitespace, selected: &[(i32, i32)]) -> Vec<i32> {
    use crate::settings::RenderWhitespace as Mode;
    let start = (min_column - 1).max(0) as usize;
    let is_ws = |u: u16| u == 0x20 || u == 0x09;
    let first_non_ws = units.iter().skip(start).position(|u| !is_ws(*u)).map(|i| i + start);
    let last_non_ws = units.iter().rposition(|u| !is_ws(*u)).filter(|i| *i >= start);
    let empty = first_non_ws.is_none();
    let (first_non_ws, last_non_ws) = (first_non_ws.unwrap_or(units.len()), last_non_ws.unwrap_or(units.len()));
    let mut result = Vec::new();
    let mut was_in_whitespace = false;
    for (index, &unit) in units.iter().enumerate().skip(start) {
        let mut in_whitespace = if index < first_non_ws || index > last_non_ws {
            true
        } else if unit == 0x09 {
            true
        } else if unit == 0x20 {
            if mode == Mode::Boundary {
                was_in_whitespace || units.get(index + 1).is_some_and(|n| is_ws(*n))
            } else {
                true
            }
        } else {
            false
        };
        if in_whitespace && mode == Mode::Selection {
            let column = index as i32 + 1;
            in_whitespace = selected.iter().any(|&(from, to)| from <= column && column < to);
        }
        if in_whitespace && mode == Mode::Trailing {
            in_whitespace = empty || index > last_non_ws;
        }
        if in_whitespace && is_ws(unit) {
            result.push(index as i32 + 1);
        }
        was_in_whitespace = in_whitespace;
    }
    result
}

/// Uma cor de CSS em hexadecimal (`#rgb`, `#rrggbb`, `#rrggbbaa`), a das réguas com cor própria.
fn parse_css_color(text: &str) -> Option<Rgba> {
    let hex = text.strip_prefix('#')?;
    if hex.len() == 3 || hex.len() == 4 {
        let expanded: String = hex.chars().flat_map(|c| [c, c]).collect();
        return parse_hex_color(&format!("#{expanded}"));
    }
    parse_hex_color(text)
}

/// A linha de base de uma linha de texto numa caixa de altura `box_height` que começa em `top`: o
/// LayoutNG soma à ascent a metade do espaço que sobra, arredondada para baixo, e o Skia põe o glifo no pixel.
fn text_baseline(top: f64, box_height: f64, line: &TextLine) -> f64 {
    (top.round() + ((box_height - f64::from(line.line_height)) / 2.0).floor() + f64::from(line.baseline)).round()
}

impl Drop for EditorView {
    fn drop(&mut self) {
        for subscription in self.subscriptions.drain(..) {
            subscription.dispose();
        }
        self.model.on_before_detached(&self.attached);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_color() {
        assert_eq!(parse_hex_color("#1F2E3D"), Some(Rgba::opaque(0x1f, 0x2e, 0x3d)));
        assert_eq!(parse_hex_color("#1F2E3D80"), Some(Rgba::new(0x1f, 0x2e, 0x3d, 0x80)));
        assert_eq!(parse_hex_color(""), None);
        assert_eq!(parse_hex_color("#12"), None);
    }
}
