//! Porte de `src/vs/platform/userDataSync/test/common/userDataProfilesManifestMerge.test.ts` (8 casos, na ordem do original).

use std::rc::Rc;

use je_base::path::{FixedEnvironment, Platform};
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::user_data_profile::{UseDefaultProfileFlags, UserDataProfile, UserDataProfileOptions, to_user_data_profile};
use je_platform::user_data_sync::user_data_profiles_manifest_merge::merge;
use je_platform::user_data_sync::user_data_sync::ISyncUserDataProfile;

fn ext_uri() -> ExtUri {
    ExtUri::unbiased(Rc::new(FixedEnvironment::new(Platform::Posix, "/")))
}

fn file(path: &str) -> URI {
    URI::file(path, Platform::Posix)
}

/// `toUserDataProfile(id, name, URI.file(location), URI.file('cache'))`.
fn profile(id: &str, name: &str, location: &str) -> UserDataProfile {
    to_user_data_profile(id, name, &file(location), &file("cache"), None, None, &ext_uri())
}

/// O mesmo, com `{ useDefaultFlags }` nas opções.
fn profile_with_flags(id: &str, name: &str, location: &str, flags: UseDefaultProfileFlags) -> UserDataProfile {
    let options = UserDataProfileOptions { use_default_flags: Some(flags), ..Default::default() };
    to_user_data_profile(id, name, &file(location), &file("cache"), Some(&options), None, &ext_uri())
}

/// `{ id, name, collection }`.
fn sync(id: &str, name: &str, collection: &str) -> ISyncUserDataProfile {
    ISyncUserDataProfile { id: id.to_string(), collection: collection.to_string(), name: name.to_string(), icon: None, use_default_flags: None }
}

/// `{ id, name, collection, useDefaultFlags }`.
fn sync_with_flags(id: &str, name: &str, collection: &str, flags: UseDefaultProfileFlags) -> ISyncUserDataProfile {
    ISyncUserDataProfile { use_default_flags: Some(flags), ..sync(id, name, collection) }
}

fn ids(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[test]
fn merge_returns_local_profiles_if_remote_does_not_exist() {
    let local_profiles = vec![profile("1", "1", "1"), profile("2", "2", "2")];

    let actual = merge(&local_profiles, None, None, &[]);

    assert_eq!(actual.local.added, vec![]);
    assert_eq!(actual.local.removed, vec![]);
    assert_eq!(actual.local.updated, vec![]);
    let remote = actual.remote.as_ref().expect("remote");
    assert_eq!(remote.added, local_profiles);
    assert_eq!(remote.updated, vec![]);
    assert_eq!(remote.removed, vec![]);
}

#[test]
fn merge_returns_local_profiles_if_remote_does_not_exist_with_ignored_profiles() {
    let local_profiles = vec![profile("1", "1", "1"), profile("2", "2", "2")];

    let actual = merge(&local_profiles, None, None, &ids(&["2"]));

    assert_eq!(actual.local.added, vec![]);
    assert_eq!(actual.local.removed, vec![]);
    assert_eq!(actual.local.updated, vec![]);
    let remote = actual.remote.as_ref().expect("remote");
    assert_eq!(remote.added, vec![local_profiles[0].clone()]);
    assert_eq!(remote.updated, vec![]);
    assert_eq!(remote.removed, vec![]);
}

#[test]
fn merge_local_and_remote_profiles_when_there_is_no_base() {
    let local_profiles = vec![profile("1", "1", "1"), profile("2", "2", "2")];
    let remote_profiles = vec![sync("1", "changed", "1"), sync("3", "3", "3")];

    let actual = merge(&local_profiles, Some(&remote_profiles), None, &[]);

    assert_eq!(actual.local.added, vec![remote_profiles[1].clone()]);
    assert_eq!(actual.local.removed, vec![]);
    assert_eq!(actual.local.updated, vec![remote_profiles[0].clone()]);
    let remote = actual.remote.as_ref().expect("remote");
    assert_eq!(remote.added, vec![local_profiles[1].clone()]);
    assert_eq!(remote.updated, vec![]);
    assert_eq!(remote.removed, vec![]);
}

#[test]
fn merge_local_and_remote_profiles_when_there_is_base() {
    let local_profiles = vec![
        profile("1", "changed 1", "1"),
        profile("3", "3", "3"),
        profile("4", "changed local", "4"),
        profile("5", "5", "5"),
        profile("6", "6", "6"),
        profile("8", "8", "8"),
        profile_with_flags("10", "10", "8", UseDefaultProfileFlags { tasks: Some(true), ..Default::default() }),
        profile_with_flags("11", "11", "1", UseDefaultProfileFlags { keybindings: Some(true), ..Default::default() }),
    ];
    let base = vec![
        sync("1", "1", "1"),
        sync("2", "2", "2"),
        sync("3", "3", "3"),
        sync("4", "4", "4"),
        sync("5", "5", "5"),
        sync("6", "6", "6"),
        sync_with_flags("10", "10", "10", UseDefaultProfileFlags { tasks: Some(true), ..Default::default() }),
        sync("11", "11", "11"),
    ];
    let remote_profiles = vec![
        sync("1", "1", "1"),
        sync("2", "2", "2"),
        sync("3", "3", "3"),
        sync("4", "changed remote", "4"),
        sync("5", "5", "5"),
        sync("7", "7", "7"),
        sync_with_flags("9", "9", "9", UseDefaultProfileFlags { snippets: Some(true), ..Default::default() }),
        sync("10", "10", "10"),
        sync("11", "11", "11"),
    ];

    let actual = merge(&local_profiles, Some(&remote_profiles), Some(&base), &[]);

    assert_eq!(actual.local.added, vec![remote_profiles[5].clone(), remote_profiles[6].clone()]);
    assert_eq!(actual.local.removed, vec![local_profiles[4].clone()]);
    assert_eq!(actual.local.updated, vec![remote_profiles[3].clone(), remote_profiles[7].clone()]);
    let remote = actual.remote.as_ref().expect("remote");
    assert_eq!(remote.added, vec![local_profiles[5].clone()]);
    assert_eq!(remote.updated, vec![local_profiles[0].clone(), local_profiles[7].clone()]);
    assert_eq!(remote.removed, vec![remote_profiles[1].clone()]);
}

#[test]
fn merge_local_and_remote_profiles_when_there_is_base_with_ignored_profiles() {
    let local_profiles = vec![
        profile("1", "changed 1", "1"),
        profile("3", "3", "3"),
        profile("4", "changed local", "4"),
        profile("5", "5", "5"),
        profile("6", "6", "6"),
        profile("8", "8", "8"),
    ];
    let base = vec![
        sync("1", "1", "1"),
        sync("2", "2", "2"),
        sync("3", "3", "3"),
        sync("4", "4", "4"),
        sync("5", "5", "5"),
        sync("6", "6", "6"),
    ];
    let remote_profiles = vec![
        sync("1", "1", "1"),
        sync("2", "2", "2"),
        sync("3", "changed 3", "3"),
        sync("4", "changed remote", "4"),
        sync("5", "5", "5"),
        sync("7", "7", "7"),
    ];

    let actual = merge(&local_profiles, Some(&remote_profiles), Some(&base), &ids(&["4", "8"]));

    assert_eq!(actual.local.added, vec![remote_profiles[5].clone()]);
    assert_eq!(actual.local.removed, vec![local_profiles[4].clone()]);
    assert_eq!(actual.local.updated, vec![remote_profiles[2].clone()]);
    let remote = actual.remote.as_ref().expect("remote");
    assert_eq!(remote.added, vec![]);
    assert_eq!(remote.updated, vec![local_profiles[0].clone()]);
    assert_eq!(remote.removed, vec![remote_profiles[1].clone()]);
}

#[test]
fn merge_when_there_are_no_remote_changes() {
    let local_profiles = vec![profile("1", "1", "1")];
    let base = vec![sync("1", "1", "1")];
    let remote_profiles = vec![sync("1", "name changed", "1")];

    let actual = merge(&local_profiles, Some(&remote_profiles), Some(&base), &[]);

    assert_eq!(actual.local.added, vec![]);
    assert_eq!(actual.local.removed, vec![]);
    assert_eq!(actual.local.updated, vec![remote_profiles[0].clone()]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_when_there_are_no_local_and_remote_changes() {
    let local_profiles = vec![profile("1", "1", "1")];
    let base = vec![sync("1", "1", "1")];
    let remote_profiles = vec![sync("1", "1", "1")];

    let actual = merge(&local_profiles, Some(&remote_profiles), Some(&base), &[]);

    assert_eq!(actual.local.added, vec![]);
    assert_eq!(actual.local.removed, vec![]);
    assert_eq!(actual.local.updated, vec![]);
    assert_eq!(actual.remote, None);
}

#[test]
fn merge_when_profile_is_removed_locally_but_not_exists_in_remote() {
    let local_profiles = vec![profile("1", "1", "1")];
    let base = vec![sync("1", "1", "1"), sync("2", "2", "2")];
    let remote_profiles = vec![sync("1", "3", "1")];

    let actual = merge(&local_profiles, Some(&remote_profiles), Some(&base), &[]);

    assert_eq!(actual.local.added, vec![]);
    assert_eq!(actual.local.removed, vec![]);
    assert_eq!(actual.local.updated, remote_profiles);
    assert_eq!(actual.remote, None);
}
