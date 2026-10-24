//! Porte de `src/vs/editor/contrib/links/browser/getLinks.ts`: a coleta dos links dos provedores (`getLinks`), a lista que
//! os une (`LinksList`) e o link que sabe se resolver (`Link`).
//!
//! Decisões de tradução:
//! - `ILink`/`ILinksList`, do `languages.ts`, são o [`ILink`] e o [`ILinksList`] (o `LinksList` de
//!   `language_features_service`); a lista deste módulo, `LinksList`, é a classe do original.
//! - `Link` é um `Rc` (o `resolve` recursivo do original captura `this`, que muda o `_link` por dentro): o `_link` é um
//!   `RefCell`. `get url(): URI | string | undefined` é [`Link::url`], e a verificação `if (this._link.url)` testa o valor
//!   como o JavaScript testa (a cadeia vazia é falsa).
//! - `LinksList.links: Link[]` é um `RefCell`, porque `dispose()` esvazia a lista (`this.links.length = 0`); [`LinksList::links`]
//!   devolve uma cópia dos `Rc<Link>`. `LinksList.Empty` é [`LinksList::empty`], uma lista nova e vazia a cada chamada.
//! - O `lists[i] = [result, provider]` do original é esparso e o `coalesce(lists)` depois dos `await` descarta as
//!   posições vazias, na ordem dos provedores invertida; o vetor de `Option` faz o mesmo.
//! - `toJSON()` de `Link` fica de fora (só serve à serialização do comando).
//! - O comando `_executeLinkProvider` (`CommandsRegistry.registerCommand`) lê `ILanguageFeaturesService` e `IModelService`
//!   de um `ServicesAccessor`, que o porte não tem: fica de fora.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::{Error, on_unexpected_external_error};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;

use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_features_service::{LinkProvider, LinksList as ILinksList};
use crate::languages::{ILink, LinkUrl};
use crate::range::Range;
use crate::text_model::TextModel;

/// `if (link.url)`: o valor existe e não é uma cadeia vazia.
fn url_is_truthy(url: &LinkUrl) -> bool {
    match url {
        LinkUrl::Text(text) => !text.is_empty(),
        LinkUrl::Uri(_) => true,
    }
}

/// `Link`.
pub struct Link {
    link: RefCell<ILink>,
    provider: Rc<dyn LinkProvider>,
}

impl Link {
    pub fn new(link: ILink, provider: Rc<dyn LinkProvider>) -> Rc<Self> {
        Rc::new(Self { link: RefCell::new(link), provider })
    }

    /// `range`.
    pub fn range(&self) -> Range {
        self.link.borrow().range
    }

    /// `url`.
    pub fn url(&self) -> Option<LinkUrl> {
        self.link.borrow().url.clone()
    }

    /// `tooltip`.
    pub fn tooltip(&self) -> Option<String> {
        self.link.borrow().tooltip.clone()
    }

    /// `resolve(token)`.
    pub fn resolve(self: &Rc<Self>, token: &CancellationToken) -> Promise<LinkUrl> {
        if let Some(url) = self.url().filter(url_is_truthy) {
            return Promise::resolved(url);
        }

        if self.provider.has_resolve_link() {
            let (this, token_for_recursion) = (Rc::clone(self), token.clone());
            let link = self.link.borrow().clone();
            return self.provider.resolve_link(&link, token).then_promise(move |value| {
                if let Some(value) = value {
                    *this.link.borrow_mut() = value;
                }
                if this.link.borrow().url.as_ref().is_some_and(url_is_truthy) {
                    // recurse
                    return this.resolve(&token_for_recursion);
                }

                Promise::rejected(Error::new("missing"))
            });
        }

        Promise::rejected(Error::new("missing"))
    }
}

/// `LinksList`.
pub struct LinksList {
    links: RefCell<Vec<Rc<Link>>>,
    disposables: Rc<DisposableStore>,
}

impl LinksList {
    /// `LinksList.Empty`: aqui uma lista nova e vazia a cada chamada.
    pub fn empty() -> Rc<Self> {
        Self::new(Vec::new())
    }

    /// `new LinksList(tuples)`.
    pub fn new(tuples: Vec<(Rc<ILinksList>, Rc<dyn LinkProvider>)>) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let mut links: Vec<Rc<Link>> = Vec::new();
        for (list, provider) in tuples {
            // merge all links
            let new_links: Vec<Rc<Link>> = list.links.iter().map(|link| Link::new(link.clone(), Rc::clone(&provider))).collect();
            links = Self::union(links, new_links);
            // register disposables
            let disposable: DisposableRef = list;
            disposables.add(disposable);
        }
        Rc::new(Self { links: RefCell::new(links), disposables })
    }

    /// `links`: uma cópia da lista atual.
    pub fn links(&self) -> Vec<Rc<Link>> {
        self.links.borrow().clone()
    }

    fn union(old_links: Vec<Rc<Link>>, new_links: Vec<Rc<Link>>) -> Vec<Rc<Link>> {
        // reunite oldLinks with newLinks and remove duplicates
        let mut result: Vec<Rc<Link>> = Vec::new();
        let (mut old_index, mut new_index) = (0, 0);
        let (old_len, new_len) = (old_links.len(), new_links.len());

        while old_index < old_len && new_index < new_len {
            let old_link = &old_links[old_index];
            let new_link = &new_links[new_index];

            if Range::are_intersecting_or_touching(&old_link.range(), &new_link.range()) {
                // Remove the oldLink
                old_index += 1;
                continue;
            }

            let comparison_result = Range::compare_ranges_using_starts(Some(&old_link.range()), Some(&new_link.range()));

            if comparison_result < 0 {
                // oldLink is before
                result.push(Rc::clone(old_link));
                old_index += 1;
            } else {
                // newLink is before
                result.push(Rc::clone(new_link));
                new_index += 1;
            }
        }

        result.extend(old_links[old_index..].iter().cloned());
        result.extend(new_links[new_index..].iter().cloned());

        result
    }
}

impl IDisposable for LinksList {
    fn dispose(&self) {
        self.disposables.dispose();
        self.links.borrow_mut().clear();
    }
}

/// `getLinks(providers, model, token)`.
pub fn get_links(providers: &LanguageFeatureRegistry<dyn LinkProvider>, model: &Rc<TextModel>, token: &CancellationToken) -> Promise<Rc<LinksList>> {
    let mut ordered = providers.ordered(model, false);
    ordered.reverse();
    let lists: Rc<RefCell<Vec<Option<(Rc<ILinksList>, Rc<dyn LinkProvider>)>>>> = Rc::new(RefCell::new(vec![None; ordered.len()]));
    let (model, token) = (Rc::clone(model), token.clone());

    async_fn(async move {
        // ask all providers for links in parallel
        let promises: Vec<Promise<()>> = ordered
            .into_iter()
            .enumerate()
            .map(|(index, provider)| {
                let (model, token, lists) = (Rc::clone(&model), token.clone(), Rc::clone(&lists));
                async_fn(async move {
                    match provider.provide_links(&model, &token).await {
                        Ok(Some(result)) => lists.borrow_mut()[index] = Some((result, provider)),
                        Ok(None) => {}
                        Err(err) => on_unexpected_external_error(&err),
                    }
                    Ok(())
                })
            })
            .collect();

        Promise::all(promises).await?;

        let tuples: Vec<(Rc<ILinksList>, Rc<dyn LinkProvider>)> = lists.borrow().iter().flatten().cloned().collect();
        let mut res = LinksList::new(tuples);

        if token.is_cancellation_requested() {
            res.dispose();
            res = LinksList::empty();
        }

        Ok(res)
    })
}
