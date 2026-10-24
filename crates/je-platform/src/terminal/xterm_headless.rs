//! A interface do emulador de terminal sem tela que a camada de plataforma lê.
//!
//! No VS Code ela vem do `@xterm/headless` (`Terminal`, `IBuffer`, `IBufferLine`, `IBufferCell`). O je-code
//! terá emulador próprio, e este módulo é o contrato que ele vai implementar: só os membros que o código de
//! `vs/platform/terminal` lê (hoje, o `promptInputModel.ts`). O [`IMarker`] do `@xterm/headless` já existe em
//! [`super::capabilities::capabilities`].
//!
//! Relação com `je_workbench::contrib::terminal::xterm_buffer`: aquele é o mesmo contrato visto do
//! workbench (`@xterm/xterm`), com os membros que o `terminalLinkHelpers.ts` lê. Como o workbench depende da
//! plataforma e não o contrário, este contrato fica aqui, mais completo (posição do cursor, atributos de cor
//! da célula e eventos do terminal), e o do workbench passa a ser uma reexportação quando o emulador existir.
//!
//! Notas de tradução:
//! - Os números do xterm (`number`) são `i32`; as máscaras de atributo das células (`isBold()` e companhia) são
//!   `u32`, diferentes de zero quando o atributo está ligado.
//! - O texto é UTF-16 ([`JsString`]), porque `getChars().length` e `translateToString` contam unidades UTF-16.
//! - `terminal.buffer.active` é [`IHeadlessTerminal::buffer_active`], e as propriedades viram métodos.
//! - `getCell(x, cell?)` perde o parâmetro opcional de reaproveitamento da célula: aqui devolve a célula.

use std::rc::Rc;

use je_base::event::Event;
use je_base::js_string::JsString;

/// `IBufferCell`: uma célula de uma linha do buffer.
pub trait IBufferCell {
    /// Os caracteres da célula (vazio se não há nenhum).
    fn get_chars(&self) -> JsString;

    /// O código Unicode da célula (0 se ela está vazia).
    fn get_code(&self) -> i32;

    /// A cor de frente, no formato que o `getFgColorMode` indica.
    fn get_fg_color(&self) -> i32;

    /// A cor de fundo, no formato que o `getBgColorMode` indica.
    fn get_bg_color(&self) -> i32;

    /// O modo da cor de frente (padrão, paleta de 16, paleta de 256 ou RGB).
    fn get_fg_color_mode(&self) -> i32;

    /// O modo da cor de fundo (padrão, paleta de 16, paleta de 256 ou RGB).
    fn get_bg_color_mode(&self) -> i32;

    /// Diferente de zero se a célula está em negrito.
    fn is_bold(&self) -> u32;

    /// Diferente de zero se a célula está em itálico.
    fn is_italic(&self) -> u32;

    /// Diferente de zero se a célula está esmaecida.
    fn is_dim(&self) -> u32;

    /// Diferente de zero se a célula está sublinhada.
    fn is_underline(&self) -> u32;

    /// Diferente de zero se a célula está piscando.
    fn is_blink(&self) -> u32;

    /// Diferente de zero se a célula está com as cores invertidas.
    fn is_inverse(&self) -> u32;

    /// Diferente de zero se a célula está invisível.
    fn is_invisible(&self) -> u32;

    /// Diferente de zero se a célula está tachada.
    fn is_strikethrough(&self) -> u32;

    /// Diferente de zero se a célula está com linha em cima.
    fn is_overline(&self) -> u32;
}

/// `IBufferLine`: uma linha do buffer.
pub trait IBufferLine {
    /// Diz se a linha é a continuação da anterior (quebra automática).
    fn is_wrapped(&self) -> bool;

    /// O número de células da linha.
    fn length(&self) -> i32;

    /// A célula da coluna `x` (começando em 0), ou `None` se ela não existe.
    fn get_cell(&self, x: i32) -> Option<Rc<dyn IBufferCell>>;

    /// A linha como texto. `trim_right` corta os espaços do fim, e `start_column` e `end_column` (exclusivo)
    /// escolhem o trecho; sem eles vale a linha inteira.
    fn translate_to_string(&self, trim_right: Option<bool>, start_column: Option<i32>, end_column: Option<i32>) -> JsString;
}

/// `IBuffer`: o buffer do terminal.
pub trait IBuffer {
    /// A coluna do cursor (começando em 0).
    fn cursor_x(&self) -> i32;

    /// A linha do cursor, relativa ao topo da área visível (começando em 0).
    fn cursor_y(&self) -> i32;

    /// A linha absoluta do topo da área visível (as linhas acima são o scrollback).
    fn base_y(&self) -> i32;

    /// A linha do índice absoluto `y` (começando em 0), ou `None` se ela não existe.
    fn get_line(&self, y: i32) -> Option<Rc<dyn IBufferLine>>;
}

/// `Terminal` do `@xterm/headless`, só com o que a camada de plataforma lê.
pub trait IHeadlessTerminal {
    /// `terminal.buffer.active`.
    fn buffer_active(&self) -> Rc<dyn IBuffer>;

    /// `terminal.rows`.
    fn rows(&self) -> i32;

    /// `terminal.onCursorMove`.
    fn on_cursor_move(&self) -> Event<()>;

    /// `terminal.onData`: o que o usuário digitou (a entrada que vai para o processo).
    fn on_data(&self) -> Event<JsString>;

    /// `terminal.onWriteParsed`.
    fn on_write_parsed(&self) -> Event<()>;
}
