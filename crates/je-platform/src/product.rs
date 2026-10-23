//! O pedaço de `src/vs/platform/product/common/productService.ts` (`IProductService`) de que o ambiente e
//! os caminhos de dados precisam: os nomes que escolhem as pastas do usuário.
//!
//! O je-code lê e escreve os MESMOS diretórios do VS Code estável (retrocompatibilidade com o perfil, as
//! extensões e os temas do usuário), então o padrão é o `product.json` do VS Code estável 1.143.1:
//! `nameShort` `Code`, `dataFolderName` `.vscode` e `sharedDataFolderName` `.vscode-shared`.

/// `IProductService`, reduzido aos campos que os serviços portados leem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductService {
    /// `nameShort`: o nome da pasta de dados do usuário (`~/.config/Code`).
    pub name_short: String,
    /// `dataFolderName`: a pasta no home com extensões e `argv.json` (`~/.vscode`).
    pub data_folder_name: String,
    /// `sharedDataFolderName`: a pasta de dados compartilhados entre aplicativos (`~/.vscode-shared`).
    pub shared_data_folder_name: String,
}

impl Default for ProductService {
    fn default() -> Self {
        Self { name_short: "Code".to_owned(), data_folder_name: ".vscode".to_owned(), shared_data_folder_name: ".vscode-shared".to_owned() }
    }
}
