//! Porte de `src/vs/platform/userDataSync/test/common/extensionsMerge.test.ts` (73 casos, na ordem do original).

use je_extensions::extensions_merge::{ExtensionState, MergeResult, SyncExtension, merge};
use je_extensions::manifest::ExtensionIdentifierInfo;
use serde_json::json;

/// O `Partial<ISyncExtension>` que cada fábrica do original recebe.
#[derive(Clone)]
struct Spec {
    id: &'static str,
    uuid: Option<&'static str>,
    version: Option<&'static str>,
    pinned: Option<bool>,
    pre_release: Option<bool>,
    disabled: Option<bool>,
    installed: Option<bool>,
    is_application_scoped: Option<bool>,
    state: Option<ExtensionState>,
}

/// `{ identifier: { id, uuid } }`.
fn s(id: &'static str, uuid: &'static str) -> Spec {
    Spec {
        id,
        uuid: Some(uuid),
        version: None,
        pinned: None,
        pre_release: None,
        disabled: None,
        installed: None,
        is_application_scoped: None,
        state: None,
    }
}

/// `{ identifier: { id } }`, sem uuid.
fn s_no_uuid(id: &'static str) -> Spec {
    Spec { uuid: None, ..s(id, "") }
}

impl Spec {
    fn version(mut self, version: &'static str) -> Self {
        self.version = Some(version);
        self
    }
    fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = Some(pinned);
        self
    }
    fn pre_release(mut self, pre_release: bool) -> Self {
        self.pre_release = Some(pre_release);
        self
    }
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = Some(disabled);
        self
    }
    fn installed(mut self, installed: bool) -> Self {
        self.installed = Some(installed);
        self
    }
    fn app_scoped(mut self, is_application_scoped: bool) -> Self {
        self.is_application_scoped = Some(is_application_scoped);
        self
    }
    fn state(mut self, entries: &[(&str, serde_json::Value)]) -> Self {
        self.state = Some(entries.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect());
        self
    }

    /// As fábricas do original diferem só nos padrões: se levam `pinned` e `preRelease` falsos, e `installed` verdadeiro.
    fn build(self, default_pinned_and_pre_release: bool, default_installed: bool) -> SyncExtension {
        let default_flag = default_pinned_and_pre_release.then_some(false);
        SyncExtension {
            identifier: ExtensionIdentifierInfo { id: self.id.to_owned(), uuid: self.uuid.map(str::to_owned) },
            version: self.version.unwrap_or("1.0.0").to_owned(),
            pinned: self.pinned.or(default_flag),
            pre_release: self.pre_release.or(default_flag),
            disabled: self.disabled,
            installed: self.installed.or(default_installed.then_some(true)),
            is_application_scoped: self.is_application_scoped,
            state: self.state,
        }
    }
}

/// `anExpectedSyncExtension`.
fn expected(spec: Spec) -> SyncExtension {
    spec.build(true, true)
}

/// `anExpectedBuiltinSyncExtension`.
fn builtin(spec: Spec) -> SyncExtension {
    spec.build(true, false)
}

/// `aLocalSyncExtension` e `aRemoteSyncExtension` (idênticas no original).
fn local(spec: Spec) -> SyncExtension {
    spec.build(true, true)
}

/// `aSyncExtension`.
fn sync(spec: Spec) -> SyncExtension {
    spec.build(false, true)
}

fn ident(id: &str, uuid: &str) -> ExtensionIdentifierInfo {
    ExtensionIdentifierInfo::with_uuid(id, uuid)
}

fn some(extensions: &[SyncExtension]) -> Option<&[SyncExtension]> {
    Some(extensions)
}

fn builtin_of(identifiers: &[ExtensionIdentifierInfo]) -> Option<&[ExtensionIdentifierInfo]> {
    Some(identifiers)
}

fn no_builtin() -> Option<&'static [ExtensionIdentifierInfo]> {
    Some(&[])
}

fn run(
    local_extensions: &[SyncExtension],
    remote_extensions: Option<&[SyncExtension]>,
    base_extensions: Option<&[SyncExtension]>,
    skipped_extensions: &[SyncExtension],
    ignored_extensions: &[&str],
    last_sync_builtin_extensions: Option<&[ExtensionIdentifierInfo]>,
) -> MergeResult {
    let ignored: Vec<String> = ignored_extensions.iter().map(|id| (*id).to_owned()).collect();
    merge(
        local_extensions,
        remote_extensions,
        base_extensions,
        skipped_extensions,
        &ignored,
        last_sync_builtin_extensions,
    )
}

/// `actual.remote?.all`: o original compara `undefined` com a lista esperada e falha, aqui o pânico faz o mesmo.
fn all(actual: &MergeResult) -> &[SyncExtension] {
    &actual.remote.as_ref().expect("remote nulo").all
}

#[test]
fn merge_returns_local_extension_if_remote_does_not_exist() {
    let local_extensions = vec![local(s("a", "a")), local(s("b", "b")), local(s("c", "c"))];

    let actual = run(&local_extensions, None, None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), local_extensions);
}

#[test]
fn merge_returns_local_extension_if_remote_does_not_exist_with_ignored_extensions() {
    let local_extensions = vec![local(s("a", "a")), local(s("b", "b")), local(s("c", "c"))];
    let expected = vec![local_extensions[1].clone(), local_extensions[2].clone()];

    let actual = run(&local_extensions, None, None, &[], &["a"], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected);
}

#[test]
fn merge_returns_local_extension_if_remote_does_not_exist_with_ignored_extensions_ignore_case() {
    let local_extensions = vec![local(s("a", "a")), local(s("b", "b")), local(s("c", "c"))];
    let expected = vec![local_extensions[1].clone(), local_extensions[2].clone()];

    let actual = run(&local_extensions, None, None, &[], &["A"], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected);
}

#[test]
fn merge_returns_local_extension_if_remote_does_not_exist_with_skipped_extensions() {
    let local_extensions = vec![local(s("a", "a")), local(s("b", "b")), local(s("c", "c"))];
    let skipped_extension = vec![sync(s("b", "b"))];
    let expected = local_extensions.clone();

    let actual = run(&local_extensions, None, None, &skipped_extension, &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected);
}

#[test]
fn merge_returns_local_extension_if_remote_does_not_exist_with_skipped_and_ignored_extensions() {
    let local_extensions = vec![local(s("a", "a")), local(s("b", "b")), local(s("c", "c"))];
    let skipped_extension = vec![sync(s("b", "b"))];
    let expected = vec![local_extensions[1].clone(), local_extensions[2].clone()];

    let actual = run(&local_extensions, None, None, &skipped_extension, &["a"], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected);
}

#[test]
fn merge_local_and_remote_extensions_when_there_is_no_base() {
    let local_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let remote_extensions = vec![sync(s("b", "b")), sync(s("c", "c"))];
    let expected_all = vec![expected(s("b", "b")), expected(s("c", "c")), expected(s("a", "a")), expected(s("d", "d"))];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, vec![expected(s("b", "b")), expected(s("c", "c"))]);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_local_and_remote_extensions_when_there_is_no_base_and_with_ignored_extensions() {
    let local_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let remote_extensions = vec![sync(s("b", "b")), sync(s("c", "c"))];
    let expected_all = vec![expected(s("b", "b")), expected(s("c", "c")), expected(s("d", "d"))];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &["a"], no_builtin());

    assert_eq!(actual.local.added, vec![expected(s("b", "b")), expected(s("c", "c"))]);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_local_and_remote_extensions_when_remote_is_moved_forwarded() {
    let base_extensions = vec![sync(s("a", "a")), sync(s("d", "d"))];
    let local_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let remote_extensions = vec![sync(s("b", "b")), sync(s("c", "c"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, vec![expected(s("b", "b")), expected(s("c", "c"))]);
    assert_eq!(actual.local.removed, vec![ident("a", "a"), ident("d", "d")]);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_and_remote_extensions_when_remote_is_moved_forwarded_with_disabled_extension() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let local_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let remote_extensions = vec![local(s("b", "b")), local(s("c", "c")), local(s("d", "d").disabled(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, vec![expected(s("b", "b")), expected(s("c", "c"))]);
    assert_eq!(actual.local.removed, vec![ident("a", "a")]);
    assert_eq!(actual.local.updated, vec![expected(s("d", "d").disabled(true))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_and_remote_extensions_when_remote_moved_forwarded_with_ignored_extensions() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let local_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let remote_extensions = vec![local(s("b", "b")), local(s("c", "c"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &["a"], no_builtin());

    assert_eq!(actual.local.added, vec![expected(s("b", "b")), expected(s("c", "c"))]);
    assert_eq!(actual.local.removed, vec![ident("d", "d")]);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_and_remote_extensions_when_remote_is_moved_forwarded_with_skipped_extensions() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let local_extensions = vec![local(s("d", "d"))];
    let skipped_extensions = vec![sync(s("a", "a"))];
    let remote_extensions = vec![local(s("b", "b")), local(s("c", "c"))];

    let actual = run(
        &local_extensions,
        some(&remote_extensions),
        some(&base_extensions),
        &skipped_extensions,
        &[],
        no_builtin(),
    );

    assert_eq!(actual.local.added, vec![expected(s("b", "b")), expected(s("c", "c"))]);
    assert_eq!(actual.local.removed, vec![ident("d", "d")]);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_and_remote_extensions_when_remote_is_moved_forwarded_with_skipped_and_ignored_extensions() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let local_extensions = vec![local(s("d", "d"))];
    let skipped_extensions = vec![sync(s("a", "a"))];
    let remote_extensions = vec![local(s("b", "b")), local(s("c", "c"))];

    let actual = run(
        &local_extensions,
        some(&remote_extensions),
        some(&base_extensions),
        &skipped_extensions,
        &["b"],
        no_builtin(),
    );

    assert_eq!(actual.local.added, vec![expected(s("c", "c"))]);
    assert_eq!(actual.local.removed, vec![ident("d", "d")]);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_and_remote_extensions_when_local_is_moved_forwarded() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let local_extensions = vec![local(s("b", "b")), local(s("c", "c"))];
    let remote_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let expected_all = vec![expected(s("b", "b")), expected(s("c", "c"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_local_and_remote_extensions_when_local_is_moved_forwarded_with_disabled_extensions() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let local_extensions = vec![local(s("a", "a").disabled(true)), local(s("b", "b")), local(s("c", "c"))];
    let remote_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let expected_all = vec![expected(s("a", "a").disabled(true)), expected(s("b", "b")), expected(s("c", "c"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_local_and_remote_extensions_when_local_is_moved_forwarded_with_ignored_settings() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let local_extensions = vec![local(s("b", "b")), local(s("c", "c"))];
    let remote_extensions = vec![local(s("a", "a")), local(s("d", "d"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &["b"], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![expected(s("c", "c"))]);
}

#[test]
fn merge_local_and_remote_extensions_when_local_is_moved_forwarded_with_skipped_extensions() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let skipped_extensions = vec![sync(s("d", "d"))];
    let local_extensions = vec![local(s("b", "b")), local(s("c", "c"))];
    let remote_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let expected_all = vec![expected(s("d", "d")), expected(s("b", "b")), expected(s("c", "c"))];

    let actual = run(
        &local_extensions,
        some(&remote_extensions),
        some(&base_extensions),
        &skipped_extensions,
        &[],
        no_builtin(),
    );

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_local_and_remote_extensions_when_local_is_moved_forwarded_with_skipped_and_ignored_extensions() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let skipped_extensions = vec![sync(s("d", "d"))];
    let local_extensions = vec![local(s("b", "b")), local(s("c", "c"))];
    let remote_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let expected_all = vec![expected(s("d", "d")), expected(s("b", "b"))];

    let actual = run(
        &local_extensions,
        some(&remote_extensions),
        some(&base_extensions),
        &skipped_extensions,
        &["c"],
        no_builtin(),
    );

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_local_and_remote_extensions_when_both_moved_forwarded() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let local_extensions = vec![local(s("a", "a")), local(s("b", "b")), local(s("c", "c"))];
    let remote_extensions = vec![local(s("d", "d")), local(s("b", "b")), local(s("e", "e"))];
    let expected_all = vec![expected(s("b", "b")), expected(s("e", "e")), expected(s("c", "c"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, vec![expected(s("e", "e"))]);
    assert_eq!(actual.local.removed, vec![ident("a", "a")]);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_local_and_remote_extensions_when_both_moved_forwarded_with_ignored_extensions() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let local_extensions = vec![local(s("a", "a")), local(s("b", "b")), local(s("c", "c"))];
    let remote_extensions = vec![local(s("d", "d")), local(s("b", "b")), local(s("e", "e"))];
    let expected_all = vec![expected(s("b", "b")), expected(s("e", "e")), expected(s("c", "c"))];

    let actual =
        run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &["a", "e"], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_local_and_remote_extensions_when_both_moved_forwarded_with_skipped_extensions() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let skipped_extensions = vec![sync(s("a", "a"))];
    let local_extensions = vec![local(s("b", "b")), local(s("c", "c"))];
    let remote_extensions = vec![local(s("d", "d")), local(s("b", "b")), local(s("e", "e"))];
    let expected_all = vec![expected(s("b", "b")), expected(s("e", "e")), expected(s("c", "c"))];

    let actual = run(
        &local_extensions,
        some(&remote_extensions),
        some(&base_extensions),
        &skipped_extensions,
        &[],
        no_builtin(),
    );

    assert_eq!(actual.local.added, vec![expected(s("e", "e"))]);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_local_and_remote_extensions_when_both_moved_forwarded_with_skipped_and_ignoredextensions() {
    let base_extensions = vec![local(s("a", "a")), local(s("d", "d"))];
    let skipped_extensions = vec![sync(s("a", "a"))];
    let local_extensions = vec![local(s("b", "b")), local(s("c", "c"))];
    let remote_extensions = vec![local(s("d", "d")), local(s("b", "b")), local(s("e", "e"))];
    let expected_all = vec![expected(s("b", "b")), expected(s("e", "e")), expected(s("c", "c"))];

    let actual = run(
        &local_extensions,
        some(&remote_extensions),
        some(&base_extensions),
        &skipped_extensions,
        &["e"],
        no_builtin(),
    );

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_when_remote_extension_has_no_uuid_and_different_extension_id_case() {
    let local_extensions = vec![local(s("a", "a")), local(s("b", "b")), local(s("c", "c"))];
    let remote_extensions = vec![local(s_no_uuid("A")), local(s("d", "d"))];
    let expected_all = vec![expected(s("A", "a")), expected(s("d", "d")), expected(s("b", "b")), expected(s("c", "c"))];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, vec![expected(s("d", "d"))]);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_when_remote_extension_is_not_an_installed_extension() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a")), local(s("b", "b").installed(false))];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_when_remote_extension_is_not_an_installed_extension_but_is_an_installed_extension_locally() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a").installed(false))];
    let expected_all = vec![expected(s("a", "a"))];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_when_an_extension_is_not_an_installed_extension_remotely_and_does_not_exist_locally() {
    let local_extensions = vec![local(s("a", "a").installed(false))];
    let remote_extensions = vec![local(s("a", "a").installed(false)), local(s("b", "b").installed(false))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_when_an_extension_is_an_installed_extension_remotely_but_not_locally_and_updated_locally() {
    let local_extensions = vec![local(s("a", "a").disabled(true))];
    let remote_extensions = vec![local(s("a", "a"))];
    let expected_all = vec![expected(s("a", "a").disabled(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_when_an_extension_is_an_installed_extension_remotely_but_not_locally_and_updated_remotely() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a").disabled(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&local_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").disabled(true))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_not_installed_extensions() {
    let local_extensions = vec![local(s("a", "a").installed(false))];
    let remote_extensions = vec![local(s("b", "b").installed(false))];
    let expected_all = vec![builtin(s("b", "b")), builtin(s("a", "a"))];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), expected_all);
}

#[test]
fn merge_remote_extension_with_prerelease_is_added() {
    let local_extensions: Vec<SyncExtension> = vec![];
    let remote_extensions = vec![local(s("a", "a").pre_release(true))];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, vec![expected(s("a", "a").pre_release(true))]);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_extension_with_prerelease_is_added() {
    let local_extensions = vec![local(s("a", "a").pre_release(true))];
    let remote_extensions: Vec<SyncExtension> = vec![];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![expected(s("a", "a").pre_release(true))]);
}

#[test]
fn merge_remote_extension_with_prerelease_is_added_when_local_extension_without_prerelease_is_added() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a").pre_release(true))];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").pre_release(true))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_remote_extension_without_prerelease_is_added_when_local_extension_with_prerelease_is_added() {
    let local_extensions = vec![local(s("a", "a").pre_release(true))];
    let remote_extensions = vec![local(s("a", "a"))];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a"))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_remote_extension_is_changed_to_prerelease() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a").pre_release(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&local_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").pre_release(true))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_remote_extension_is_changed_to_release() {
    let local_extensions = vec![local(s("a", "a").pre_release(true))];
    let remote_extensions = vec![local(s("a", "a"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&local_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a"))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_extension_is_changed_to_prerelease() {
    let local_extensions = vec![local(s("a", "a").pre_release(true))];
    let remote_extensions = vec![local(s("a", "a"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![expected(s("a", "a").pre_release(true))]);
}

#[test]
fn merge_local_extension_is_changed_to_release() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a").pre_release(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![local(s("a", "a"))]);
}

#[test]
fn merge_local_extension_not_an_installed_extension_remote_prerelease_property_is_taken_precedence_when_there_are_no_updates()
 {
    let local_extensions = vec![local(s("a", "a").installed(false))];
    let remote_extensions = vec![local(s("a", "a").pre_release(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_extension_not_an_installed_extension_remote_prerelease_property_is_taken_precedence_when_there_are_updates_locally()
 {
    let local_extensions = vec![local(s("a", "a").installed(false).disabled(true))];
    let remote_extensions = vec![local(s("a", "a").pre_release(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![expected(s("a", "a").pre_release(true).disabled(true))]);
}

#[test]
fn merge_local_extension_not_an_installed_extension_remote_prerelease_property_is_taken_precedence_when_there_are_updates_remotely()
 {
    let local_extensions = vec![local(s("a", "a").installed(false))];
    let base_extensions = vec![local(s("a", "a").pre_release(true))];
    let remote_extensions = vec![local(s("a", "a").pre_release(true).disabled(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").pre_release(true).disabled(true))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_extension_not_an_installed_extension_remote_version_is_taken_precedence_when_there_are_no_updates() {
    let local_extensions = vec![local(s("a", "a").installed(false))];
    let remote_extensions = vec![local(s("a", "a").version("1.1.0"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_extension_not_an_installed_extension_remote_version_is_taken_precedence_when_there_are_updates_locally() {
    let local_extensions = vec![local(s("a", "a").installed(false).disabled(true))];
    let remote_extensions = vec![local(s("a", "a").version("1.1.0"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![expected(s("a", "a").version("1.1.0").disabled(true))]);
}

#[test]
fn merge_local_extension_not_an_installed_extension_remote_version_property_is_taken_precedence_when_there_are_updates_remotely()
 {
    let local_extensions = vec![local(s("a", "a").installed(false))];
    let base_extensions = vec![local(s("a", "a").version("1.1.0"))];
    let remote_extensions = vec![local(s("a", "a").version("1.1.0").disabled(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").version("1.1.0").disabled(true))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_base_has_builtin_extension_local_does_not_have_extension_remote_has_extension_installed() {
    let local_extensions: Vec<SyncExtension> = vec![];
    let base_extensions = vec![local(s("a", "a").version("1.1.0").installed(false))];
    let remote_extensions = vec![local(s("a", "a").version("1.1.0"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, vec![expected(s("a", "a").version("1.1.0"))]);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_base_has_installed_extension_local_has_installed_extension_remote_has_extension_builtin() {
    let local_extensions = vec![local(s("a", "a"))];
    let base_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a").installed(false))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, vec![ident("a", "a")]);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_base_has_installed_extension_local_has_builtin_extension_remote_does_not_has_extension() {
    let local_extensions = vec![local(s("a", "a").installed(false))];
    let base_extensions = vec![local(s("a", "a"))];
    let remote_extensions: Vec<SyncExtension> = vec![];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![builtin(s("a", "a"))]);
}

#[test]
fn merge_base_has_builtin_extension_local_has_installed_extension_remote_has_builtin_extension_with_updated_state() {
    let local_extensions = vec![local(s("a", "a"))];
    let base_extensions = vec![local(s("a", "a").installed(false))];
    let remote_extensions = vec![local(s("a", "a").installed(false).state(&[("a", json!(1))]))];

    let actual = run(
        &local_extensions,
        some(&remote_extensions),
        some(&base_extensions),
        &[],
        &[],
        builtin_of(&[ident("a", "a")]),
    );

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").state(&[("a", json!(1))]))]);
    assert_eq!(all(&actual), vec![expected(s("a", "a").state(&[("a", json!(1))]))]);
}

#[test]
fn merge_base_has_installed_extension_last_time_synced_as_builtin_extension_local_has_installed_extension_remote_has_builtin_extension_with_updated_state()
 {
    let local_extensions = vec![local(s("a", "a"))];
    let base_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a").installed(false).state(&[("a", json!(1))]))];

    let actual = run(
        &local_extensions,
        some(&remote_extensions),
        some(&base_extensions),
        &[],
        &[],
        builtin_of(&[ident("a", "a")]),
    );

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").state(&[("a", json!(1))]))]);
    assert_eq!(all(&actual), vec![expected(s("a", "a").state(&[("a", json!(1))]))]);
}

#[test]
fn merge_base_has_builtin_extension_local_does_not_have_extension_remote_has_builtin_extension() {
    let local_extensions: Vec<SyncExtension> = vec![];
    let base_extensions = vec![local(s("a", "a").version("1.1.0").installed(false))];
    let remote_extensions = vec![local(s("a", "a").version("1.1.0").installed(false))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_base_has_installed_extension_last_synced_as_builtin_local_does_not_have_extension_remote_has_installed_extension()
 {
    let local_extensions: Vec<SyncExtension> = vec![];
    let base_extensions = vec![local(s("a", "a").version("1.1.0"))];
    let remote_extensions = vec![local(s("a", "a").version("1.1.0"))];

    let actual = run(
        &local_extensions,
        some(&remote_extensions),
        some(&base_extensions),
        &[],
        &[],
        builtin_of(&[ident("a", "a")]),
    );

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_base_has_builtin_extension_last_synced_as_builtin_local_does_not_have_extension_remote_has_installed_extension()
 {
    let local_extensions: Vec<SyncExtension> = vec![];
    let base_extensions = vec![local(s("a", "a").version("1.1.0").installed(false))];
    let remote_extensions = vec![local(s("a", "a").version("1.1.0"))];

    let actual = run(
        &local_extensions,
        some(&remote_extensions),
        some(&base_extensions),
        &[],
        &[],
        builtin_of(&[ident("a", "a")]),
    );

    assert_eq!(actual.local.added, vec![expected(s("a", "a").version("1.1.0"))]);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_remote_extension_with_pinned_is_added() {
    let local_extensions: Vec<SyncExtension> = vec![];
    let remote_extensions = vec![local(s("a", "a").pinned(true))];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, vec![expected(s("a", "a").pinned(true))]);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_extension_with_pinned_is_added() {
    let local_extensions = vec![local(s("a", "a").pinned(true))];
    let remote_extensions: Vec<SyncExtension> = vec![];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![expected(s("a", "a").pinned(true))]);
}

#[test]
fn merge_remote_extension_with_pinned_is_added_when_local_extension_without_pinned_is_added() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a").pinned(true))];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").pinned(true))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_remote_extension_without_pinned_is_added_when_local_extension_with_pinned_is_added() {
    let local_extensions = vec![local(s("a", "a").pinned(true))];
    let remote_extensions = vec![local(s("a", "a"))];

    let actual = run(&local_extensions, some(&remote_extensions), None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a"))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_remote_extension_is_changed_to_pinned() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a").pinned(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&local_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").pinned(true))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_remote_extension_is_changed_to_unpinned() {
    let local_extensions = vec![local(s("a", "a").pinned(true))];
    let remote_extensions = vec![local(s("a", "a"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&local_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a"))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_extension_is_changed_to_pinned() {
    let local_extensions = vec![local(s("a", "a").pinned(true))];
    let remote_extensions = vec![local(s("a", "a"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![expected(s("a", "a").pinned(true))]);
}

#[test]
fn merge_local_extension_is_changed_to_unpinned() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a").pinned(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![local(s("a", "a"))]);
}

#[test]
fn merge_local_extension_not_an_installed_extension_remote_pinned_property_is_taken_precedence_when_there_are_no_updates()
 {
    let local_extensions = vec![local(s("a", "a").installed(false))];
    let remote_extensions = vec![local(s("a", "a").pinned(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_extension_not_an_installed_extension_remote_pinned_property_is_taken_precedence_when_there_are_updates_locally()
 {
    let local_extensions = vec![local(s("a", "a").installed(false).disabled(true))];
    let remote_extensions = vec![local(s("a", "a").pinned(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![expected(s("a", "a").pinned(true).disabled(true))]);
}

#[test]
fn merge_local_extension_not_an_installed_extension_remote_pinned_property_is_taken_precedence_when_there_are_updates_remotely()
 {
    let local_extensions = vec![local(s("a", "a").installed(false))];
    let base_extensions = vec![local(s("a", "a").pinned(true))];
    let remote_extensions = vec![local(s("a", "a").pinned(true).disabled(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").pinned(true).disabled(true))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_extension_is_changed_to_pinned_and_version_changed() {
    let local_extensions = vec![local(s("a", "a").version("0.0.1").pinned(true))];
    let remote_extensions = vec![local(s("a", "a"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![expected(s("a", "a").version("0.0.1").pinned(true))]);
}

#[test]
fn merge_local_extension_is_changed_to_unpinned_and_version_changed() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a").version("0.0.1").pinned(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&remote_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![expected(s("a", "a"))]);
}

#[test]
fn merge_remote_extension_is_changed_to_pinned_and_version_changed() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a").version("0.0.1").pinned(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&local_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").version("0.0.1").pinned(true))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_extension_is_changed_to_pinned_and_version_changed_and_remote_extension_is_channged_to_pinned_with_different_version()
 {
    let local_extensions = vec![local(s("a", "a").version("0.0.1").pinned(true))];
    let remote_extensions = vec![local(s("a", "a").version("0.0.2").pinned(true))];
    let base_extensions = vec![local(s("a", "a"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").version("0.0.2").pinned(true))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_remote_extension_is_changed_to_unpinned_and_version_changed() {
    let local_extensions = vec![local(s("a", "a").version("0.0.1").pinned(true))];
    let remote_extensions = vec![local(s("a", "a"))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&local_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a"))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_local_extension_is_changed_to_unpinned_and_version_changed_and_remote_extension_is_channged_to_unpinned_with_different_version()
 {
    let local_extensions = vec![local(s("a", "a").version("0.0.1"))];
    let remote_extensions = vec![local(s("a", "a").version("0.0.2"))];
    let base_extensions = vec![local(s("a", "a").pinned(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn sync_adding_local_application_scoped_extension() {
    let local_extensions = vec![local(s("a", "a").app_scoped(true))];

    let actual = run(&local_extensions, None, None, &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), local_extensions);
}

#[test]
fn sync_merging_local_extension_with_is_application_scoped_property_and_remote_does_not_has_is_application_scoped_property()
 {
    let local_extensions = vec![local(s("a", "a").app_scoped(false))];
    let base_extensions = vec![sync(s("a", "a"))];

    let actual = run(&local_extensions, some(&base_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), vec![expected(s("a", "a"))]);
}

#[test]
fn sync_merging_when_applicaiton_scope_is_changed_locally() {
    let local_extensions = vec![local(s("a", "a").app_scoped(true))];
    let base_extensions = vec![local(s("a", "a").app_scoped(false))];

    let actual = run(&local_extensions, some(&base_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(all(&actual), local_extensions);
}

#[test]
fn sync_merging_when_applicaiton_scope_is_changed_remotely() {
    let local_extensions = vec![local(s("a", "a").app_scoped(false))];
    let base_extensions = vec![local(s("a", "a").app_scoped(false))];
    let remote_extensions = vec![local(s("a", "a").app_scoped(true))];

    let actual = run(&local_extensions, some(&remote_extensions), some(&base_extensions), &[], &[], no_builtin());

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, vec![expected(s("a", "a").app_scoped(true))]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_does_not_remove_remote_extension_when_skipped_extension_has_uuid_but_remote_does_not_has() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a")), local(s_no_uuid("b"))];

    let actual = run(
        &local_extensions,
        some(&remote_extensions),
        some(&remote_extensions),
        &[local(s("b", "b"))],
        &[],
        no_builtin(),
    );

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_does_not_remove_remote_extension_when_last_sync_builtin_extension_has_uuid_but_remote_does_not_has() {
    let local_extensions = vec![local(s("a", "a"))];
    let remote_extensions = vec![local(s("a", "a")), local(s_no_uuid("b"))];

    let actual = run(
        &local_extensions,
        some(&remote_extensions),
        some(&remote_extensions),
        &[],
        &[],
        builtin_of(&[ident("b", "b")]),
    );

    assert_eq!(actual.local.added, []);
    assert_eq!(actual.local.removed, []);
    assert_eq!(actual.local.updated, []);
    assert_eq!(actual.remote, None);
}
