//! Porte de `src/vs/base/common/resources.ts`: `IExtUri`/`ExtUri` (identidade e aritmética de caminho
//! de URIs), `DataUri`, `distinctParents` e `toLocalResource`.
//!
//! O original exporta singletons (`extUri`, `extUriBiasedIgnorePathCase`, `extUriIgnorePathCase`) e
//! funções soltas ligadas a `extUri` (`isEqual`, `joinPath`, `basename`...), que leem `isWindows` e
//! `isLinux` e o módulo `path` da plataforma. Aqui o [`ExtUri`] carrega o [`Environment`] (plataforma e
//! diretório atual) e as três instâncias saem de construtores ([`ExtUri::unbiased`],
//! [`ExtUri::biased_ignore_path_case`], [`ExtUri::ignore_path_case`]); as funções soltas são
//! métodos da instância não enviesada.
//!
//! As operações de caminho trabalham em `String` (via [`JsString::to_string_lossy`]) porque o módulo
//! `path` é em `&str`: um surrogate isolado no caminho do URI vira U+FFFD ao passar por elas.

use std::rc::Rc;

use crate::extpath;
use crate::js_string::substring;
use crate::map::OrderedMap;
use crate::network::schemas;
use crate::path::{Environment, PathModule, Platform};
use crate::strings::{compare, equals_ignore_case};
use crate::uri::{uri_to_fs_path, Field, UriChange, URI};
use crate::{char_code as cc, JsString};

/// `originalFSPath`: o `fsPath` sem normalizar a caixa da letra da unidade.
pub fn original_fs_path(uri: &URI, platform: Platform) -> JsString {
    uri_to_fs_path(uri, true, platform)
}

fn change_path(path: impl Into<JsString>) -> UriChange {
    UriChange { path: Field::Set(path.into()), ..UriChange::default() }
}

/// Utilitário de URIs (`IExtUri`): identidade, comparação e aritmética de caminho.
#[derive(Clone)]
pub struct ExtUri {
    env: Rc<dyn Environment>,
    ignore_path_casing: Rc<dyn Fn(&URI) -> bool>,
}

impl ExtUri {
    pub fn new(env: Rc<dyn Environment>, ignore_path_casing: impl Fn(&URI) -> bool + 'static) -> Self {
        Self { env, ignore_path_casing: Rc::new(ignore_path_casing) }
    }

    /// `extUri`: unbiased utility that takes uris "as they are". This means it can be interchanged
    /// with `uri.toString()` usages.
    pub fn unbiased(env: Rc<dyn Environment>) -> Self {
        Self::new(env, |_| false)
    }

    /// `extUriBiasedIgnorePathCase`: BIASED utility that _mostly_ ignores the case of uri paths.
    /// `is_linux` é o `platform.isLinux` do original.
    pub fn biased_ignore_path_case(env: Rc<dyn Environment>, is_linux: bool) -> Self {
        // A file scheme resource is in the same platform as code, so ignore case for non linux platforms.
        // Resource can be from another platform. Lowering the case as an hack. Should come from File system provider.
        Self::new(env, move |uri| if uri.scheme() == schemas::FILE { !is_linux } else { true })
    }

    /// `extUriIgnorePathCase`: BIASED utility that always ignores the casing of uri paths.
    pub fn ignore_path_case(env: Rc<dyn Environment>) -> Self {
        Self::new(env, |_| true)
    }

    fn platform(&self) -> Platform {
        self.env.platform()
    }

    fn native(&self) -> PathModule<'_> {
        PathModule::native(self.env.as_ref())
    }

    fn posix(&self) -> PathModule<'_> {
        PathModule::posix(self.env.as_ref())
    }

    fn file_uri(&self, path: &str) -> URI {
        URI::file(path, self.platform())
    }

    fn fs_path_string(&self, uri: &URI) -> String {
        original_fs_path(uri, self.platform()).to_string_lossy()
    }

    // --- identity

    /// Compares two uris.
    pub fn compare(&self, uri1: &URI, uri2: &URI, ignore_fragment: bool) -> i32 {
        if URI::ptr_eq(uri1, uri2) {
            return 0;
        }
        compare(
            self.get_comparison_key(uri1, ignore_fragment).units(),
            self.get_comparison_key(uri2, ignore_fragment).units(),
        )
    }

    /// Tests whether two uris are equal.
    pub fn is_equal(&self, uri1: Option<&URI>, uri2: Option<&URI>, ignore_fragment: bool) -> bool {
        match (uri1, uri2) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                URI::ptr_eq(a, b) || self.get_comparison_key(a, ignore_fragment) == self.get_comparison_key(b, ignore_fragment)
            }
            _ => false,
        }
    }

    /// Creates a key from a resource URI to be used to resource comparison and for resource maps.
    pub fn get_comparison_key(&self, uri: &URI, ignore_fragment: bool) -> JsString {
        uri.with(&UriChange {
            path: if (self.ignore_path_casing)(uri) { Field::Set(uri.path().to_lower_case()) } else { Field::Keep },
            fragment: if ignore_fragment { Field::Clear } else { Field::Keep },
            ..UriChange::default()
        })
        .to_string(false)
    }

    /// Whether the casing of the path-component of the uri should be ignored.
    pub fn ignore_path_casing(&self, uri: &URI) -> bool {
        (self.ignore_path_casing)(uri)
    }

    /// Tests whether a `candidate` URI is a parent or equal of a given `base` URI. Non-file URIs must
    /// be contained with both POSIX and Windows path semantics by default.
    pub fn is_equal_or_parent(&self, base: &URI, parent_candidate: &URI, ignore_fragment: bool, force_posix_semantics: bool) -> bool {
        if base.scheme() != parent_candidate.scheme() {
            return false;
        }
        let same_query_and_fragment =
            base.query() == parent_candidate.query() && (ignore_fragment || base.fragment() == parent_candidate.fragment());
        if base.scheme() == schemas::FILE {
            return extpath::is_equal_or_parent(
                &self.fs_path_string(base),
                &self.fs_path_string(parent_candidate),
                self.ignore_path_casing(base),
                false,
                self.env.as_ref(),
            ) && same_query_and_fragment;
        }
        if self.is_equal_authority(base.authority(), parent_candidate.authority()) {
            let ignore_case = self.ignore_path_casing(base);
            let (base_path, parent_path) = (base.path().to_string_lossy(), parent_candidate.path().to_string_lossy());
            if !extpath::is_equal_or_parent(&base_path, &parent_path, ignore_case, true, self.env.as_ref()) {
                return false;
            }
            // Non-file URIs can still be backed by a Windows filesystem.
            if !force_posix_semantics && (base_path.contains('\\') || parent_path.contains('\\')) {
                let win32 = PathModule::win32(self.env.as_ref());
                let base_path = extpath::to_slashes(&win32.normalize(&self.fs_path_string(base)));
                let parent_path = extpath::to_slashes(&win32.normalize(&self.fs_path_string(parent_candidate)));
                if !extpath::is_equal_or_parent(&base_path, &parent_path, ignore_case, true, self.env.as_ref()) {
                    return false;
                }
            }
            return same_query_and_fragment;
        }
        false
    }

    // --- path math

    /// Join a URI path with path fragments and normalizes the resulting path.
    pub fn join_path(&self, resource: &URI, path_fragments: &[&str]) -> URI {
        URI::join_path(resource, path_fragments, self.platform())
    }

    pub fn basename_or_authority(&self, resource: &URI) -> String {
        let basename = self.basename(resource, None);
        if basename.is_empty() { resource.authority().to_string_lossy() } else { basename }
    }

    /// Returns the basename of the path component of an uri.
    pub fn basename(&self, resource: &URI, suffix: Option<&str>) -> String {
        self.posix().basename(&resource.path().to_string_lossy(), suffix)
    }

    /// Returns the extension of the path component of an uri.
    pub fn extname(&self, resource: &URI) -> String {
        self.posix().extname(&resource.path().to_string_lossy())
    }

    /// Return a URI representing the directory of a URI path.
    pub fn dirname(&self, resource: &URI) -> URI {
        if resource.path().is_empty() {
            return resource.clone();
        }
        let dirname: JsString = if resource.scheme() == schemas::FILE {
            self.file_uri(&self.native().dirname(&self.fs_path_string(resource))).path().clone()
        } else {
            let mut dirname = self.posix().dirname(&resource.path().to_string_lossy());
            if !resource.authority().is_empty() && !dirname.is_empty() && !dirname.starts_with('/') {
                eprintln!("dirname(\"{}\") resulted in a relative path", resource.to_string(false));
                // If a URI contains an authority component, then the path component must either be
                // empty or begin with a CharCode.Slash ("/") character
                dirname = "/".to_string();
            }
            JsString::from(dirname)
        };
        resource.with(&change_path(dirname))
    }

    /// Normalizes the path part of a URI: Resolves `.` and `..` elements with directory names.
    pub fn normalize_path(&self, resource: &URI) -> URI {
        if resource.path().is_empty() {
            return resource.clone();
        }
        let normalized_path: JsString = if resource.scheme() == schemas::FILE {
            self.file_uri(&self.native().normalize(&self.fs_path_string(resource))).path().clone()
        } else {
            JsString::from(self.posix().normalize(&resource.path().to_string_lossy()))
        };
        resource.with(&change_path(normalized_path))
    }

    pub fn relative_path(&self, from: &URI, to: &URI) -> Option<String> {
        if from.scheme() != to.scheme() || !self.is_equal_authority(from.authority(), to.authority()) {
            return None;
        }
        if from.scheme() == schemas::FILE {
            let relative_path = self.native().relative(&self.fs_path_string(from), &self.fs_path_string(to));
            return Some(if self.platform() == Platform::Win32 { extpath::to_slashes(&relative_path) } else { relative_path });
        }
        let slash = JsString::from("/");
        let mut from_path = if from.path().is_empty() { slash.clone() } else { from.path().clone() };
        let to_path = if to.path().is_empty() { slash } else { to.path().clone() };
        if self.ignore_path_casing(from) {
            // make casing of fromPath match toPath
            let lower = |unit: u16| JsString::from_units(vec![unit]).to_lower_case();
            let len = from_path.len().min(to_path.len());
            let mut i = 0;
            while i < len {
                if from_path.units()[i] != to_path.units()[i] && lower(from_path.units()[i]) != lower(to_path.units()[i]) {
                    break;
                }
                i += 1;
            }
            let mut aligned = to_path.units()[..i].to_vec();
            aligned.extend_from_slice(&from_path.units()[i..]);
            from_path = JsString::from_units(aligned);
        }
        Some(self.posix().relative(&from_path.to_string_lossy(), &to_path.to_string_lossy()))
    }

    /// Resolves an absolute or relative path against a base URI. The path can be relative or absolute
    /// posix or a Windows path.
    pub fn resolve_path(&self, base: &URI, path: &str) -> URI {
        if base.scheme() == schemas::FILE {
            let new_uri = self.file_uri(&self.native().resolve(&[&self.fs_path_string(base), path]));
            return base.with(&UriChange {
                authority: Field::Set(new_uri.authority().clone()),
                path: Field::Set(new_uri.path().clone()),
                ..UriChange::default()
            });
        }
        let path = extpath::to_posix_path(path); // we allow path to be a windows path
        base.with(&change_path(self.posix().resolve(&[&base.path().to_string_lossy(), &path])))
    }

    // --- misc

    /// Returns true if the URI path is absolute.
    pub fn is_absolute_path(&self, resource: &URI) -> bool {
        resource.path().units().first() == Some(&cc::Slash)
    }

    /// Tests whether the two authorities are the same.
    pub fn is_equal_authority(&self, a1: &JsString, a2: &JsString) -> bool {
        a1 == a2 || equals_ignore_case(a1.units(), a2.units())
    }

    /// Returns true if the URI path has a trailing path separator. `sep` é o separador da plataforma
    /// quando omitido.
    pub fn has_trailing_path_separator(&self, resource: &URI, sep: Option<&str>) -> bool {
        if resource.scheme() == schemas::FILE {
            let sep = JsString::from(sep.unwrap_or(self.native().sep()));
            let fsp = original_fs_path(resource, self.platform());
            fsp.len() > JsString::from(extpath::get_root(&fsp.to_string_lossy(), "/")).len()
                && fsp.units().last() == sep.units().first()
        } else {
            let p = resource.path();
            // ignore the slash at offset 0
            p.len() > 1 && p.units().last() == Some(&cc::Slash) && !is_drive_root_fs_path(&resource.fs_path(self.platform()))
        }
    }

    /// Removes a trailing path separator, if there's one. Important: Doesn't remove the first slash,
    /// it would make the URI invalid.
    pub fn remove_trailing_path_separator(&self, resource: &URI, sep: Option<&str>) -> URI {
        // Make sure that the path isn't a drive letter. A trailing separator there is not removable.
        if self.has_trailing_path_separator(resource, sep) {
            let path = resource.path();
            return resource.with(&change_path(path.slice(0, path.len() - 1)));
        }
        resource.clone()
    }

    /// Adds a trailing path separator to the URI if there isn't one already. For example, c:\ would be
    /// unchanged, but c:\users would become c:\users\.
    pub fn add_trailing_path_separator(&self, resource: &URI, sep: Option<&str>) -> URI {
        let mut sep = sep.map(str::to_string);
        let is_root_sep = if resource.scheme() == schemas::FILE {
            let sep = JsString::from(sep.as_deref().unwrap_or(self.native().sep()));
            let fsp = original_fs_path(resource, self.platform());
            fsp.len() == JsString::from(extpath::get_root(&fsp.to_string_lossy(), "/")).len()
                && fsp.units().last() == sep.units().first()
        } else {
            sep = Some("/".to_string());
            let p = resource.path();
            p.len() == 1 && p.units().last() == Some(&cc::Slash)
        };
        if !is_root_sep && !self.has_trailing_path_separator(resource, sep.as_deref()) {
            let mut path = resource.path().clone();
            path.push_unit(cc::Slash);
            return resource.with(&change_path(path));
        }
        resource.clone()
    }
}

/// `/^[a-zA-Z]:(\/$|\\$)/`.
fn is_drive_root_fs_path(fs_path: &JsString) -> bool {
    let units = fs_path.units();
    units.len() == 3
        && extpath::is_windows_drive_letter(i32::from(units[0]))
        && units[1] == cc::Colon
        && (units[2] == cc::Slash || units[2] == cc::Backslash)
}

/// `distinctParents`: os itens que não são filhos (nem iguais) de nenhum outro item da lista.
pub fn distinct_parents<'a, T>(items: &'a [T], resource_accessor: impl Fn(&T) -> URI, ext_uri: &ExtUri) -> Vec<&'a T> {
    let mut distinct_parents = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let candidate_resource = resource_accessor(item);
        let is_child = items.iter().enumerate().any(|(index, other_item)| {
            index != i && ext_uri.is_equal_or_parent(&candidate_resource, &resource_accessor(other_item), false, false)
        });
        if !is_child {
            distinct_parents.push(item);
        }
    }

    distinct_parents
}

/// Data URI related helpers (`DataUri`).
pub mod data_uri {
    use super::*;

    pub const META_DATA_LABEL: &str = "label";
    pub const META_DATA_DESCRIPTION: &str = "description";
    pub const META_DATA_SIZE: &str = "size";
    pub const META_DATA_MIME: &str = "mime";

    pub fn parse_meta_data(data_uri: &URI) -> OrderedMap<String, String> {
        let mut metadata = OrderedMap::new();
        let path = data_uri.path().units();
        let index_of = |unit: u16| path.iter().position(|&u| u == unit).map_or(-1, |p| p as i64);
        let last_index_of = |unit: u16| path.iter().rposition(|&u| u == unit).map_or(-1, |p| p as i64);

        // Given a URI of:  data:image/png;size:2313;label:SomeLabel;description:SomeDescription;base64,77+9UE5...
        // the metadata is: size:2313;label:SomeLabel;description:SomeDescription
        let meta = JsString::from(substring(path, index_of(cc::Semicolon) + 1, last_index_of(cc::Semicolon))).to_string_lossy();
        for property in meta.split(';') {
            let mut parts = property.split(':');
            if let (Some(key), Some(value)) = (parts.next(), parts.next())
                && !key.is_empty()
                && !value.is_empty()
            {
                metadata.insert(key.to_string(), value.to_string());
            }
        }

        // Given a URI of:  data:image/png;size:2313;label:SomeLabel;description:SomeDescription;base64,77+9UE5...
        // the mime is: image/png
        let mime = JsString::from(substring(path, 0, index_of(cc::Semicolon))).to_string_lossy();
        if !mime.is_empty() {
            metadata.insert(META_DATA_MIME.to_string(), mime);
        }

        metadata
    }
}

pub fn to_local_resource(resource: &URI, authority: Option<&str>, local_scheme: &str) -> URI {
    match authority.filter(|a| !a.is_empty()) {
        Some(authority) => {
            let mut path = resource.path().clone();
            if !path.is_empty() && path.units()[0] != cc::Slash {
                let mut with_slash = vec![cc::Slash];
                with_slash.extend_from_slice(path.units());
                path = JsString::from_units(with_slash);
            }

            resource.with(&UriChange {
                scheme: local_scheme.into(),
                authority: authority.into(),
                path: Field::Set(path),
                ..UriChange::default()
            })
        }
        None => resource.with(&UriChange { scheme: local_scheme.into(), ..UriChange::default() }),
    }
}
