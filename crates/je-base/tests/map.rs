//! Porte de `src/vs/base/test/common/map.test.ts` (suíte `Map`), caso a caso e na mesma ordem.
//! As suítes `BidirectionalMap`, `SetMap` e `NKeyMap` do mesmo arquivo vêm em seguida, na ordem original.
//!
//! O `Map`/`Set` do JavaScript é o `OrderedMap` do porte. Os valores `any` do original são o enum `Val`.
//! Os caches `LRUCache` e `MRUCache` são aliases de `Cache<K, V, OLD>`.

use std::rc::Rc;

use je_base::errors;
use je_base::map::{
    maps_strict_equal_ignore_order, BidirectionalMap, LRUCache, LinkedMap, MRUCache, NKey, NKeyMap, OrderedMap, ResourceMap,
    ResourceMapKeyFn, SetMap, Touch,
};
use je_base::path::{FixedEnvironment, Platform};
use je_base::resources::ExtUri;
use je_base::uri::URI;

/// Os valores `any` do original (número, texto, booleano, objeto, data).
#[derive(Clone, Debug, PartialEq)]
enum Val {
    Num(i64),
    Str(String),
    Bool(bool),
    Obj(u32),
}

fn parse(value: &str) -> URI {
    URI::parse(value, false)
}

fn file(path: &str) -> URI {
    URI::file(path, Platform::Posix)
}

fn ignore_case_key_fn() -> ResourceMapKeyFn {
    let ext_uri = ExtUri::ignore_path_case(Rc::new(FixedEnvironment::new(Platform::Posix, "/workspace/project")));
    Rc::new(move |uri| ext_uri.get_comparison_key(uri, false))
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn linked_map_simple() {
    let map = LinkedMap::<String, String>::new();
    map.set("ak".into(), "av".into(), Touch::None);
    map.set("bk".into(), "bv".into(), Touch::None);
    assert_eq!(map.keys().collect::<Vec<_>>(), strs(&["ak", "bk"]));
    assert_eq!(map.values().collect::<Vec<_>>(), strs(&["av", "bv"]));
    assert_eq!(map.first().as_deref(), Some("av"));
    assert_eq!(map.last().as_deref(), Some("bv"));
}

#[test]
fn linked_map_touch_old_one() {
    let map = LinkedMap::<String, String>::new();
    map.set("ak".into(), "av".into(), Touch::None);
    map.set("ak".into(), "av".into(), Touch::AsOld);
    assert_eq!(map.keys().collect::<Vec<_>>(), strs(&["ak"]));
    assert_eq!(map.values().collect::<Vec<_>>(), strs(&["av"]));
}

#[test]
fn linked_map_touch_new_one() {
    let map = LinkedMap::<String, String>::new();
    map.set("ak".into(), "av".into(), Touch::None);
    map.set("ak".into(), "av".into(), Touch::AsNew);
    assert_eq!(map.keys().collect::<Vec<_>>(), strs(&["ak"]));
    assert_eq!(map.values().collect::<Vec<_>>(), strs(&["av"]));
}

#[test]
fn linked_map_touch_old_two() {
    let map = LinkedMap::<String, String>::new();
    map.set("ak".into(), "av".into(), Touch::None);
    map.set("bk".into(), "bv".into(), Touch::None);
    map.set("bk".into(), "bv".into(), Touch::AsOld);
    assert_eq!(map.keys().collect::<Vec<_>>(), strs(&["bk", "ak"]));
    assert_eq!(map.values().collect::<Vec<_>>(), strs(&["bv", "av"]));
}

#[test]
fn linked_map_touch_new_two() {
    let map = LinkedMap::<String, String>::new();
    map.set("ak".into(), "av".into(), Touch::None);
    map.set("bk".into(), "bv".into(), Touch::None);
    map.set("ak".into(), "av".into(), Touch::AsNew);
    assert_eq!(map.keys().collect::<Vec<_>>(), strs(&["bk", "ak"]));
    assert_eq!(map.values().collect::<Vec<_>>(), strs(&["bv", "av"]));
}

#[test]
fn linked_map_touch_old_from_middle() {
    let map = LinkedMap::<String, String>::new();
    map.set("ak".into(), "av".into(), Touch::None);
    map.set("bk".into(), "bv".into(), Touch::None);
    map.set("ck".into(), "cv".into(), Touch::None);
    map.set("bk".into(), "bv".into(), Touch::AsOld);
    assert_eq!(map.keys().collect::<Vec<_>>(), strs(&["bk", "ak", "ck"]));
    assert_eq!(map.values().collect::<Vec<_>>(), strs(&["bv", "av", "cv"]));
}

#[test]
fn linked_map_touch_new_from_middle() {
    let map = LinkedMap::<String, String>::new();
    map.set("ak".into(), "av".into(), Touch::None);
    map.set("bk".into(), "bv".into(), Touch::None);
    map.set("ck".into(), "cv".into(), Touch::None);
    map.set("bk".into(), "bv".into(), Touch::AsNew);
    assert_eq!(map.keys().collect::<Vec<_>>(), strs(&["ak", "ck", "bk"]));
    assert_eq!(map.values().collect::<Vec<_>>(), strs(&["av", "cv", "bv"]));
}

#[test]
fn linked_map_basics() {
    let map = LinkedMap::<String, Val>::new();
    let key = |k: &str| k.to_string();

    assert_eq!(map.size(), 0);

    map.set(key("1"), Val::Num(1), Touch::None);
    map.set(key("2"), Val::Str("2".into()), Touch::None);
    map.set(key("3"), Val::Bool(true), Touch::None);

    let obj = Val::Obj(1);
    map.set(key("4"), obj.clone(), Touch::None);

    let date = Val::Num(1_700_000_000_000);
    map.set(key("5"), date.clone(), Touch::None);

    assert_eq!(map.size(), 5);
    assert_eq!(map.get(&key("1"), Touch::None), Some(Val::Num(1)));
    assert_eq!(map.get(&key("2"), Touch::None), Some(Val::Str("2".into())));
    assert_eq!(map.get(&key("3"), Touch::None), Some(Val::Bool(true)));
    assert_eq!(map.get(&key("4"), Touch::None), Some(obj));
    assert_eq!(map.get(&key("5"), Touch::None), Some(date));
    assert!(map.get(&key("6"), Touch::None).is_none());

    map.delete(&key("6"));
    assert_eq!(map.size(), 5);
    assert!(map.delete(&key("1")));
    assert!(map.delete(&key("2")));
    assert!(map.delete(&key("3")));
    assert!(map.delete(&key("4")));
    assert!(map.delete(&key("5")));

    assert_eq!(map.size(), 0);
    for k in ["5", "4", "3", "2", "1"] {
        assert!(map.get(&key(k), Touch::None).is_none());
    }

    map.set(key("1"), Val::Num(1), Touch::None);
    map.set(key("2"), Val::Str("2".into()), Touch::None);
    map.set(key("3"), Val::Bool(true), Touch::None);

    assert!(map.has(&key("1")));
    assert_eq!(map.get(&key("1"), Touch::None), Some(Val::Num(1)));
    assert_eq!(map.get(&key("2"), Touch::None), Some(Val::Str("2".into())));
    assert_eq!(map.get(&key("3"), Touch::None), Some(Val::Bool(true)));

    map.clear();

    assert_eq!(map.size(), 0);
    assert!(map.get(&key("1"), Touch::None).is_none());
    assert!(map.get(&key("2"), Touch::None).is_none());
    assert!(map.get(&key("3"), Touch::None).is_none());
    assert!(!map.has(&key("1")));
}

#[test]
fn linked_map_iterators() {
    let map = LinkedMap::<i32, i32>::new();
    map.set(1, 1, Touch::None);
    map.set(2, 2, Touch::None);
    map.set(3, 3, Touch::None);

    for elem in map.keys() {
        assert!(elem != 0);
    }

    for elem in map.values() {
        assert!(elem != 0);
    }

    for elem in map.entries() {
        assert!(elem.0 != 0);
    }

    {
        let mut keys = map.keys();
        let mut values = map.values();
        let mut entries = map.entries();
        map.get(&1, Touch::None);
        keys.next();
        values.next();
        entries.next();
    }

    {
        let mut keys = map.keys();
        let mut values = map.values();
        let mut entries = map.entries();
        map.get(&1, Touch::AsNew);

        let mut exceptions = 0;
        if errors::catch(|| keys.next()).is_err() {
            exceptions += 1;
        }
        if errors::catch(|| values.next()).is_err() {
            exceptions += 1;
        }
        if errors::catch(|| entries.next()).is_err() {
            exceptions += 1;
        }

        assert_eq!(exceptions, 3);
    }
}

#[test]
fn linked_map_lru_cache_simple() {
    let cache = LRUCache::<i32, i32>::new(5, 1.0);

    [1, 2, 3, 4, 5].iter().for_each(|&value| {
        cache.set(value, value);
    });
    assert_eq!(cache.size(), 5);
    cache.set(6, 6);
    assert_eq!(cache.size(), 5);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![2, 3, 4, 5, 6]);
    cache.set(7, 7);
    assert_eq!(cache.size(), 5);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![3, 4, 5, 6, 7]);
    let values: Vec<i32> = [3, 4, 5, 6, 7].iter().map(|key| cache.get(key).unwrap()).collect();
    assert_eq!(values, vec![3, 4, 5, 6, 7]);
}

#[test]
fn linked_map_lru_cache_get() {
    let cache = LRUCache::<i32, i32>::new(5, 1.0);

    [1, 2, 3, 4, 5].iter().for_each(|&value| {
        cache.set(value, value);
    });
    assert_eq!(cache.size(), 5);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![1, 2, 3, 4, 5]);
    cache.get(&3);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![1, 2, 4, 5, 3]);
    cache.peek(&4);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![1, 2, 4, 5, 3]);
    let values: Vec<i32> = [1, 2, 3, 4, 5].iter().map(|key| cache.get(key).unwrap()).collect();
    assert_eq!(values, vec![1, 2, 3, 4, 5]);
}

#[test]
fn linked_map_lru_cache_limit() {
    let cache = LRUCache::<i32, i32>::new(10, 1.0);

    for i in 1..=10 {
        cache.set(i, i);
    }
    assert_eq!(cache.size(), 10);
    cache.set_limit(5);
    assert_eq!(cache.size(), 5);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![6, 7, 8, 9, 10]);
    cache.set_limit(20);
    assert_eq!(cache.size(), 5);
    for i in 11..=20 {
        cache.set(i, i);
    }
    assert_eq!(cache.size(), 15);
    let mut values = Vec::new();
    for i in 6..=20 {
        values.push(cache.get(&i).unwrap());
        assert_eq!(cache.get(&i), Some(i));
    }
    assert_eq!(cache.values().collect::<Vec<_>>(), values);
}

#[test]
fn linked_map_lru_cache_limit_with_ratio() {
    let cache = LRUCache::<i32, i32>::new(10, 0.5);

    for i in 1..=10 {
        cache.set(i, i);
    }
    assert_eq!(cache.size(), 10);
    cache.set(11, 11);
    assert_eq!(cache.size(), 5);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![7, 8, 9, 10, 11]);
    let values: Vec<i32> = cache.keys().collect::<Vec<_>>().iter().map(|key| cache.get(key).unwrap()).collect();
    assert_eq!(values, vec![7, 8, 9, 10, 11]);
    assert_eq!(cache.values().collect::<Vec<_>>(), values);
}

#[test]
fn linked_map_mru_cache_simple() {
    let cache = MRUCache::<i32, i32>::new(5, 1.0);

    [1, 2, 3, 4, 5].iter().for_each(|&value| {
        cache.set(value, value);
    });
    assert_eq!(cache.size(), 5);
    cache.set(6, 6);
    assert_eq!(cache.size(), 5);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![1, 2, 3, 4, 6]);
    cache.set(7, 7);
    assert_eq!(cache.size(), 5);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![1, 2, 3, 4, 7]);
    let values: Vec<i32> = [1, 2, 3, 4, 7].iter().map(|key| cache.get(key).unwrap()).collect();
    assert_eq!(values, vec![1, 2, 3, 4, 7]);
}

#[test]
fn linked_map_mru_cache_get() {
    let cache = MRUCache::<i32, i32>::new(5, 1.0);

    [1, 2, 3, 4, 5].iter().for_each(|&value| {
        cache.set(value, value);
    });
    assert_eq!(cache.size(), 5);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![1, 2, 3, 4, 5]);
    cache.get(&3);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![1, 2, 4, 5, 3]);
    cache.peek(&4);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![1, 2, 4, 5, 3]);
    let values: Vec<i32> = [1, 2, 3, 4, 5].iter().map(|key| cache.get(key).unwrap()).collect();
    assert_eq!(values, vec![1, 2, 3, 4, 5]);
}

#[test]
fn linked_map_mru_cache_limit_with_ratio() {
    let cache = MRUCache::<i32, i32>::new(10, 0.5);

    for i in 1..=10 {
        cache.set(i, i);
    }
    assert_eq!(cache.size(), 10);
    cache.set(11, 11);
    assert_eq!(cache.size(), 5);
    assert_eq!(cache.keys().collect::<Vec<_>>(), vec![1, 2, 3, 4, 11]);
    let values: Vec<i32> = cache.keys().collect::<Vec<_>>().iter().map(|key| cache.get(key).unwrap()).collect();
    assert_eq!(values, vec![1, 2, 3, 4, 11]);
    assert_eq!(cache.values().collect::<Vec<_>>(), values);
}

#[test]
fn linked_map_to_json_from_json() {
    let mut map = LinkedMap::<String, String>::new();
    map.set("ak".into(), "av".into(), Touch::None);
    map.set("bk".into(), "bv".into(), Touch::None);
    map.set("ck".into(), "cv".into(), Touch::None);

    let json = map.to_json();
    map = LinkedMap::<String, String>::new();
    map.from_json(json);

    let mut i = 0;
    map.for_each(|value, key, _| {
        if i == 0 {
            assert_eq!(key, "ak");
            assert_eq!(value, "av");
        } else if i == 1 {
            assert_eq!(key, "bk");
            assert_eq!(value, "bv");
        } else if i == 2 {
            assert_eq!(key, "ck");
            assert_eq!(value, "cv");
        }
        i += 1;
    });
}

#[test]
fn linked_map_delete_head_and_tail() {
    let map = LinkedMap::<String, i32>::new();

    assert_eq!(map.size(), 0);

    map.set("1".into(), 1, Touch::None);
    assert_eq!(map.size(), 1);
    map.delete(&"1".to_string());
    assert_eq!(map.get(&"1".to_string(), Touch::None), None);
    assert_eq!(map.size(), 0);
    assert_eq!(map.keys().count(), 0);
}

#[test]
fn linked_map_delete_head() {
    let map = LinkedMap::<String, i32>::new();

    assert_eq!(map.size(), 0);

    map.set("1".into(), 1, Touch::None);
    map.set("2".into(), 2, Touch::None);
    assert_eq!(map.size(), 2);
    map.delete(&"1".to_string());
    assert_eq!(map.get(&"2".to_string(), Touch::None), Some(2));
    assert_eq!(map.size(), 1);
    assert_eq!(map.keys().count(), 1);
    assert_eq!(map.keys().next().unwrap(), "2");
}

#[test]
fn linked_map_delete_tail() {
    let map = LinkedMap::<String, i32>::new();

    assert_eq!(map.size(), 0);

    map.set("1".into(), 1, Touch::None);
    map.set("2".into(), 2, Touch::None);
    assert_eq!(map.size(), 2);
    map.delete(&"2".to_string());
    assert_eq!(map.get(&"1".to_string(), Touch::None), Some(1));
    assert_eq!(map.size(), 1);
    assert_eq!(map.keys().count(), 1);
    assert_eq!(map.keys().next().unwrap(), "1");
}

#[test]
fn resource_map_basics() {
    let mut map = ResourceMap::<Val>::new();

    let resource1 = parse("some://1");
    let resource2 = parse("some://2");
    let resource3 = parse("some://3");
    let resource4 = parse("some://4");
    let resource5 = parse("some://5");
    let resource6 = parse("some://6");

    assert_eq!(map.size(), 0);

    let res = map.set(resource1.clone(), Val::Num(1)) as *const ResourceMap<Val>;
    assert!(std::ptr::eq(res, &map));
    map.set(resource2.clone(), Val::Str("2".into()));
    map.set(resource3.clone(), Val::Bool(true));

    let values: Vec<Val> = map.values().cloned().collect();
    assert_eq!(values[0], Val::Num(1));
    assert_eq!(values[1], Val::Str("2".into()));
    assert_eq!(values[2], Val::Bool(true));

    let mut counter = 0;
    map.for_each(|value, key, map_obj| {
        assert_eq!(*value, values[counter]);
        counter += 1;
        assert!(URI::is_uri(key));
        assert!(std::ptr::eq(map_obj, &map));
    });

    let obj = Val::Obj(1);
    map.set(resource4.clone(), obj.clone());

    let date = Val::Num(1_700_000_000_000);
    map.set(resource5.clone(), date.clone());

    assert_eq!(map.size(), 5);
    assert_eq!(map.get(&resource1), Some(&Val::Num(1)));
    assert_eq!(map.get(&resource2), Some(&Val::Str("2".into())));
    assert_eq!(map.get(&resource3), Some(&Val::Bool(true)));
    assert_eq!(map.get(&resource4), Some(&obj));
    assert_eq!(map.get(&resource5), Some(&date));
    assert!(map.get(&resource6).is_none());

    map.delete(&resource6);
    assert_eq!(map.size(), 5);
    assert!(map.delete(&resource1));
    assert!(map.delete(&resource2));
    assert!(map.delete(&resource3));
    assert!(map.delete(&resource4));
    assert!(map.delete(&resource5));

    assert_eq!(map.size(), 0);
    assert!(map.get(&resource5).is_none());
    assert!(map.get(&resource4).is_none());
    assert!(map.get(&resource3).is_none());
    assert!(map.get(&resource2).is_none());
    assert!(map.get(&resource1).is_none());

    map.set(resource1.clone(), Val::Num(1));
    map.set(resource2.clone(), Val::Str("2".into()));
    map.set(resource3.clone(), Val::Bool(true));

    assert!(map.has(&resource1));
    assert_eq!(map.get(&resource1), Some(&Val::Num(1)));
    assert_eq!(map.get(&resource2), Some(&Val::Str("2".into())));
    assert_eq!(map.get(&resource3), Some(&Val::Bool(true)));

    map.clear();

    assert_eq!(map.size(), 0);
    assert!(map.get(&resource1).is_none());
    assert!(map.get(&resource2).is_none());
    assert!(map.get(&resource3).is_none());
    assert!(!map.has(&resource1));

    map.set(resource1.clone(), Val::Bool(false));
    map.set(resource2.clone(), Val::Num(0));

    assert!(map.has(&resource1));
    assert!(map.has(&resource2));
}

#[test]
fn resource_map_files_do_not_ignorecase() {
    let mut map = ResourceMap::<String>::new();

    let file_a = parse("file://some/filea");
    let file_b = parse("some://some/other/fileb");
    let file_a_upper = parse("file://SOME/FILEA");

    map.set(file_a.clone(), "true".into());
    assert_eq!(map.get(&file_a).map(String::as_str), Some("true"));

    assert!(map.get(&file_a_upper).is_none());

    assert!(map.get(&file_b).is_none());

    map.set(file_a_upper.clone(), "false".into());
    assert_eq!(map.get(&file_a_upper).map(String::as_str), Some("false"));

    assert_eq!(map.get(&file_a).map(String::as_str), Some("true"));

    let windows_file = file("c:\\test with %25\\c#code");
    let unc_file = file("\\\\shäres\\path\\c#\\plugin.json");

    map.set(windows_file.clone(), "true".into());
    map.set(unc_file.clone(), "true".into());

    assert_eq!(map.get(&windows_file).map(String::as_str), Some("true"));
    assert_eq!(map.get(&unc_file).map(String::as_str), Some("true"));
}

#[test]
fn resource_map_files_ignorecase() {
    let mut map = ResourceMap::<String>::with_key_fn(ignore_case_key_fn());

    let file_a = parse("file://some/filea");
    let file_b = parse("some://some/other/fileb");
    let file_a_upper = parse("file://SOME/FILEA");

    map.set(file_a.clone(), "true".into());
    assert_eq!(map.get(&file_a).map(String::as_str), Some("true"));

    assert_eq!(map.get(&file_a_upper).map(String::as_str), Some("true"));

    assert!(map.get(&file_b).is_none());

    map.set(file_a_upper.clone(), "false".into());
    assert_eq!(map.get(&file_a_upper).map(String::as_str), Some("false"));

    assert_eq!(map.get(&file_a).map(String::as_str), Some("false"));

    let windows_file = file("c:\\test with %25\\c#code");
    let unc_file = file("\\\\shäres\\path\\c#\\plugin.json");

    map.set(windows_file.clone(), "true".into());
    map.set(unc_file.clone(), "true".into());

    assert_eq!(map.get(&windows_file).map(String::as_str), Some("true"));
    assert_eq!(map.get(&unc_file).map(String::as_str), Some("true"));
}

#[test]
fn resource_map_files_ignorecase_but_preservecase() {
    let mut map = ResourceMap::<i32>::with_key_fn(ignore_case_key_fn());

    let file_a = parse("file://some/filea");
    let file_a_upper = parse("file://SOME/FILEA");

    map.set(file_a.clone(), 1);
    assert_eq!(map.get(&file_a), Some(&1));
    assert_eq!(map.get(&file_a_upper), Some(&1));
    let keys: Vec<String> = map.keys().map(|uri| uri.to_string(false).to_string_lossy()).collect();
    assert_eq!(keys, vec![file_a.to_string(false).to_string_lossy()]);
    assert_eq!(map.entries().collect::<Vec<_>>(), vec![(&file_a, &1)]);

    map.set(file_a_upper.clone(), 1);
    assert_eq!(map.get(&file_a), Some(&1));
    assert_eq!(map.get(&file_a_upper), Some(&1));
    let keys: Vec<String> = map.keys().map(|uri| uri.to_string(false).to_string_lossy()).collect();
    assert_eq!(keys, vec![file_a_upper.to_string(false).to_string_lossy()]);
    assert_eq!(map.entries().collect::<Vec<_>>(), vec![(&file_a_upper, &1)]);
}

#[test]
fn maps_strict_equal_ignore_order_case() {
    let mut map1 = OrderedMap::<String, String>::new();
    let mut map2 = OrderedMap::<String, String>::new();

    assert!(maps_strict_equal_ignore_order(&map1, &map2));

    map1.insert("foo".into(), "bar".into());
    assert!(!maps_strict_equal_ignore_order(&map1, &map2));

    map2.insert("foo".into(), "bar".into());
    assert!(maps_strict_equal_ignore_order(&map1, &map2));

    map2.insert("bar".into(), "foo".into());
    assert!(!maps_strict_equal_ignore_order(&map1, &map2));

    map1.insert("bar".into(), "foo".into());
    assert!(maps_strict_equal_ignore_order(&map1, &map2));
}

fn three_entry_bimap() -> BidirectionalMap<String, i32> {
    let mut map = BidirectionalMap::<String, i32>::new();
    map.set("one".into(), 1);
    map.set("two".into(), 2);
    map.set("three".into(), 3);
    map
}

#[test]
fn bidirectional_map_should_set_and_get_values_correctly() {
    let map = three_entry_bimap();

    assert_eq!(map.get(&"one".to_string()), Some(&1));
    assert_eq!(map.get(&"two".to_string()), Some(&2));
    assert_eq!(map.get(&"three".to_string()), Some(&3));
}

#[test]
fn bidirectional_map_should_get_keys_by_value_correctly() {
    let map = three_entry_bimap();

    assert_eq!(map.get_key(&1).map(String::as_str), Some("one"));
    assert_eq!(map.get_key(&2).map(String::as_str), Some("two"));
    assert_eq!(map.get_key(&3).map(String::as_str), Some("three"));
}

#[test]
fn bidirectional_map_should_delete_values_correctly() {
    let mut map = three_entry_bimap();

    assert!(map.delete(&"one".to_string()));
    assert_eq!(map.get(&"one".to_string()), None);
    assert_eq!(map.get_key(&1), None);

    assert!(map.delete(&"two".to_string()));
    assert_eq!(map.get(&"two".to_string()), None);
    assert_eq!(map.get_key(&2), None);

    assert!(map.delete(&"three".to_string()));
    assert_eq!(map.get(&"three".to_string()), None);
    assert_eq!(map.get_key(&3), None);
}

#[test]
fn bidirectional_map_should_handle_non_existent_keys_correctly() {
    let mut map = three_entry_bimap();

    assert_eq!(map.get(&"four".to_string()), None);
    assert_eq!(map.get_key(&4), None);
    assert!(!map.delete(&"four".to_string()));
}

#[test]
fn bidirectional_map_should_not_leave_a_stale_reverse_entry_when_a_key_value_is_updated() {
    let mut map = BidirectionalMap::<String, i32>::new();
    map.set("one".into(), 1);
    map.set("one".into(), 2);

    assert_eq!(map.get(&"one".to_string()), Some(&2));
    assert_eq!(map.get_key(&2).map(String::as_str), Some("one"));
    assert_eq!(map.get_key(&1), None);
}

#[test]
fn bidirectional_map_should_handle_for_each_correctly() {
    let map = three_entry_bimap();

    let mut keys: Vec<String> = Vec::new();
    let mut values: Vec<i32> = Vec::new();
    map.for_each(|value, key, _| {
        keys.push(key.clone());
        values.push(*value);
    });

    assert_eq!(keys, strs(&["one", "two", "three"]));
    assert_eq!(values, vec![1, 2, 3]);
}

#[test]
fn bidirectional_map_should_handle_clear_correctly() {
    let mut map = three_entry_bimap();

    map.clear();

    assert_eq!(map.get(&"one".to_string()), None);
    assert_eq!(map.get(&"two".to_string()), None);
    assert_eq!(map.get(&"three".to_string()), None);
    assert_eq!(map.get_key(&1), None);
    assert_eq!(map.get_key(&2), None);
    assert_eq!(map.get_key(&3), None);
}

#[test]
fn set_map_add_and_get() {
    let mut set_map = SetMap::<String, i32>::new();
    set_map.add("a".into(), 1);
    set_map.add("a".into(), 2);
    set_map.add("b".into(), 3);
    assert_eq!(set_map.get(&"a".to_string()), vec![&1, &2]);
    assert_eq!(set_map.get(&"b".to_string()), vec![&3]);
}

#[test]
fn set_map_delete() {
    let mut set_map = SetMap::<String, i32>::new();
    set_map.add("a".into(), 1);
    set_map.add("a".into(), 2);
    set_map.add("b".into(), 3);
    set_map.delete(&"a".to_string(), &1);
    assert_eq!(set_map.get(&"a".to_string()), vec![&2]);
    set_map.delete(&"a".to_string(), &2);
    assert_eq!(set_map.get(&"a".to_string()), Vec::<&i32>::new());
}

#[test]
fn set_map_for_each() {
    let mut set_map = SetMap::<String, i32>::new();
    set_map.add("a".into(), 1);
    set_map.add("a".into(), 2);
    set_map.add("b".into(), 3);
    let mut sum = 0;
    set_map.for_each(&"a".to_string(), |value| sum += value);
    assert_eq!(sum, 3);
}

#[test]
fn set_map_get_empty_set() {
    let set_map = SetMap::<String, i32>::new();
    assert_eq!(set_map.get(&"a".to_string()), Vec::<&i32>::new());
}

fn keys(parts: &[&str]) -> Vec<NKey> {
    parts.iter().map(|&part| NKey::from(part)).collect()
}

fn four_key_map() -> NKeyMap<i32> {
    let mut map = NKeyMap::<i32>::new();
    map.set(1, &keys(&["a", "b", "c", "d"]));
    map.set(2, &keys(&["a", "c", "c", "d"]));
    map.set(3, &keys(&["b", "e", "f", "g"]));
    map
}

#[test]
fn n_key_map_set_and_get() {
    let map = four_key_map();
    assert_eq!(map.get(&keys(&["a", "b", "c", "d"])), Some(&1));
    assert_eq!(map.get(&keys(&["a", "c", "c", "d"])), Some(&2));
    assert_eq!(map.get(&keys(&["b", "e", "f", "g"])), Some(&3));
    assert_eq!(map.get(&keys(&["a", "b", "c", "a"])), None);
}

#[test]
fn n_key_map_clear() {
    let mut map = four_key_map();
    map.clear();
    assert_eq!(map.get(&keys(&["a", "b", "c", "d"])), None);
    assert_eq!(map.get(&keys(&["a", "c", "c", "d"])), None);
    assert_eq!(map.get(&keys(&["b", "e", "f", "g"])), None);
}

#[test]
fn n_key_map_values() {
    let map = four_key_map();
    assert_eq!(map.values(), vec![&1, &2, &3]);
}

fn three_key_map(entries: &[(i32, [&str; 3])]) -> NKeyMap<i32> {
    let mut map = NKeyMap::<i32>::new();
    for (value, parts) in entries {
        map.set(*value, &keys(parts));
    }
    map
}

#[test]
fn n_key_map_get_all() {
    let map = three_key_map(&[(1, ["a", "b", "c"]), (2, ["a", "b", "d"]), (3, ["a", "e", "f"]), (4, ["g", "h", "i"])]);
    assert_eq!(map.get_all(&keys(&["a", "b"])), vec![&1, &2]);
    assert_eq!(map.get_all(&keys(&["a"])), vec![&1, &2, &3]);
    assert_eq!(map.get_all(&keys(&["missing"])), Vec::<&i32>::new());
}

#[test]
fn n_key_map_delete() {
    let mut map = three_key_map(&[(1, ["a", "b", "c"]), (2, ["a", "b", "d"]), (3, ["x", "y", "z"])]);
    assert!(map.delete(&keys(&["a", "b", "c"])));
    assert!(!map.delete(&keys(&["a", "b", "c"])));
    assert_eq!(map.values(), vec![&2, &3]);
}

#[test]
fn n_key_map_delete_all() {
    let mut map = three_key_map(&[(1, ["a", "b", "c"]), (2, ["a", "b", "d"]), (3, ["a", "e", "f"]), (4, ["g", "h", "i"])]);
    assert!(map.delete_all(&keys(&["a", "b"])));
    assert_eq!(map.values(), vec![&3, &4]);
    assert!(!map.delete_all(&keys(&["missing"])));
    assert!(map.delete_all(&[]));
    assert_eq!(map.values(), Vec::<&i32>::new());
}

#[test]
fn n_key_map_delete_all_cleans_empty_parent_maps() {
    let mut map = three_key_map(&[(1, ["a", "b", "c"]), (2, ["x", "y", "z"])]);
    assert!(map.delete_all(&keys(&["a", "b"])));
    assert!(!map.delete_all(&keys(&["a"])));
    assert_eq!(map.values(), vec![&2]);
}

#[test]
fn n_key_map_to_string() {
    let map = three_key_map(&[(1, ["f", "o", "o"]), (2, ["b", "a", "r"]), (3, ["b", "a", "z"]), (3, ["b", "o", "o"])]);
    assert_eq!(
        map.to_string(),
        ["f: ", "  o: ", "    o: 1", "b: ", "  a: ", "    r: 2", "    z: 3", "  o: ", "    o: 3", ""].join("\n")
    );
}
