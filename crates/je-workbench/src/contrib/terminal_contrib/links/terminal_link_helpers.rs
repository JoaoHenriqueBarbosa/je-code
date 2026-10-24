//! Porte de `src/vs/workbench/contrib/terminalContrib/links/browser/terminalLinkHelpers.ts`: a conversão
//! entre as posições de um link (índices de texto) e as do buffer do emulador, que leva em conta os
//! caracteres largos e os de várias unidades.
//!
//! Notas de tradução:
//! - `IBufferLine`, `IBuffer` e os tipos de posição são o contrato de [`crate::contrib::terminal::xterm_buffer`].
//! - `IRange` é o [`Range`] do editor (o mesmo tipo em Rust, com campos públicos).
//! - `ITerminalLogService` (um `ILogService` com uma marca de tipo) é o [`LogService`].
//! - `IPath` é o [`PathModule`], que recebe o ambiente (cwd) explícito em vez de ler o `process` global,
//!   por isso [`os_path_module`] recebe o [`Environment`].
//! - `ITerminalCapabilityStore.get<T>` é o `TerminalCapabilityStoreExt::get`.
//! - `number | undefined` é `Option`, e o par `lastFgAttr`/`lastBgAttr` (com `-1` de "ainda não vi
//!   nenhuma célula") junto de `bufferRangeStart` é um `Option` só, porque os três nascem juntos.
//! - Os laços de células do início e do fim do link diferem só no ponto de partida e na regra da última
//!   célula da linha, então dividem [`wide_char_line_offset`].
//! - A função comentada `positionIsInRange` do original não é portada.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::js_string::JsString;
use je_base::path::{Environment, PathModule};
use je_base::platform::OperatingSystem;
use je_editor::range::Range;
use je_platform::log::{LogArg, LogService};
use je_platform::terminal::capabilities::capabilities::{CommandDetection, ITerminalCapabilityStore};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStoreExt;

use crate::contrib::terminal::xterm_buffer::{IBuffer, IBufferCellPosition, IBufferLine, IBufferRange, IViewportRange};

/// `Math.ceil(a / b)`.
fn ceil_div(a: i32, b: i32) -> i32 {
    (f64::from(a) / f64::from(b)).ceil() as i32
}

/// O deslocamento de uma linha do buffer causado pelas células de `start_x` até o limite que o
/// `line_length` e o próprio deslocamento vão empurrando (o limite cresce a cada caractere largo).
/// `count_early_wrap` liga a regra do fim do link: a última célula da linha vazia indica que um caractere
/// largo quebrou cedo.
fn wide_char_line_offset(line: &dyn IBufferLine, start_x: i32, buffer_width: i32, line_length: i32, count_early_wrap: bool) -> i32 {
    let mut line_offset = 0;
    let mut x = start_x;
    while x < buffer_width.min(line_length + line_offset) {
        let Some(cell) = line.get_cell(x) else {
            // Inesperado, mas significa que o caractere não existe, então não soma ao deslocamento.
            break;
        };
        let width = cell.get_width();
        let chars = cell.get_chars();
        // Deslocamento das células nulas que seguem caracteres largos.
        if width == 2 {
            line_offset += 1;
        }
        // Deslocamento da quebra cedo quando a última célula da linha é um caractere largo.
        if count_early_wrap && x == buffer_width - 1 && chars.is_empty() {
            line_offset += 1;
        }
        // Deslocamento dos caracteres de vários códigos, como emoji.
        if chars.len() > 1 {
            line_offset -= chars.len() as i32 - 1;
        }
        x += 1;
    }
    line_offset
}

/// Converte o intervalo de um link, possivelmente quebrado em várias linhas (feito de índices de texto), num
/// intervalo do buffer que o emulador entende.
///
/// `lines` é uma linha só (não o buffer inteiro), `buffer_width` é o número de colunas do terminal, `range`
/// é o intervalo do link (índices de texto) e `start_line` é a posição y absoluta (no buffer) da linha.
pub fn convert_link_range_to_buffer(lines: &[Rc<dyn IBufferLine>], buffer_width: i32, range: &Range, start_line: i32) -> IBufferRange {
    let mut buffer_range = IBufferRange {
        start: IBufferCellPosition { x: range.start_column, y: range.start_line_number + start_line },
        end: IBufferCellPosition { x: range.end_column - 1, y: range.end_line_number + start_line },
    };

    // Desloca o início para a direita a cada caractere largo antes do link.
    let mut start_offset = 0;
    let start_wrapped_line_count = ceil_div(range.start_column, buffer_width);
    for y in 0..start_wrapped_line_count {
        let line_length = buffer_width.min((range.start_column - 1) - y * buffer_width);
        // Verificação de sanidade: isso acontece, mas não se sabe em que circunstâncias. Segue adiante e
        // pula o resto do deslocamento do início para minimizar o impacto.
        let Some(line) = lines.get(y as usize) else {
            break;
        };
        start_offset += wide_char_line_offset(line.as_ref(), 0, buffer_width, line_length, false);
    }

    // Desloca o fim para a direita a cada caractere largo dentro do link.
    let mut end_offset = 0;
    let end_wrapped_line_count = ceil_div(range.end_column, buffer_width);
    for y in (start_wrapped_line_count - 1).max(0)..end_wrapped_line_count {
        let start = if y == start_wrapped_line_count - 1 { (range.start_column - 1 + start_offset) % buffer_width } else { 0 };
        let line_length = buffer_width.min(range.end_column + start_offset - y * buffer_width);
        // Mesma verificação de sanidade do início.
        let Some(line) = lines.get(y as usize) else {
            break;
        };
        end_offset += wide_char_line_offset(line.as_ref(), start, buffer_width, line_length, true);
    }

    // Aplica os deslocamentos de caracteres largos ao resultado.
    buffer_range.start.x += start_offset;
    buffer_range.end.x += start_offset + end_offset;

    // Converte de volta para linhas quebradas.
    while buffer_range.start.x > buffer_width {
        buffer_range.start.x -= buffer_width;
        buffer_range.start.y += 1;
    }
    while buffer_range.end.x > buffer_width {
        buffer_range.end.x -= buffer_width;
        buffer_range.end.y += 1;
    }

    buffer_range
}

pub fn convert_buffer_range_to_viewport(buffer_range: &IBufferRange, viewport_y: i32) -> IViewportRange {
    IViewportRange {
        start: IBufferCellPosition { x: buffer_range.start.x - 1, y: buffer_range.start.y - viewport_y - 1 },
        end: IBufferCellPosition { x: buffer_range.end.x - 1, y: buffer_range.end.y - viewport_y - 1 },
    }
}

pub fn get_xterm_line_content(buffer: &dyn IBuffer, line_start: i32, line_end: i32, cols: i32) -> JsString {
    // Limita o número de linhas geradas para evitar problemas de desempenho. É mais uma verificação de
    // sanidade, já que a linha quebrada já deveria estar reduzida a esta altura.
    let max_line_length = 2048.max(cols * 2);
    let line_end = line_end.min(line_start + max_line_length);
    let mut content = JsString::new();
    for i in line_start..=line_end {
        // Só de 0 a cols conta: ao redimensionar no modo do Windows o buffer guarda dados além da largura
        // do terminal, porque o reflow fica desligado.
        if let Some(line) = buffer.get_line(i) {
            content.push_units(line.translate_to_string(Some(true), Some(0), Some(cols)).units());
        }
    }
    content
}

pub fn get_xterm_ranges_by_attr(buffer: &dyn IBuffer, line_start: i32, line_end: i32, cols: i32) -> Vec<IBufferRange> {
    // O início do intervalo em aberto e os últimos atributos (fg, bg) vistos: ficam indefinidos juntos até a
    // primeira célula.
    let mut current: Option<(IBufferCellPosition, (u32, u32))> = None;
    let mut ranges = Vec::new();
    for y in line_start..=line_end {
        let Some(line) = buffer.get_line(y) else {
            continue;
        };
        for x in 0..cols {
            let Some(cell) = line.get_cell(x) else {
                break;
            };
            // HACK: reconstrói os atributos a partir de fg e bg, o que depende do layout de bits interno
            // do buffer.
            let this_attrs = (
                cell.is_bold() | cell.is_inverse() | cell.is_strikethrough() | cell.is_underline(),
                cell.is_dim() | cell.is_italic(),
            );
            let position = IBufferCellPosition { x, y };
            match current {
                None => current = Some((position, this_attrs)),
                Some((start, last_attrs)) if last_attrs != this_attrs => {
                    // Limitação herdada do original: o estouro de x não é tratado.
                    ranges.push(IBufferRange { start, end: position });
                    current = Some((position, this_attrs));
                }
                Some(_) => {}
            }
        }
    }
    ranges
}

/// Para shells com a capacidade CommandDetection, o cwd de um comando relativo à linha de um link pode
/// restringir o resultado a uma correspondência exata de arquivo.
pub fn update_link_with_relative_cwd(
    capabilities: &dyn ITerminalCapabilityStore,
    y: f64,
    text: &str,
    os_path: &PathModule,
    log_service: &dyn LogService,
) -> Option<Vec<String>> {
    let cwd = capabilities.get::<CommandDetection>().and_then(|detection| detection.get_cwd_for_line(y));
    let logged_cwd = cwd.as_deref().map_or(Dynamic::Undefined, |cwd| Dynamic::String(JsString::from(cwd)));
    log_service.trace_args("terminalLinkHelpers#updateLinkWithRelativeCwd cwd", &[LogArg::Value(logged_cwd)]);
    let cwd = cwd.filter(|cwd| !cwd.is_empty())?;
    let mut result = Vec::new();
    let sep = os_path.sep();
    if !text.contains(sep) {
        result.push(os_path.resolve(&[&format!("{cwd}{sep}{text}")]));
    } else {
        let mut common_dirs = 0;
        let mut i = 0;
        let mut cwd_path: Vec<&str> = cwd.split(sep).collect();
        cwd_path.reverse();
        let link_path: Vec<&str> = text.split(sep).collect();
        // Pega todos os resultados como candidatos, priorizando o link com mais diretórios em comum. Por
        // exemplo, no diretório /home/common com o link common/file, o resultado deve ser
        // `["/home/common/common/file", "/home/common/file"]`. O primeiro é o mais provável, já que a
        // detecção de cwd está ativa.
        while i < cwd_path.len() {
            let remaining = link_path.iter().skip(common_dirs).copied().collect::<Vec<_>>().join(sep);
            result.push(os_path.resolve(&[&format!("{cwd}{sep}{remaining}")]));
            if link_path.get(i) == Some(&cwd_path[i]) {
                common_dirs += 1;
            } else {
                break;
            }
            i += 1;
        }
    }
    Some(result)
}

/// `osPathModule(os)`: o `win32` no Windows e o `posix` nos demais, sobre o `env` (cwd).
pub fn os_path_module(os: OperatingSystem, env: &dyn Environment) -> PathModule<'_> {
    if os == OperatingSystem::Windows { PathModule::win32(env) } else { PathModule::posix(env) }
}
