//! Porte de `runtime/RegExpGlobalData.{h,cpp}`, `RegExpGlobalDataInlines.h` e, de
//! `RegExpCachedResult.{h,cpp}`, o cache do último casamento que `RegExp.$1`, `RegExp.lastMatch`,
//! `RegExp.leftContext` e companhia leem.
//!
//! DIVERGÊNCIAS (heap ausente, camada 3):
//!
//! - Os `WriteBarrier<>` viram `Option<Rc<..>>`; `visitAggregate`, `vm.writeBarrier` e os `offsetOf*`
//!   (layout para o JIT) somem. O estado mutável fica num `RefCell`, porque o `RegExpGlobalData` mora
//!   no `JSGlobalObject` compartilhado por `Rc`.
//! - `performMatch` com `int** ovector` e `resetResultFromCache` dependem do `m_ovector` da célula
//!   `RegExp` (aqui cada casamento devolve o seu vetor, ver `RegExp::match_ovector`) e não existem.
//! - `RegExpSubstringGlobalAtomCache` (cache de `String.prototype.replace` com átomo global) espera o
//!   `String.prototype` e não existe.
//! - `m_lastRegExp` nulo materializa o `RegExp` vazio de `RegExpCache::ensureEmptyRegExp`: aqui o
//!   `RegExp::create(vm, "(?:)", {})` (do mesmo cache de padrão, a mesma célula sempre).
//! - A exceção de `string->view(globalObject)` não existe (sem ropes).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::runtime::js_array::JSArray;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::{js_empty_string, js_substring, JSStringRef};
use crate::runtime::js_value::JSValue;
use crate::runtime::match_result::{MatchResult, NOT_FOUND};
use crate::runtime::reg_exp::{RegExp, RegExpRef};
use crate::runtime::reg_exp_matches_array::{create_empty_reg_exp_matches_array, create_reg_exp_matches_array};
use crate::wtf::text::string_view::StringView;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::yarr::yarr_flags::FlagSet;

/// `class RegExpCachedResult`: o último casamento, "preguiçoso" (só `m_result`, `m_lastInput` e
/// `m_lastRegExp`) até alguém ler `lastResult`, e depois "reificado" (`m_reified`).
struct RegExpCachedResult {
    result: MatchResult,
    reified: bool,
    one_character_match: bool,
    last_input: Option<JSStringRef>,
    last_reg_exp: Option<RegExpRef>,
    reified_result: Option<JSArray>,
    reified_input: Option<JSStringRef>,
    reified_left_context: Option<JSStringRef>,
    reified_right_context: Option<JSStringRef>,
}

impl RegExpCachedResult {
    /// `RegExpCachedResult()`: `m_result { 0, 0 }`.
    fn new() -> RegExpCachedResult {
        RegExpCachedResult {
            result: MatchResult::new(0, 0),
            reified: false,
            one_character_match: false,
            last_input: None,
            last_reg_exp: None,
            reified_result: None,
            reified_input: None,
            reified_left_context: None,
            reified_right_context: None,
        }
    }

    /// `record(vm, owner, regExp, input, result, oneCharacterMatch)`.
    fn record(&mut self, reg_exp: RegExpRef, input: JSStringRef, result: MatchResult, one_character_match: bool) {
        self.last_reg_exp = Some(reg_exp);
        self.last_input = Some(input);
        self.result = result;
        self.reified = false;
        self.one_character_match = one_character_match;
    }

    /// `input()`.
    fn input(&self) -> Option<JSStringRef> {
        if self.reified { self.reified_input.clone() } else { self.last_input.clone() }
    }

    /// `lastResult(globalObject, owner)`.
    fn last_result(&mut self, global_object: &JSGlobalObject) -> JSArray {
        if !self.reified {
            let vm = global_object.vm();
            self.reified_input = self.last_input.clone();
            let reg_exp = Rc::clone(
                self.last_reg_exp.get_or_insert_with(|| RegExp::create(vm, &WtfString::from_latin1(b"(?:)"), FlagSet::empty())),
            );
            let input = self.last_input.clone().unwrap_or_else(|| js_empty_string(vm));

            let result = if self.result.matched() && self.last_input.is_some() {
                if self.one_character_match {
                    debug_assert!(reg_exp.has_valid_atom());
                    let atom = reg_exp.atom();
                    debug_assert_eq!(atom.length(), 1);
                    let value = input.value();
                    let found = StringView::from(&value).reverse_find_character(atom.code_unit_at(0), u32::MAX);
                    if found != NOT_FOUND {
                        self.result = MatchResult::new(found, found + 1);
                    }
                    self.one_character_match = false;
                }
                create_reg_exp_matches_array(global_object, &input, &reg_exp, self.result.start as u32).map(|(array, _)| array)
            } else {
                None
            };
            let result = result.unwrap_or_else(|| create_empty_reg_exp_matches_array(global_object, &input, &reg_exp));

            self.reified_result = Some(result);
            self.reified_left_context = None;
            self.reified_right_context = None;
            self.reified = true;
        }
        self.reified_result.clone().expect("RegExpCachedResult reificado sem resultado")
    }

    /// `leftContext(globalObject, owner)`.
    fn left_context(&mut self, global_object: &JSGlobalObject) -> JSStringRef {
        self.last_result(global_object);
        let input = self.reified_input_or_empty(global_object);
        let result = self.result;
        self.reified_left_context
            .get_or_insert_with(|| js_substring(global_object.vm(), &input, 0, if result.matched() { result.start as u32 } else { 0 }))
            .clone()
    }

    /// `rightContext(globalObject, owner)`.
    fn right_context(&mut self, global_object: &JSGlobalObject) -> JSStringRef {
        self.last_result(global_object);
        let input = self.reified_input_or_empty(global_object);
        let end = (self.result.end as u32).min(input.length());
        self.reified_right_context
            .get_or_insert_with(|| js_substring(global_object.vm(), &input, end, input.length() - end))
            .clone()
    }

    /// `setInput(globalObject, owner, input)`: reifica tudo antes, senão o `m_reifiedInput` seria
    /// ignorado.
    fn set_input(&mut self, global_object: &JSGlobalObject, input: JSStringRef) {
        self.last_result(global_object);
        self.left_context(global_object);
        self.right_context(global_object);
        debug_assert!(self.reified);
        self.reified_input = Some(input);
    }

    /// `m_reifiedInput.get()` depois da reificação (`jsEmptyString` quando nunca houve entrada, o que
    /// no C++ seria um `nullptr` que o `leftContext` não chega a desreferenciar sem casamento).
    fn reified_input_or_empty(&self, global_object: &JSGlobalObject) -> JSStringRef {
        self.reified_input.clone().unwrap_or_else(|| js_empty_string(global_object.vm()))
    }
}

/// `class RegExpGlobalData`.
pub struct RegExpGlobalData {
    cached_result: RefCell<RegExpCachedResult>,
    /// `m_multiline`.
    multiline: Cell<bool>,
}

impl std::fmt::Debug for RegExpGlobalData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegExpGlobalData").field("multiline", &self.multiline.get()).finish_non_exhaustive()
    }
}

impl Default for RegExpGlobalData {
    fn default() -> RegExpGlobalData {
        RegExpGlobalData::new()
    }
}

impl RegExpGlobalData {
    /// `RegExpGlobalData()`.
    pub fn new() -> RegExpGlobalData {
        RegExpGlobalData { cached_result: RefCell::new(RegExpCachedResult::new()), multiline: Cell::new(false) }
    }

    /// `setMultiline(bool)`.
    pub fn set_multiline(&self, multiline: bool) {
        self.multiline.set(multiline);
    }

    /// `multiline()`.
    pub fn multiline(&self) -> bool {
        self.multiline.get()
    }

    /// `setInput(globalObject, string)`.
    pub fn set_input(&self, global_object: &JSGlobalObject, string: JSStringRef) {
        self.cached_result.borrow_mut().set_input(global_object, string);
    }

    /// `input()`: `None` é o `nullptr` de antes do primeiro casamento.
    pub fn input(&self) -> Option<JSStringRef> {
        self.cached_result.borrow().input()
    }

    /// `getBackref(globalObject, i)`: `$i`, a string vazia quando o grupo não existe ou não participou.
    pub fn get_backref(&self, global_object: &JSGlobalObject, i: u32) -> JSValue {
        let vm = global_object.vm();
        let array = self.cached_result.borrow_mut().last_result(global_object);
        if i < array.length() {
            let result = array.get_by_index(vm, i);
            debug_assert!(result.is_string() || result.is_undefined());
            if !result.is_undefined() {
                return result;
            }
        }
        JSValue::from_js_string(js_empty_string(vm))
    }

    /// `getLastParen(globalObject)`: `RegExp.lastParen` (`$+`).
    pub fn get_last_paren(&self, global_object: &JSGlobalObject) -> JSValue {
        let vm = global_object.vm();
        let array = self.cached_result.borrow_mut().last_result(global_object);
        let length = array.length();
        if length > 1 {
            let result = array.get_by_index(vm, length - 1);
            debug_assert!(result.is_string() || result.is_undefined());
            if !result.is_undefined() {
                return result;
            }
        }
        JSValue::from_js_string(js_empty_string(vm))
    }

    /// `getLeftContext(globalObject)`.
    pub fn get_left_context(&self, global_object: &JSGlobalObject) -> JSValue {
        JSValue::from_js_string(self.cached_result.borrow_mut().left_context(global_object))
    }

    /// `getRightContext(globalObject)`.
    pub fn get_right_context(&self, global_object: &JSGlobalObject) -> JSValue {
        JSValue::from_js_string(self.cached_result.borrow_mut().right_context(global_object))
    }

    /// `lastResult` do `RegExpCachedResult` (`RegExp.lastMatch` é o elemento 0).
    pub fn last_result(&self, global_object: &JSGlobalObject) -> JSArray {
        self.cached_result.borrow_mut().last_result(global_object)
    }

    /// `performMatch(globalObject, regExp, string, input, startOffset)`: casa e, se casou, grava no
    /// cache. É por aqui que `exec`, `test`, `match`, `search` e `replace` passam.
    pub fn perform_match(
        &self,
        _global_object: &JSGlobalObject,
        reg_exp: &RegExpRef,
        string: &JSStringRef,
        start_offset: u32,
    ) -> MatchResult {
        let value = string.value();
        let Some(ovector) = reg_exp.match_ovector(StringView::from(&value), start_offset) else {
            return MatchResult::failed();
        };
        debug_assert!(ovector[1] >= ovector[0]);
        let result = MatchResult::new(ovector[0] as usize, ovector[1] as usize);
        self.record_match(reg_exp, string, result, false);
        result
    }

    /// `recordMatch(vm, owner, regExp, string, result, oneCharacterMatch)`.
    pub fn record_match(&self, reg_exp: &RegExpRef, string: &JSStringRef, result: MatchResult, one_character_match: bool) {
        debug_assert!(result.matched());
        self.cached_result.borrow_mut().record(Rc::clone(reg_exp), Rc::clone(string), result, one_character_match);
    }

    /// `matchResult()`.
    pub fn match_result(&self) -> MatchResult {
        self.cached_result.borrow().result
    }
}
