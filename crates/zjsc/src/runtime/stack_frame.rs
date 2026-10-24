//! Porte de `runtime/StackFrame.h` e `runtime/StackFrame.cpp`, a parte de frame JS: o que
//! `Interpreter::getStackTrace` captura em `Exception::m_stack` e que `Error.stack` formata.
//!
//! DIVERGÊNCIAS:
//!
//! - Como no C++, o frame JS guarda só o `CodeBlock` (aqui um `Rc`), o `BytecodeIndex` e o `callee`;
//!   nome, URL, linha, coluna e o recuo do `new` saem sob demanda ([`StackFrame::location`]) e ficam
//!   num cache compartilhado pelos clones do frame (o C++ recalcula a cada `toString`). `this`, o tipo
//!   de código, `strict`, `constructor` e o id do script são lidos na captura porque o `CallFrame` deixa de
//!   existir quando o frame é desempilhado. Frames nativos e assíncronos nascem já resolvidos.
//! - Sem `WasmFrameData`, sem `isAsyncFrameWithoutCodeBlock`, sem `vm.clientData->overrideSourceURL`.
//! - `functionName` usa `CodeBlock::inferredName` (o `ecmaName` do executável) no lugar de
//!   `getCalculatedDisplayName(callee)`, que depende de `displayName`/`name` do `JSFunction`.
//! - O texto de `Error.stack` não é o de `StackFrame::toString` (`fn@url:linha:coluna`, que o JSC
//!   puro usa em `Interpreter::stackTraceAsString`) e sim o do `Bun`, que o `onComputeErrorInfo` do
//!   cliente produz: uma linha de cabeçalho (`Error: mensagem`) e um `    at nome (url:linha:coluna)`
//!   por frame, ou `    at url:linha:coluna` quando o frame não tem nome de função. A fonte do `Bun`
//!   (`ZigGlobalObject`, `JSCStackFrame`) não está na árvore: o prefixo `new ` do construtor e o resto do
//!   formato seguem o que o `Bun` imprime, sem conferência contra o fonte.
//! - O frame também guarda o que `JSCallSite` lê (`callee`, `this`, `code_type`, `strict`, `constructor`),
//!   porque o `CallFrame` deixa de existir quando o frame é desempilhado. `async`, `wasm` e o `[native code]`
//!   não existem: nenhum frame assim entra na captura. O frame de função nativa é o do `Bun`:
//!   `nome (unknown)` ([`StackFrame::native_function`]), e o de builtin em JS público é `nome (native:1:11)`.

use std::cell::{Cell, OnceCell};
use std::fmt;
use std::rc::Rc;

use crate::bytecode::bytecode_index::BytecodeIndex;
use crate::bytecode::code_block::{CodeBlock, CodeBlockRef};
use crate::bytecode::code_type::CodeType;
use crate::bytecode::opcode::OpcodeID;
use crate::interpreter::stack_visitor::{line_and_column_for, to_rust_string};
use crate::interpreter::unwind::source_url_stripped_of;
use crate::parser::source_provider::SourceProvider;
use crate::runtime::js_value::JSValue;
use crate::runtime::script_executable::ScriptExecutableRef;

thread_local! {
    /// O programa é um navegador: `Error.stack`, o `CallSite` e as mensagens de erro seguem o V8, não o bun.
    static V8_STACK_FORMAT: Cell<bool> = const { Cell::new(false) };
}

/// Liga (o navegador) ou desliga o formato de pilha do V8 para o programa em curso.
pub fn set_v8_stack_format(enabled: bool) {
    V8_STACK_FORMAT.with(|flag| flag.set(enabled));
}

/// O programa em curso usa o formato de pilha do V8.
pub fn v8_stack_format() -> bool {
    V8_STACK_FORMAT.try_with(Cell::get).unwrap_or(false)
}

/// Fim do programa: volta ao formato do bun.
pub(crate) fn reset_for_program() {
    let _ = V8_STACK_FORMAT.try_with(|flag| flag.set(false));
    crate::runtime::v8_stack::reset_for_program();
}

/// O que `StackFrame` resolve sob demanda: `functionName(vm)`, `sourceURLStripped(vm)`,
/// `computeLineAndColumn()`, `hasLineAndColumnInfo()` e o recuo do `new`.
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub function_name: String,
    pub source_url: String,
    /// Só vale com `has_line_and_column_info`.
    pub line: u32,
    pub column: u32,
    /// `hasLineAndColumnInfo()`: há `CodeBlock`.
    pub has_line_and_column_info: bool,
    /// Quantos bytes o `Bun` recua a coluna do `CallSite` quando a instrução é `op_construct` (e variantes):
    /// o `startOffset` do `ExpressionInfo`, que leva do `(` ao `new`. 0 nas demais instruções. A linha de
    /// `stack` (texto) continua no `(`.
    pub construct_back_offset: u32,
}

/// O `CodeBlock` e o `BytecodeIndex` do frame JS, de onde a [`Location`] sai.
#[derive(Clone)]
struct JsSource {
    code_block: CodeBlockRef,
    bytecode_index: BytecodeIndex,
}

/// `class StackFrame` (variante `JSFrameData`).
#[derive(Clone)]
pub struct StackFrame {
    /// `JSFrameData::callee`: o `cell_id` do objeto chamado (0 sem callee).
    pub callee: usize,
    /// `callFrame->thisValue()` no momento da captura (`undefined` quando o `this` ainda não existe).
    pub this_value: JSValue,
    /// `codeBlock->codeType()`.
    pub code_type: CodeType,
    /// `codeBlock->ownerExecutable()->isInStrictContext()`.
    pub is_strict: bool,
    /// `codeBlock->isConstructor()`.
    pub is_constructor: bool,
    /// `sourceID()`: o id do `SourceProvider` do frame (0 sem `CodeBlock`), o que `CallSite.getScriptId` devolve.
    pub script_id: i32,
    /// `isAsyncFrame()`: o frame que `getAsyncStackTrace` acrescenta (`at async nome`), o que `CallSite.isAsync` lê.
    pub is_async: bool,
    /// Fonte da resolução preguiçosa; `None` nos frames que nascem resolvidos.
    source: Option<JsSource>,
    /// Cache da resolução, compartilhado pelos clones.
    location: Rc<OnceCell<Location>>,
}

impl fmt::Debug for StackFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StackFrame").field("location", self.location()).field("callee", &self.callee).finish()
    }
}

impl PartialEq for StackFrame {
    fn eq(&self, other: &StackFrame) -> bool {
        self.callee == other.callee
            && self.this_value == other.this_value
            && self.code_type == other.code_type
            && self.is_strict == other.is_strict
            && self.is_constructor == other.is_constructor
            && self.script_id == other.script_id
            && self.location() == other.location()
    }
}

/// `StackFrame::functionName` do `Bun`: no código de função o nome vem do callee (propriedade `name`,
/// `displayName`), não do `inferredName` do `CodeBlock`; sem callee que seja função cai no nome do tipo de código.
fn js_function_name(block: &CodeBlock, callee: usize) -> String {
    match block.code_type() {
        CodeType::EvalCode => "eval code".to_string(),
        CodeType::ModuleCode => "module code".to_string(),
        CodeType::GlobalCode => "global code".to_string(),
        CodeType::FunctionCode => {
            if callee != 0 {
                if let Some(function) = JSValue::from_cell(callee).as_js_function() {
                    return to_rust_string(&function.stack_frame_name(block.global_object().vm(), block.global_object()));
                }
            }
            block.inferred_name()
        }
    }
}

/// O `startOffset` do `ExpressionInfo` quando a instrução do frame constrói (`op_construct` e variantes), que
/// o `Bun` usa para recuar a coluna do `CallSite` do `(` até o `new` (`getAdjustedPositionForBytecode`).
fn construct_back_offset(block: &CodeBlock, bytecode_index: BytecodeIndex) -> u32 {
    let instruction = block.instructions().at(bytecode_index.offset());
    match instruction.opcode_id_enum() {
        OpcodeID::op_construct | OpcodeID::op_construct_varargs | OpcodeID::op_super_construct | OpcodeID::op_super_construct_varargs => {
            block.unlinked_code_block().borrow().expression_info_for_bytecode_index(bytecode_index).start_offset
        }
        _ => 0,
    }
}

/// Quantas colunas o `Bun` recua a posição de um frame, seja a instrução uma chamada, uma construção ou outra
/// com posição (acesso a membro, getter, setter, `in`...).
///
/// O texto de `Error.stack` do `Bun` não mostra a coluna crua do JSC (o `divot`, o `(` da chamada) e sim a
/// coluna remapeada pelo source map do transpilador do `Bun`: `SavedSourceMap::resolve_mapping` ->
/// `Mapping::find` devolve o maior mapping com coluna gerada <= a coluna crua, na mesma linha gerada, e o
/// frame mostra a coluna ORIGINAL dele; sem mapping na linha, a coluna crua fica. O `js_printer` só emite
/// mapping (`add_source_mapping`) no início de certos tokens. Como a saída do printer preserva a ordem dos
/// tokens, o mapping escolhido é o do ÚLTIMO token mapeável cujo início é <= o `divot` na mesma linha.
/// Esta função reproduz isso varrendo o fonte com o `Lexer` do próprio porte, da expressão (`divot -
/// startOffset`, limitada ao começo da linha) até o `divot`, e devolve `divot - início desse token`.
///
/// Tokens mapeáveis (conferidos no `js_printer` do `Bun`, `print_expr`/`print_block`/`print_class`):
/// identificador (inclusive nome após `.`), palavras reservadas (`new`, `this`, `super`, `function`, `class`,
/// `await`, `yield`, `null`, `true`, `false`, `typeof`/`void`/`delete`), número, bigint, string, regexp, início
/// de template, `{` e `}` (bloco, objeto, corpo de classe), `[` e `]` de literal de array, `...` (spread),
/// operador unário prefixo, e o `)` que fecha uma chamada ou `new`. Não mapeiam: `(` (chamada ou agrupamento),
/// `)` de agrupamento, `.`, `?.`, `[` e `]` de índice, operadores binários, `,`, `;`, `:`, `=>`.
///
/// Divergências assumidas: a linha exibida é a do `divot` (o remap também poderia trocar a linha quando o
/// printer junta ou parte linhas); a escolha entre regexp e divisão, entre array e índice, e entre `)` de
/// chamada e de agrupamento sai da classe do token anterior (valor ou não), não de caracteres; sem
/// transpilação os mappings do `Bun` de `(` dos parâmetros de função não existem, o que não altera o resultado
/// porque o `{` do corpo vem depois. 0 se o fonte não passou pelo transpilador, ou se nenhum token serve.
fn callee_back_offset(block: &CodeBlock, bytecode_index: BytecodeIndex) -> u32 {
    // Vale para toda instrução com posição, não só chamada e construção: o `Bun` remapeia o `divot` de qualquer
    // frame. Medido no bun 1.4.2: `null.p` lançando mostra o início de `null` (o `divot` é o `.`, que não
    // mapeia), e um getter ou setter chamado por `o.g` / `o.s = 1` mostra o `o`, não o ponto.
    let source = block.owner_executable().source();
    let Some(provider) = source.provider() else { return 0 };
    // O transpilador do `Bun` só roda sobre o arquivo carregado (fonte com URL própria); o fonte de `eval` e de
    // `new Function` não tem source map, então a coluna fica a crua do JSC (o `(` da chamada). O fonte deles
    // leva como URL a string da origem do chamador (`file:///arq.js`), o que o arquivo carregado não tem. O módulo ES
    // carregado tem a chave como URL e como origem, e também passou pelo transpilador.
    let provider_url = to_rust_string(provider.source_url());
    let is_module = provider.source_type() == crate::parser::source_provider::SourceProviderSourceType::Module;
    if provider.has_raw_columns() || provider_url.is_empty() || (!is_module && provider_url == to_rust_string(provider.source_origin().string())) {
        return 0;
    }
    let text = provider.source();
    let info = block.unlinked_code_block().borrow().expression_info_for_bytecode_index(bytecode_index);
    let source_start = u32::try_from(source.start_offset()).unwrap_or(0);
    let divot = source_start.saturating_add(info.divot);
    if divot >= text.length() {
        return 0;
    }
    // Medido no bun 1.4.2 com o arquivo transpilado: numa construção (`return new Error('x')`) tanto o texto de
    // `stack` quanto `CallSite.getColumnNumber` mostram o início de `Error` (o último mapping até o `(`), não o
    // `new`. O recuo de `getAdjustedPositionForBytecode` só vale para fonte sem source map (ver `location`).
    let end = divot;
    let mut line_start = end;
    while line_start > 0 && !is_line_terminator_unit(text.code_unit_at(line_start - 1)) {
        line_start -= 1;
    }
    let expression_start = divot.saturating_sub(info.start_offset);
    let scan_start = expression_start.max(line_start).max(source_start);
    let vm = block.global_object().vm_rc();
    let found = if text.is_8bit() {
        last_mappable_token_start::<u8>(vm, &source, scan_start, line_start, end)
    } else {
        last_mappable_token_start::<u16>(vm, &source, scan_start, line_start, end)
    };
    found.map_or(0, |start| divot - start)
}

pub(crate) fn is_line_terminator_unit(unit: u16) -> bool {
    unit == 0x0A || unit == 0x0D || unit == 0x2028 || unit == 0x2029
}

/// A linha e a coluna que o frame da instrução `bytecode_index` de `block` mostra, e se a coluna recuou ao último token
/// mapeável: a posição do JSC, a coluna remapeada pelo source map do transpilador do `Bun` e o mapa de posições dos goldens. Vale
/// também para o frame `async` que `getAsyncStackTrace` acrescenta, que o bun remapeia como qualquer outro (medido no
/// bun 1.4.2: `return await b()` sai no `b`, não no `(`).
pub(crate) fn displayed_position(
    code_block: &CodeBlockRef,
    block: &CodeBlock,
    bytecode_index: BytecodeIndex,
) -> (crate::bytecode::line_column::LineColumn, bool) {
    let mut line_column = line_and_column_for(code_block, bytecode_index);
    let mut remapped_to_token = false;
    // O fonte que passou pelo transpilador do zbun carrega o mapa de verdade do impressor: o `Mapping::find` do bun
    // sobre a posição crua, sem a varredura de tokens que simula o mapa quando ele não existe.
    let exact_map = block
        .owner_executable()
        .source()
        .provider()
        .and_then(|provider| provider.position_map())
        .filter(|map| map.is_exact());
    if let Some(map) = exact_map {
        if let Some((line, column)) = map.map_exact(line_column.line, line_column.column) {
            (line_column.line, line_column.column) = (line, column);
        }
        return (line_column, true);
    }
    if v8_stack_format() {
        // O V8 não tem transpilador nem recuo de `CallSite`: a coluna já é a do `SourcePosition`.
        let shift = crate::runtime::v8_stack::position_shift(block, bytecode_index);
        line_column.column = u32::try_from((i64::from(line_column.column) + shift).max(1)).unwrap_or(1);
    } else {
        // O texto de `stack` mostra a coluna remapeada pelo source map do transpilador do `Bun` (o último
        // token mapeável até o `divot`, ver `callee_back_offset`), não o `(`.
        let raw_name_back_offset = callee_back_offset(block, bytecode_index);
        let name_back_offset = raw_name_back_offset.min(line_column.column.saturating_sub(1));
        line_column.column -= name_back_offset;
        remapped_to_token = raw_name_back_offset > 0;
    }
    // O harness dos goldens roda o texto reimpresso e o bun mostra a posição no fonte original.
    if let Some(map) = block.owner_executable().source().provider().and_then(|provider| provider.position_map()) {
        (line_column.line, line_column.column) = map.map(line_column.line, line_column.column);
    }
    (line_column, remapped_to_token)
}

/// Quem escreve o frame: a linha de `stack`, o `CallSite.toString` ou o `console.trace` do bun.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Stack,
    CallSite,
    Trace,
}

/// O que o laço de [`last_mappable_token_start`] guarda por `(`/`[`/`{` aberto: o que o fecho correspondente
/// vai mapear.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenKind {
    /// `(` de chamada ou `new`: o `)` mapeia.
    Call,
    /// `(` de agrupamento, `if`, `for`...: o `)` não mapeia.
    Group,
    /// `[` de literal de array: o `]` mapeia.
    Array,
    /// `[` de índice: o `]` não mapeia.
    Index,
    /// `(` dos parâmetros de `function`: o `)` não mapeia e o `{` seguinte abre um corpo.
    Params,
    /// `{` do corpo de função ou arrow: o `}` não mapeia.
    FnBody,
    /// `{` (bloco, objeto, classe): o `}` mapeia.
    Brace,
    /// `${` de template: o `}` reabre a leitura do template.
    Template,
}

/// Início (offset absoluto no fonte) do último token mapeável com início <= `end`, varrendo de `scan_start`
/// (`line_start` é o começo da linha do `end`). Ver [`callee_back_offset`].
fn last_mappable_token_start<T: crate::wtf::text::string_impl::CharType>(
    vm: Rc<crate::runtime::vm::VM>,
    source: &crate::parser::source_code::SourceCode,
    scan_start: u32,
    line_start: u32,
    end: u32,
) -> Option<u32> {
    use crate::parser::lexer::{Lexer, LexerFlagSet, RawStringsBuildMode};
    use crate::parser::parser_arena::ParserArena;
    use crate::parser::parser_modes::{JSParserBuiltinMode, JSParserScriptMode};
    use crate::parser::parser_tokens::*;

    let mut arena = ParserArena::new();
    let mut lexer = Lexer::<T>::new(vm, JSParserBuiltinMode::NotBuiltin, JSParserScriptMode::Classic);
    lexer.set_code(source, &mut arena);
    lexer.set_offset(scan_start as i32, line_start as i32);
    let mut stack: Vec<OpenKind> = Vec::new();
    let mut previous_is_value = false;
    // `function` já visto e o `(` dos parâmetros ainda não; `)` dos parâmetros ou `=>` recém-lidos (o `{` seguinte
    // abre corpo de função).
    let mut params_next = false;
    let mut body_next = false;
    let mut found: Option<u32> = None;
    let mut token = JSToken::default();
    loop {
        let mut type_ = lexer.lex(&mut token, LexerFlagSet::default(), false);
        let start = token.start_position.offset as u32;
        if type_ == EOFTOK || (type_ & CAN_BE_ERROR_TOKEN_FLAG != 0 && lexer.saw_error()) || start > end {
            break;
        }
        // Regexp no lugar de divisão: `/` onde um valor não pode terminar.
        if (type_ == DIVIDE || type_ == DIVEQUAL) && !previous_is_value {
            type_ = lexer.scan_reg_exp(&mut token, if type_ == DIVEQUAL { u16::from(b'=') } else { 0 });
            if lexer.saw_error() {
                break;
            }
        }
        let mut mapped = false;
        let mut value_after = false;
        let was_body_next = std::mem::take(&mut body_next);
        match type_ {
            OPENPAREN => {
                if std::mem::take(&mut params_next) {
                    stack.push(OpenKind::Params);
                } else {
                    stack.push(if previous_is_value { OpenKind::Call } else { OpenKind::Group });
                }
            }
            CLOSEPAREN => {
                match stack.pop() {
                    Some(OpenKind::Call) => mapped = true,
                    Some(OpenKind::Params) => body_next = true,
                    _ => {}
                }
                value_after = true;
            }
            ARROWFUNCTION => body_next = true,
            FUNCTION => {
                mapped = true;
                params_next = true;
            }
            OPENBRACKET => {
                mapped = !previous_is_value;
                stack.push(if previous_is_value { OpenKind::Index } else { OpenKind::Array });
            }
            CLOSEBRACKET => {
                mapped = stack.pop() == Some(OpenKind::Array);
                value_after = true;
            }
            OPENBRACE => {
                mapped = true;
                stack.push(if was_body_next { OpenKind::FnBody } else { OpenKind::Brace });
            }
            CLOSEBRACE => {
                if stack.last() == Some(&OpenKind::FnBody) {
                    // Medido no bun 1.4.2: o `}` do corpo de função ou arrow não tem mapping (o `stack` cai no
                    // token anterior), ao contrário do `}` de bloco, objeto e classe.
                    stack.pop();
                    value_after = true;
                } else if stack.last() == Some(&OpenKind::Template) {
                    stack.pop();
                    let part = lexer.scan_template_string(&mut token, RawStringsBuildMode::DontBuildRawStrings);
                    if part != TEMPLATE {
                        break;
                    }
                    if !token.data.is_tail {
                        stack.push(OpenKind::Template);
                    }
                    value_after = token.data.is_tail;
                } else {
                    stack.pop();
                    mapped = true;
                    value_after = true;
                }
            }
            BACKQUOTE => {
                mapped = true;
                let part = lexer.scan_template_string(&mut token, RawStringsBuildMode::DontBuildRawStrings);
                if part != TEMPLATE {
                    break;
                }
                if !token.data.is_tail {
                    stack.push(OpenKind::Template);
                }
                value_after = token.data.is_tail;
            }
            INTEGER | DOUBLE | BIGINT | IDENT | PRIVATENAME | STRING | REGEXP | THISTOKEN | SUPER | NULLTOKEN | TRUETOKEN | FALSETOKEN => {
                mapped = true;
                value_after = true;
            }
            DOTDOTDOT => mapped = true,
            DOT | QUESTIONDOT => {}
            _ if type_ & UNARY_OP_TOKEN_FLAG != 0 => mapped = !previous_is_value,
            _ if type_ & KEYWORD_TOKEN_FLAG != 0 => mapped = true,
            _ => {}
        }
        if mapped {
            found = Some(start);
        }
        previous_is_value = value_after;
    }
    found
}

impl StackFrame {
    /// `StackFrame::visitAggregate`: o callee e o `this` (o `CodeBlock` é `Rc`, já raiz).
    pub(crate) fn visit_aggregate(&self, marker: &mut crate::runtime::gc::Marker) {
        marker.cell(self.callee);
        marker.value(self.this_value);
    }
    /// Frame JS preguiçoso: guarda o `CodeBlock` e o `BytecodeIndex`; o resto dos campos o chamador preenche.
    pub fn lazy(code_block: CodeBlockRef, bytecode_index: BytecodeIndex, callee: usize) -> StackFrame {
        StackFrame {
            callee,
            this_value: JSValue::undefined(),
            code_type: CodeType::FunctionCode,
            is_strict: false,
            is_constructor: false,
            script_id: 0,
            is_async: false,
            source: Some(JsSource { code_block, bytecode_index }),
            location: Rc::new(OnceCell::new()),
        }
    }

    /// Frame que nasce com a [`Location`] pronta (nativo, assíncrono); o resto dos campos o chamador preenche.
    pub fn resolved(location: Location) -> StackFrame {
        StackFrame {
            callee: 0,
            this_value: JSValue::undefined(),
            code_type: CodeType::FunctionCode,
            is_strict: false,
            is_constructor: false,
            script_id: 0,
            is_async: false,
            source: None,
            location: Rc::new(OnceCell::from(location)),
        }
    }

    /// Nome, URL, linha e coluna do frame, calculados na primeira chamada (`StackFrame::functionName`,
    /// `sourceURLStripped`, `computeLineAndColumn`).
    pub fn location(&self) -> &Location {
        self.location.get_or_init(|| {
            let source = self.source.as_ref().expect("frame sem Location nem fonte");
            let block = source.code_block.borrow();
            let owner = block.owner_executable();
            let source_url = source_url_stripped_of(owner);
            let (line_column, remapped_to_token) = displayed_position(&source.code_block, &block, source.bytecode_index);
            // O V8 não tem recuo de `CallSite`, e com source map (arquivo transpilado) a construção fica no último mapping
            // até o `(`, sem recuo ao `new`.
            let construct_back_offset =
                if v8_stack_format() || remapped_to_token { 0 } else { construct_back_offset(&block, source.bytecode_index) };
            let is_builtin = matches!(owner, ScriptExecutableRef::Function(function) if function.borrow().is_builtin_function());
            // Um builtin com `sourceURL` (os módulos embutidos do bun, `node:events`) mostra a posição real no fonte dele;
            // só o builtin do próprio JSC, sem URL, vira `native:1:11`.
            let is_native_builtin = is_builtin && source_url.is_empty();
            // Medido no bun 1.4.2: o construtor padrão (`class B extends A {}`) tem o fonte sintético
            // `(function (...args) { super(...args); })` sem URL, e o frame sai `new B (unknown:1:28)`.
            let is_default_constructor =
                matches!(owner, ScriptExecutableRef::Function(function) if function.borrow().is_builtin_default_class_constructor());
            let source_url = if is_default_constructor && source_url.is_empty() { "unknown".to_string() } else { source_url };
            let location = Location {
                function_name: js_function_name(&block, self.callee),
                source_url,
                line: line_column.line,
                column: line_column.column,
                has_line_and_column_info: true,
                construct_back_offset,
            };
            if is_native_builtin { location.with_builtin_location() } else { location }
        })
    }

    /// O texto do fonte do `SourceProvider` do frame (de onde o relato de erro tira o trecho de contexto); `None` nos
    /// frames sem `CodeBlock`.
    pub fn source_text(&self) -> Option<String> {
        let source = self.source.as_ref()?;
        let block = source.code_block.borrow();
        let code = block.owner_executable().source();
        let provider = code.provider()?;
        // Arquivo reimpresso pelo transpilador: o trecho de código do relato de erro é o do fonte original, como no bun.
        if let Some(original) = provider.position_map().and_then(|map| map.original_source().map(str::to_string)) {
            return Some(original);
        }
        Some(crate::runtime::js_module_loader::rust_string(&provider.source()))
    }

    /// A origem do `eval` ou `new Function` de onde o código do frame veio (formato de pilha do V8).
    pub fn eval_origin(&self) -> Option<String> {
        let source = self.source.as_ref()?;
        let block = source.code_block.borrow();
        let code = block.owner_executable().source();
        code.provider()?.eval_origin()
    }

    /// `functionName(vm)`.
    pub fn function_name(&self) -> &str {
        &self.location().function_name
    }

    /// `sourceURLStripped(vm)`.
    pub fn source_url(&self) -> &str {
        &self.location().source_url
    }

    /// `computeLineAndColumn().line`; só vale com [`StackFrame::has_line_and_column_info`].
    pub fn line(&self) -> u32 {
        self.location().line
    }

    /// `computeLineAndColumn().column`; só vale com [`StackFrame::has_line_and_column_info`].
    pub fn column(&self) -> u32 {
        self.location().column
    }

    /// `hasLineAndColumnInfo()`: há `CodeBlock`.
    pub fn has_line_and_column_info(&self) -> bool {
        self.location().has_line_and_column_info
    }

    /// A frame da função nativa `eval` que o `Bun` mostra logo depois da frame do código de `eval`
    /// (`at eval (unknown)`): o laço não empilha frame para função nativa, e o código de `eval` só entra
    /// por ela, então a captura a acrescenta.
    pub fn native_eval() -> StackFrame {
        StackFrame::native_function("eval".to_string(), 0, JSValue::undefined())
    }

    /// A frame de uma função nativa (`CodeType::Native` do `StackVisitor`, sem `CodeBlock`): o `Bun` mostra
    /// `nome (unknown)`, sem linha nem coluna.
    pub fn native_function(function_name: String, callee: usize, this_value: JSValue) -> StackFrame {
        let mut frame = StackFrame::resolved(Location {
            function_name,
            source_url: "unknown".to_string(),
            line: 0,
            column: 0,
            has_line_and_column_info: false,
            construct_back_offset: 0,
        });
        frame.callee = callee;
        frame.this_value = this_value;
        frame
    }

    /// Frame de função nativa (`eval`, sem informação de posição) ou de builtin em JS (`native`): o
    /// `CallSite.isNative` do `Bun` vale `true` nas duas (medido na 1.4.2: `map`, `reduce` e `eval`).
    pub fn is_native_function(&self) -> bool {
        !self.has_line_and_column_info() || self.source_url() == "native"
    }

    /// `CallSite.getFileName` e `getScriptNameOrSourceURL` (medido no `Bun` 1.4.2): `[native code]` na frame
    /// nativa, `[unknown]` na de builtin em JS, `None` (undefined) sem URL e a URL nos demais.
    pub fn call_site_file_name(&self) -> Option<String> {
        if !self.has_line_and_column_info() {
            return Some("[native code]".to_string());
        }
        match self.source_url() {
            "" => None,
            "native" => Some("[unknown]".to_string()),
            url => Some(url.to_string()),
        }
    }

    /// `CallSite.getLineNumber`: 1 na frame nativa, como no `Bun`.
    pub fn call_site_line_number(&self) -> u32 {
        if self.has_line_and_column_info() { self.line() } else { 1 }
    }

    /// `CallSite.getColumnNumber`: base zero (`toString` mostra +1); 0 na frame nativa.
    pub fn call_site_column_number(&self) -> u32 {
        if self.has_line_and_column_info() { self.call_site_column().saturating_sub(1) } else { 0 }
    }

    /// A coluna (base um) que o `CallSite` do `Bun` mostra: recuada até o `new` nas instruções de construção
    /// (`getAdjustedPositionForBytecode`), sem passar da primeira coluna da linha.
    fn call_site_column(&self) -> u32 {
        let location = self.location();
        if location.construct_back_offset < location.column { location.column - location.construct_back_offset } else { location.column }
    }

    /// `CallSite.prototype.toString`: `nome (native)` nas frames nativas e de builtin (o texto da linha de
    /// `stack` é outro: `eval (unknown)`, `map (native:1:11)`); nas demais é o da linha de `stack`.
    pub fn call_site_text(&self) -> String {
        if self.is_native_function() { format!("{} (native)", self.display_name()) } else { self.format_call_site(self.call_site_column(), Style::CallSite) }
    }

    /// Os quatro campos de `CallSite.prototype.toJSON` do `Bun` (medido na 1.4.2):
    /// `(sourceURL, lineNumber, columnNumber, functionName)`. A coluna é de base zero; a frame nativa é
    /// `[native code]`, linha 0, coluna -1, e a de builtin em JS (`native`) é `[unknown]`.
    pub fn call_site_json_fields(&self) -> (String, i64, i64, String) {
        let name = self.call_site_function_name().unwrap_or_default().to_string();
        if !self.has_line_and_column_info() {
            return ("[native code]".to_string(), 0, -1, name);
        }
        let url = if self.source_url() == "native" { "[unknown]".to_string() } else { self.source_url().to_string() };
        (url, i64::from(self.line()), i64::from(self.call_site_column()) - 1, name)
    }

    /// A frame de uma função embutida em JS (`@builtin`) pública, como `map` ou `new Promise`: o `Bun` a
    /// mostra com o fonte `native` na linha 1, coluna 11, seja qual for o ponto de execução dentro dela.
    pub fn with_builtin_location(mut self) -> StackFrame {
        let location = self.location().clone().with_builtin_location();
        self.source = None;
        self.location = Rc::new(OnceCell::from(location));
        self
    }

    /// O nome que a linha `    at` mostra: o código de programa, o de `eval` e a função sem nome são
    /// `<anonymous>` no `Bun` (medido: o `eval` aparece como `<anonymous>` seguido de uma frame nativa
    /// `eval (unknown)`, que [`StackFrame::native_eval`] cria) e o módulo é `module code`.
    fn display_name(&self) -> &str {
        match self.function_name() {
            "" | "global code" | "eval code" => "<anonymous>",
            // Frame assíncrono (`StackFrame::functionName` com `isAsyncFrame`) de função sem nome: `async <anonymous>`.
            "async " => "async <anonymous>",
            name => name,
        }
    }

    /// O nome de função que `CallSite.getFunctionName` devolve: `None` para o código de programa, de
    /// `eval` e de módulo e para a função sem nome.
    pub fn call_site_function_name(&self) -> Option<&str> {
        match self.function_name() {
            "" | "global code" | "eval code" | "module code" => None,
            // Frame assíncrono: `getFunctionName` devolve o nome sem o `async ` (medido: `"f"`, e `""` se anônima).
            name if self.is_async => Some(name.strip_prefix("async ").unwrap_or(name)).filter(|bare| !bare.is_empty()),
            name => Some(name),
        }
    }

    /// `CallSite.prototype.toString` e o texto depois do `at ` da linha de `stack`.
    pub fn to_call_site_string(&self) -> String {
        self.format_call_site(self.column(), Style::Stack)
    }

    /// O texto depois do `at ` do `console.trace`, que o bun escreve com o formatador próprio (`ZigStackFrame`), não
    /// com o de `stack` (medido no bun 1.4.2): a coluna sempre aparece, o builtin sai sem a URL (`forEach (1:11)`), o
    /// código de `eval` se chama `eval` e o frame sem posição (função nativa, o `eval` nativo) não aparece (`None`).
    pub fn to_trace_string(&self) -> Option<String> {
        self.has_line_and_column_info().then(|| self.format_call_site(self.column(), Style::Trace))
    }

    fn format_call_site(&self, column: u32, style: Style) -> String {
        let omit_first_column = style == Style::Stack;
        let location = self.location();
        // Função nativa sem nome (o `Proxy` chamado): o `Bun` imprime só `at unknown`.
        if location.function_name.is_empty() && location.source_url == "unknown" {
            return location.source_url.clone();
        }
        // Medido no `bun` 1.4.2: o código de programa (`global code`) com fonte escreve só a posição
        // (`at x.js:3:9`); o de `eval` e a função sem nome escrevem `<anonymous> (x.js:3:9)`. O arquivo principal que o
        // bun roda como módulo ES (`module code`) sai como o de programa.
        let top_level = matches!(location.function_name.as_str(), "global code" | "module code");
        if top_level && !location.source_url.is_empty() && location.has_line_and_column_info {
            // A coluna 1 some só no de programa; no módulo principal o bun a escreve (`at /app/main.js:3:1`).
            return if column == 1 && omit_first_column && location.function_name == "global code" {
                format!("{}:{}", location.source_url, location.line)
            } else {
                format!("{}:{}:{}", location.source_url, location.line, column)
            };
        }
        let name = if style == Style::Trace && location.function_name == "eval code" { "eval" } else { self.display_name() };
        // Medido no `bun` 1.4.2: o texto de `stack` escreve `async f`, mas `CallSite.toString` (a coluna inteira,
        // `omit_first_column == false`) escreve só `f` (ou `<anonymous>`) no frame assíncrono.
        let name = if self.is_async && style == Style::CallSite {
            match name.strip_prefix("async ") {
                Some("") | None => name,
                Some(bare) => bare,
            }
        } else {
            name
        };
        // O `Bun` não escreve `new` na frente de `<anonymous>` (função anônima construída com `new`).
        let name = if self.is_constructor && self.code_type == CodeType::FunctionCode && name != "<anonymous>" {
            format!("new {name}")
        } else {
            name.to_string()
        };
        // O `ZigStackFrame` não tem URL para o builtin nem para o código sem fonte (o inicializador de campo de classe,
        // `unknown` na linha de `stack`): medido no bun 1.4.2, `at new A (1:17)` no trace e `new A (unknown:1:17)` no `stack`.
        if style == Style::Trace && (location.source_url == "native" || location.source_url == "unknown") {
            return format!("{name} ({}:{column})", location.line);
        }
        let location_text = match (location.source_url.is_empty(), location.has_line_and_column_info) {
            (true, _) => return name,
            (false, false) => location.source_url.clone(),
            // O `Bun` omite a coluna quando ela é a primeira (zero na base zero): `at <anonymous> (a.js:3)`.
            // O `CallSite.toString` sempre escreve a coluna (medido: `<anonymous> (file:///a.js:3:1)`).
            (false, true) if column == 1 && omit_first_column => format!("{}:{}", location.source_url, location.line),
            (false, true) => format!("{}:{}:{}", location.source_url, location.line, column),
        };
        format!("{name} ({location_text})")
    }

    /// A linha `    at ...` do frame (sem a quebra de linha).
    pub fn to_stack_line(&self) -> String {
        format!("    at {}", self.to_call_site_string())
    }
}

impl Location {
    /// O fonte `native` na linha 1, coluna 11 dos builtins públicos em JS.
    fn with_builtin_location(mut self) -> Location {
        self.source_url = "native".to_string();
        self.line = 1;
        self.column = 11;
        self.has_line_and_column_info = true;
        self
    }
}

/// O texto de `Error.stack`: `header` (o `Error.prototype.toString` do erro) e uma linha `    at` por frame.
/// `Error.captureStackTrace` e a captura da exceção juntam as linhas com `\n`.
pub fn format_stack_trace(header: &str, frames: &[StackFrame]) -> String {
    let mut text = header.to_string();
    for frame in frames {
        text.push('\n');
        text.push_str(&frame.to_stack_line());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(name: &str, url: &str, line: u32, column: u32, info: bool) -> StackFrame {
        StackFrame::resolved(Location {
            function_name: name.to_string(),
            source_url: url.to_string(),
            line,
            column,
            has_line_and_column_info: info,
            construct_back_offset: 0,
        })
    }

    #[test]
    fn json_fields_follow_the_bun_measure() {
        assert_eq!(frame("foo", "a.js", 3, 27, true).call_site_json_fields(), ("a.js".to_string(), 3, 26, "foo".to_string()));
        assert_eq!(StackFrame::native_eval().call_site_json_fields(), ("[native code]".to_string(), 0, -1, "eval".to_string()));
        assert_eq!(
            frame("map", "x.js", 5, 7, true).with_builtin_location().call_site_json_fields(),
            ("[unknown]".to_string(), 1, 10, "map".to_string())
        );
    }

    #[test]
    fn stack_line_follows_the_bun_format() {
        assert_eq!(frame("f", "a.js", 3, 9, true).to_stack_line(), "    at f (a.js:3:9)");
        assert_eq!(frame("", "a.js", 3, 9, true).to_stack_line(), "    at <anonymous> (a.js:3:9)");
        assert_eq!(frame("global code", "a.js", 3, 9, true).to_stack_line(), "    at a.js:3:9");
        assert_eq!(frame("eval code", "a.js", 3, 9, true).to_stack_line(), "    at <anonymous> (a.js:3:9)");
        assert_eq!(StackFrame::native_eval().to_stack_line(), "    at eval (unknown)");
        assert_eq!(StackFrame::native_function("sort".to_string(), 0, JSValue::undefined()).to_stack_line(), "    at sort (unknown)");
        assert_eq!(frame("map", "x.js", 5, 7, true).with_builtin_location().to_stack_line(), "    at map (native:1:11)");
        assert_eq!(frame("", "a.js", 3, 1, true).to_stack_line(), "    at <anonymous> (a.js:3)");
        assert_eq!(frame("f", "", 3, 9, true).to_stack_line(), "    at f");
        assert_eq!(frame("f", "a.js", 3, 9, false).to_stack_line(), "    at f (a.js)");
        let mut constructor = frame("Foo", "a.js", 3, 9, true);
        constructor.is_constructor = true;
        assert_eq!(constructor.to_stack_line(), "    at new Foo (a.js:3:9)");
    }

    #[test]
    fn call_site_function_name_hides_program_frames() {
        assert_eq!(frame("f", "a.js", 1, 1, true).call_site_function_name(), Some("f"));
        assert_eq!(frame("", "a.js", 1, 1, true).call_site_function_name(), None);
        assert_eq!(frame("global code", "a.js", 1, 1, true).call_site_function_name(), None);
        assert_eq!(frame("eval code", "a.js", 1, 1, true).call_site_function_name(), None);
    }

    #[test]
    fn format_joins_header_and_lines() {
        // Medido no bun 1.4.2: a coluna 1 é omitida (`at h (a.js:3)`), as demais ficam.
        let frames = [frame("g", "a.js", 1, 1, true), frame("global code", "a.js", 5, 2, true)];
        assert_eq!(
            format_stack_trace("Error: boom", &frames),
            "Error: boom\n    at g (a.js:1)\n    at a.js:5:2"
        );
        assert_eq!(format_stack_trace("Error", &[]), "Error");
    }
}
