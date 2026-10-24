//! A interface do buffer do emulador de terminal que o workbench lê.
//!
//! No VS Code ela vem do `@xterm/xterm` (`IBuffer`, `IBufferLine`, `IBufferCell`, `IBufferCellPosition`,
//! `IBufferRange`, `IViewportRange`). O je-code terá emulador próprio, e este módulo é o contrato que ele
//! vai implementar: só os membros que o código do workbench lê (hoje, `terminalLinkHelpers.ts`).
//!
//! Notas de tradução:
//! - Os números do xterm (`number`) são `i32`; as máscaras de atributo das células são `u32`, combinadas com
//!   `|` como no original.
//! - O texto é UTF-16 ([`JsString`]), porque `getChars().length` e `translateToString` contam unidades
//!   UTF-16 (um emoji são duas unidades).
//! - `getCell(x, cell?)` perde o parâmetro opcional de reaproveitamento da célula: aqui devolve a célula.
//! - O `IViewportRange` do xterm tem o mesmo formato do `IBufferRange`, e os dois são o mesmo tipo.

use std::rc::Rc;

use je_base::js_string::JsString;

/// `IBufferCellPosition`: a posição de uma célula no buffer (`x` é a coluna e `y` a linha absoluta).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IBufferCellPosition {
    pub x: i32,
    pub y: i32,
}

/// `IBufferRange`: o intervalo entre duas células do buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IBufferRange {
    pub start: IBufferCellPosition,
    pub end: IBufferCellPosition,
}

/// `IViewportCellPosition`: a posição de uma célula relativa à área visível.
pub type IViewportCellPosition = IBufferCellPosition;

/// `IViewportRange`: o intervalo entre duas células relativo à área visível.
pub type IViewportRange = IBufferRange;

/// `IBufferCell`: uma célula de uma linha do buffer.
pub trait IBufferCell {
    /// A largura da célula em colunas: 0 para a célula vazia que segue um caractere largo, 1 ou 2.
    fn get_width(&self) -> i32;

    /// Os caracteres da célula (vazio se não há nenhum). Um emoji ou uma sequência com caracteres
    /// combinados passa de uma unidade UTF-16.
    fn get_chars(&self) -> JsString;

    /// Diferente de zero se a célula está em negrito.
    fn is_bold(&self) -> u32;

    /// Diferente de zero se a célula está com as cores invertidas.
    fn is_inverse(&self) -> u32;

    /// Diferente de zero se a célula está tachada.
    fn is_strikethrough(&self) -> u32;

    /// Diferente de zero se a célula está sublinhada.
    fn is_underline(&self) -> u32;

    /// Diferente de zero se a célula está esmaecida.
    fn is_dim(&self) -> u32;

    /// Diferente de zero se a célula está em itálico.
    fn is_italic(&self) -> u32;
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

/// `IBuffer`: o buffer do terminal, de onde se leem as linhas pelo índice absoluto.
pub trait IBuffer {
    /// A linha do índice `y` (começando em 0), ou `None` se ela não existe.
    fn get_line(&self, y: i32) -> Option<Rc<dyn IBufferLine>>;
}
