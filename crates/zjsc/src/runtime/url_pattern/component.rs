//! `URLPatternComponent.cpp` (WebCore do bun 1.4.2): https://urlpattern.spec.whatwg.org/#compile-a-component.

use super::canonical::EncodingCallbackType;
use super::parser::{generate_pattern_string, generate_regex_and_name_list, parse, PartType, URLPatternStringOptions};
use super::{units, Units};
use crate::runtime::reg_exp::{RegExp, RegExpRef};
use crate::runtime::vm::VM;
use crate::wtf::text::string_view::StringView;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::yarr::yarr_flags::{FlagSet, Flags};

/// O valor de um grupo: `None` é o `std::monostate` (o grupo não participou, `undefined` no JS).
pub(crate) type GroupValue = Option<Units>;

/// `URLPatternComponentResult`.
#[derive(Clone, Default)]
pub(crate) struct ComponentResult {
    pub(crate) input: Units,
    pub(crate) groups: Vec<(Units, GroupValue)>,
}

pub(crate) struct URLPatternComponent {
    pattern_string: Units,
    regular_expression: RegExpRef,
    group_name_list: Vec<Units>,
    has_regex_groups_from_part_list: bool,
}

impl URLPatternComponent {
    pub(crate) fn compile(vm: &VM, input: &[u16], kind: EncodingCallbackType, options: &URLPatternStringOptions) -> Result<URLPatternComponent, String> {
        let part_list = parse(input, options, kind)?;
        let (regular_expression_string, name_list) = generate_regex_and_name_list(&part_list, options);
        let mut flags = FlagSet::from(Flags::UnicodeSets);
        if options.ignore_case {
            flags.add(Flags::IgnoreCase);
        }
        let regular_expression = RegExp::create(vm, &WtfString::from_utf16(&regular_expression_string), flags);
        if !regular_expression.is_valid() {
            return Err("Unable to create RegExp object regular expression from provided URLPattern string.".into());
        }
        let pattern_string = generate_pattern_string(&part_list, options);
        let has_regex_groups_from_part_list = part_list.iter().any(|part| part.kind == PartType::Regexp);
        Ok(URLPatternComponent { pattern_string, regular_expression, group_name_list: name_list, has_regex_groups_from_part_list })
    }

    pub(crate) fn pattern_string(&self) -> &Units {
        &self.pattern_string
    }

    pub(crate) fn has_regex_groups_from_part_list(&self) -> bool {
        self.has_regex_groups_from_part_list
    }

    /// https://urlpattern.spec.whatwg.org/#protocol-component-matches-a-special-scheme
    pub(crate) fn match_special_scheme_protocol(&self) -> bool {
        ["ftp", "file", "http", "https", "ws", "wss"].iter().any(|scheme| {
            let scheme = units(scheme);
            self.regular_expression.match_ovector(StringView::from(scheme.as_slice()), 0).is_some()
        })
    }

    /// https://urlpattern.spec.whatwg.org/#create-a-component-match-result (com o "regexp matching").
    pub(crate) fn component_match(&self, input: Units) -> Option<ComponentResult> {
        let ovector = self.regular_expression.match_ovector(StringView::from(input.as_slice()), 0)?;
        let num_subpatterns = self.regular_expression.num_subpatterns() as usize;
        let mut groups = Vec::with_capacity(num_subpatterns);
        for i in 1..=num_subpatterns {
            let (start, end) = (ovector[i * 2], ovector[i * 2 + 1]);
            let value = (start >= 0).then(|| input[start as usize..end as usize].to_vec());
            let name = self.group_name_list.get(i - 1).cloned().unwrap_or_default();
            groups.push((name, value));
        }
        Some(ComponentResult { input, groups })
    }
}
