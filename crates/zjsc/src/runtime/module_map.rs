//! Tradução de `runtime/ModuleMap.h`.
//!
//! `ModuleMapKey` é `std::pair<UniquedStringImpl*, ScriptFetchParameters::Type>`: o ponteiro vira
//! `Option<UniquedKey>` (comparado e espalhado por endereço, nulo possível), então o `ModuleMapHash`
//! do C++ é o `Hash` derivado da tupla.

use std::collections::HashMap;

use crate::runtime::script_fetch_parameters::ScriptFetchParametersType;
use crate::wtf::text::string_impl::UniquedKey;

/// `ModuleMapKey`.
pub type ModuleMapKey = (Option<UniquedKey>, ScriptFetchParametersType);

/// `ModuleMap<T>`.
pub type ModuleMap<T> = HashMap<ModuleMapKey, T>;

/// `ResolutionMapKey`: referrer e specifier.
pub type ResolutionMapKey = (Option<UniquedKey>, Option<UniquedKey>);

/// `ResolutionMap<T>`.
pub type ResolutionMap<T> = HashMap<ResolutionMapKey, T>;
