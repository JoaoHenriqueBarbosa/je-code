//! Porte parcial de `src/vs/workbench/contrib/output/common/outputLinkComputer.ts`: a detecção de links para caminhos
//! de arquivo (com linha e coluna) numa linha do painel de saída (`OutputLinkComputer`).
//!
//! Notas de tradução:
//! - **Fora do porte.** O `OutputLinkComputer` do original é um manipulador de requisições do web worker
//!   (`IWebWorkerServerRequestHandler`) ligado ao `WorkerTextModelSyncServer`, de onde `$computeLinks(uri)` lê o texto do
//!   modelo. O porte guarda os padrões por pasta e calcula os links sobre o texto que o chamador entrega
//!   ([`OutputLinkComputer::compute_links`]); a busca do modelo pela URI é de quem sincroniza os modelos. A função `create`
//!   (a fábrica do worker) não existe.
//! - **`IResourceCreator`.** `toResource` devolve `URI | null`; o `try/catch` em volta da chamada (um URI inválido não perde os
//!   outros links) é o [`errors::catch`] sobre ela.
//! - **Texto.** As colunas e o `indexOf` contam unidades UTF-16, então a linha é `&[u16]` e as regexes são [`JsRegExp`] (com
//!   `lastIndex`, como a global do original).
//! - **`return` dentro do `forEach`.** Um link repetido encerra o laço daquele padrão (o `return` do callback), não os demais
//!   padrões.
//! - **`isWindows`.** É a plataforma do hospedeiro.

use je_base::errors;
use je_base::js_regexp::JsRegExp;
use je_base::js_string::JsString;
use je_base::platform::Platform;
use je_base::js_string::index_of;
use je_base::strings::{escape_reg_exp_characters, rtrim, split_lines};
use je_base::uri::URI;
use je_editor::languages::{ILink, LinkUrl};
use je_editor::range::Range;
use je_platform::resources::{fs_path, join_path};

/// `IResourceCreator`.
pub trait IResourceCreator {
    fn to_resource(&self, folder_relative_path: &str) -> Option<URI>;
}

/// O `IResourceCreator` de uma pasta do workspace: `resources.joinPath(folderUri, folderRelativePath)`.
struct FolderResourceCreator {
    folder_uri: URI,
}

impl IResourceCreator for FolderResourceCreator {
    fn to_resource(&self, folder_relative_path: &str) -> Option<URI> {
        Some(join_path(&self.folder_uri, &[folder_relative_path]))
    }
}

/// `OutputLinkComputer`.
#[derive(Default)]
pub struct OutputLinkComputer {
    /// `patterns`: os padrões por pasta do workspace, na ordem de registro.
    patterns: Vec<(URI, Vec<JsRegExp>)>,
}

impl OutputLinkComputer {
    pub fn new() -> Self {
        Self::default()
    }

    /// `$setWorkspaceFolders`.
    pub fn set_workspace_folders(&mut self, workspace_folders: Vec<String>) {
        self.compute_patterns(workspace_folders);
    }

    fn compute_patterns(&mut self, mut workspace_folders: Vec<String>) {
        // Produce patterns for each workspace root we are configured with
        // This means that we will be able to detect links for paths that
        // contain any of the workspace roots as segments.
        // longest paths first (for https://github.com/microsoft/vscode/issues/88121)
        workspace_folders.sort_by(|resource_str_a, resource_str_b| resource_str_b.encode_utf16().count().cmp(&resource_str_a.encode_utf16().count()));

        for resource_str in workspace_folders {
            let workspace_folder = URI::parse(resource_str.as_str(), false);
            let patterns = Self::create_patterns(&workspace_folder);
            self.patterns.push((workspace_folder, patterns));
        }
    }

    /// `$computeLinks(uri)` sobre o texto do modelo (`model.getValue()`).
    pub fn compute_links(&self, text: &JsString) -> Vec<ILink> {
        let mut links: Vec<ILink> = Vec::new();
        let lines = split_lines(text.units());

        // For each workspace root patterns
        for (folder_uri, folder_patterns) in &self.patterns {
            let resource_creator = FolderResourceCreator { folder_uri: folder_uri.clone() };

            for (i, line) in lines.iter().enumerate() {
                links.extend(Self::detect_links(line, i as i32 + 1, folder_patterns, &resource_creator));
            }
        }

        links
    }

    pub fn create_patterns(workspace_folder: &URI) -> Vec<JsRegExp> {
        let mut patterns: Vec<JsRegExp> = Vec::new();

        let is_file_scheme = workspace_folder.scheme().to_string_lossy() == "file";
        let workspace_folder_path = if is_file_scheme { fs_path(workspace_folder) } else { workspace_folder.path().to_string_lossy() };
        let mut workspace_folder_variants = vec![workspace_folder_path.clone()];
        if Platform::detect().is_windows && is_file_scheme {
            workspace_folder_variants.push(je_base::extpath::to_slashes(&workspace_folder_path));
        }

        for workspace_folder_variant in workspace_folder_variants {
            let valid_path_character_pattern = r#"[^\s\(\):<>'"]"#;
            let valid_path_character_or_space_pattern = format!("(?:{valid_path_character_pattern}| {valid_path_character_pattern})");
            let path_pattern = format!(r"{valid_path_character_or_space_pattern}+\.{valid_path_character_pattern}+");
            let strict_path_pattern = format!("{valid_path_character_pattern}+");
            let folder_pattern = escape_reg_exp_characters(&workspace_folder_variant.encode_utf16().collect::<Vec<u16>>()).to_string_lossy();

            let regexes = [
                // Example: /workspaces/express/server.js on line 8, column 13
                format!(r"{folder_pattern}({path_pattern}) on line ((\d+)(, column (\d+))?)"),
                // Example: /workspaces/express/server.js:line 8, column 13
                format!(r"{folder_pattern}({path_pattern}):line ((\d+)(, column (\d+))?)"),
                // Example: /workspaces/mankala/Features.ts(45): error
                // Example: /workspaces/mankala/Features.ts (45): error
                // Example: /workspaces/mankala/Features.ts(45,18): error
                // Example: /workspaces/mankala/Features.ts (45,18): error
                // Example: /workspaces/mankala/Features Special.ts (45,18): error
                format!(r"{folder_pattern}({path_pattern})(\s?\((\d+)(,(\d+))?)\)"),
                // Example: at /workspaces/mankala/Game.ts
                // Example: at /workspaces/mankala/Game.ts:336
                // Example: at /workspaces/mankala/Game.ts:336:9
                format!(r"{folder_pattern}({strict_path_pattern})(:(\d+))?(:(\d+))?"),
            ];
            for regex in regexes {
                patterns.push(JsRegExp::from_str(&regex, "gi").expect("o padrão de link de saída é uma regex válida"));
            }
        }

        patterns
    }

    /// Detect links. Made static to allow for tests.
    pub fn detect_links(line: &[u16], line_index: i32, patterns: &[JsRegExp], resource_creator: &dyn IResourceCreator) -> Vec<ILink> {
        let mut links: Vec<ILink> = Vec::new();
        let dot = [u16::from(b'.')];

        for pattern in patterns {
            pattern.set_last_index(0); // the holy grail of software development

            let mut offset: i64 = 0;
            while let Some(found) = pattern.exec(line) {
                let group = |n: usize| found.captures.get(n).and_then(|range| range.clone()).map(|range| &line[range]).filter(|text| !text.is_empty());

                // Convert the relative path information to a resource that we can use in links
                // remove trailing "." that likely indicate end of sentence
                let folder_relative_path: JsString = JsString::from_units(
                    rtrim(group(1).unwrap_or_default(), &dot).iter().map(|&unit| if unit == u16::from(b'\\') { u16::from(b'/') } else { unit }).collect(),
                );
                let relative_path = folder_relative_path.to_string_lossy();
                let mut resource_string: Option<JsString> = None;
                match errors::catch(|| resource_creator.to_resource(&relative_path)) {
                    Ok(resource) => {
                        if let Some(resource) = resource {
                            resource_string = Some(resource.to_string(false));
                        }
                    }
                    Err(_) => continue, // we might find an invalid URI and then we dont want to loose all other links
                }

                // Append line/col information to URI if matching
                if let Some(line_number) = group(3) {
                    let mut text = resource_string.take().unwrap_or_else(|| JsString::from("undefined"));
                    text.push_unit(u16::from(b'#'));
                    text.push_units(line_number);
                    if let Some(column_number) = group(5) {
                        text.push_unit(u16::from(b','));
                        text.push_units(column_number);
                    }
                    resource_string = Some(text);
                }

                // remove trailing "." that likely indicate end of sentence
                let full_match = rtrim(group(0).unwrap_or_default(), &dot);

                let index = index_of(line, full_match, offset.max(0) as usize).map_or(-1, |index| index as i64);
                offset = index + full_match.len() as i64;

                let link_range = Range {
                    start_column: (index + 1) as i32,
                    start_line_number: line_index,
                    end_column: (index + 1 + full_match.len() as i64) as i32,
                    end_line_number: line_index,
                };

                if links.iter().any(|link| Range::are_intersecting_or_touching(&link.range, &link_range)) {
                    break; // Do not detect duplicate links
                }

                links.push(ILink { range: link_range, url: resource_string.map(LinkUrl::Text), tooltip: None, cache_id: None });
            }
        }

        links
    }
}
