//! Porte de `src/vs/workbench/services/label/common/resourceLabelTemplate.ts`: o modelo de `home` de um formatador de
//! rótulo, um caminho em que segmentos inteiros podem ser parâmetros (`/sessions/${sessionId}`).
//!
//! Notas de tradução:
//! - `templateParameterRegex` (`/^\$\{(?<name>[a-zA-Z_][\w]*)\}$/`) é a função [`parameter_name`], porque o segmento
//!   inteiro precisa ser `${nome}` e `\w` é ASCII.
//! - `ReadonlyMap<string, string>` é um `HashMap`, e `throw new Error(...)` é [`errors::throw`].
//! - O método `match` do original é [`ResourceLabelTemplate::match_resource`] (`match` é palavra reservada).

use std::collections::HashMap;

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::resources::ExtUri;
use je_base::uri::{Field, URI, UriChange};

/// `TemplateSegment`.
enum TemplateSegment {
    Literal(String),
    Parameter(String),
}

/// `IResourceLabelTemplateMatch`.
pub struct ResourceLabelTemplateMatch {
    pub home: URI,
    pub parameters: HashMap<String, String>,
}

/// O nome de `${nome}` quando o segmento inteiro é um parâmetro (`[a-zA-Z_][\w]*`).
fn parameter_name(segment: &str) -> Option<&str> {
    let name = segment.strip_prefix("${")?.strip_suffix('}')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    let valid = (first.is_ascii_alphabetic() || first == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    valid.then_some(name)
}

/// `resource.with({ path, query: null, fragment: null })`.
pub(crate) fn with_path_only(resource: &URI, path: impl Into<JsString>) -> URI {
    resource.with(&UriChange { path: Field::Set(path.into()), query: Field::Clear, fragment: Field::Clear, ..UriChange::default() })
}

/// `ResourceLabelTemplate`.
pub struct ResourceLabelTemplate {
    home: URI,
    segments: Vec<TemplateSegment>,
    is_root: bool,
}

impl ResourceLabelTemplate {
    pub fn new(home: &URI) -> Self {
        let path = home.path().to_string_lossy();
        let home_path = if path.len() > 1 { path.trim_end_matches('/') } else { path.as_str() };
        let is_root = home_path.is_empty() || home_path == "/";
        let mut parameter_names: Vec<&str> = Vec::new();
        let mut segments = Vec::new();
        if !is_root {
            for segment in home_path.split('/') {
                if let Some(parameter) = parameter_name(segment) {
                    if parameter_names.contains(&parameter) {
                        errors::throw(Error::new(format!("Duplicate resource label home template parameter: {parameter}")));
                    }
                    parameter_names.push(parameter);
                    segments.push(TemplateSegment::Parameter(parameter.to_string()));
                    continue;
                }
                if segment.contains("${") {
                    errors::throw(Error::new(format!("Resource label home template parameters must occupy an entire path segment: {segment}")));
                }
                segments.push(TemplateSegment::Literal(segment.to_string()));
            }
        }

        Self { home: home.clone(), segments, is_root }
    }

    pub fn match_resource(&self, resource: &URI, ext_uri: &ExtUri) -> Option<ResourceLabelTemplateMatch> {
        if self.home.scheme() != resource.scheme() || (!self.home.authority().is_empty() && !ext_uri.is_equal_authority(self.home.authority(), resource.authority())) {
            return None;
        }
        if self.is_root {
            return Some(ResourceLabelTemplateMatch { home: with_path_only(resource, self.home.path().clone()), parameters: HashMap::new() });
        }

        let resource_path = resource.path().to_string_lossy();
        let resource_segments: Vec<&str> = resource_path.split('/').collect();
        if resource_segments.len() < self.segments.len() {
            return None;
        }
        let mut parameters = HashMap::new();
        for (template_segment, resource_segment) in self.segments.iter().zip(&resource_segments) {
            match template_segment {
                TemplateSegment::Parameter(name) => {
                    if *resource_segment == "." || *resource_segment == ".." {
                        return None;
                    }
                    parameters.insert(name.clone(), resource_segment.to_string());
                }
                TemplateSegment::Literal(value) => {
                    let resource_segment_uri = with_path_only(resource, format!("/{resource_segment}"));
                    let template_segment_uri = with_path_only(resource, format!("/{value}"));
                    if !ext_uri.is_equal(Some(&resource_segment_uri), Some(&template_segment_uri), false) {
                        return None;
                    }
                }
            }
        }
        Some(ResourceLabelTemplateMatch { home: with_path_only(resource, resource_segments[..self.segments.len()].join("/")), parameters })
    }
}
