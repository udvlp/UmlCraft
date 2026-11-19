//! What tells the structogram of a method on a new import: a key of the file's path, the
//! classes and the signature, `shop/orders/Order.java:Order:void add(Item item)`. The path
//! starts with the folder imported (it is taken from the folder above the files, see
//! [`key_base`]), so that files of the same name in other folders, and the same file
//! imported from a folder further up or down, or from another place on the disk, are told
//! apart and found again. A key matches a file whose path ends with the key's path: keys of
//! UmlCraft 0.22.0, which held the file's name only, match too, and are replaced when found.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crate::MethodRef;

/// The folder the paths in keys start from: the one above the folder that holds all `files`
/// (so the paths start with that folder's name).
pub fn key_base(files: &[PathBuf]) -> PathBuf {
    let mut common: Option<PathBuf> = None;
    for file in files {
        let folder = file.parent().unwrap_or(Path::new(""));
        common = Some(match common {
            None => folder.to_owned(),
            Some(c) => c
                .components()
                .zip(folder.components())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect(),
        });
    }
    let common = common.unwrap_or_default();
    common.parent().map(Path::to_owned).unwrap_or(common)
}

/// The key of `method` of the file at `path`, its path taken from `base`.
pub fn method_key(base: &Path, path: &Path, method: &MethodRef) -> String {
    format!("{}{}", file_path(base, path), tail(method))
}

/// The path of the file at `path` in a key: from `base`, `/` between folders.
pub fn file_path(base: &Path, path: &Path) -> String {
    segments(path.strip_prefix(base).unwrap_or(path)).join("/")
}

/// The path a key names (what stands before the first `:`), `/` between folders.
pub fn key_path(key: &str) -> &str {
    key.split(':').next().unwrap_or_default()
}

/// Whether the file at `path` is the one `key` names: its path ends with the key's.
pub fn names_file(key: &str, path: &Path) -> bool {
    path_score(key_path(key), path).is_some()
}

/// Of `old` and `new`, keys of one method, the one with the longer path (which tells it
/// best); `new` where they are as long.
pub fn better_key(old: &str, new: &str) -> String {
    let length = |key: &str| key_path(key).split('/').count();
    if length(old) > length(new) { old.to_owned() } else { new.to_owned() }
}

/// For each of `methods` (with their files' paths) the key of `keys` that names it, if any:
/// a key names a method with its classes and signature whose file's path ends with the key's.
/// Where several keys name a method, the one with the longest path is taken; a key goes to one
/// method only, first to those `prefer` (method, key) pairs, then in the order given.
pub fn pair(
    keys: &[&str],
    methods: &[(&Path, &MethodRef)],
    prefer: impl Fn(usize, usize) -> bool,
) -> Vec<Option<usize>> {
    let mut by_tail: HashMap<&str, Vec<usize>> = HashMap::new();
    for (k, key) in keys.iter().enumerate() {
        by_tail.entry(&key[key_path(key).len()..]).or_default().push(k);
    }
    // (method, key, how many folders agree, preferred)
    let mut matches: Vec<(usize, usize, usize, bool)> = Vec::new();
    for (m, (path, method)) in methods.iter().enumerate() {
        let Some(candidates) = by_tail.get(tail(method).as_str()) else { continue };
        for &k in candidates {
            if let Some(score) = path_score(key_path(keys[k]), path) {
                matches.push((m, k, score, prefer(m, k)));
            }
        }
    }
    matches
        .sort_by(|a, b| b.2.cmp(&a.2).then(b.3.cmp(&a.3)).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
    let mut paired = vec![None; methods.len()];
    let mut taken = vec![false; keys.len()];
    for (m, k, _, _) in matches {
        if paired[m].is_none() && !taken[k] {
            paired[m] = Some(k);
            taken[k] = true;
        }
    }
    paired
}

/// `:Class:signature`.
fn tail(method: &MethodRef) -> String {
    format!(":{}:{}", method.class_name(), method.signature)
}

/// A path's names, without a drive or root.
fn segments(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// How many names the end of `path` shares with `key_path` (`a/b.py`) when it ends with all
/// of them; `None` when it does not.
fn path_score(key_path: &str, path: &Path) -> Option<usize> {
    let wanted: Vec<&str> = key_path.split('/').filter(|s| !s.is_empty()).collect();
    let names = segments(path);
    if wanted.is_empty() || wanted.len() > names.len() {
        return None;
    }
    let end = &names[names.len() - wanted.len()..];
    end.iter().zip(&wanted).all(|(a, b)| a == b).then_some(wanted.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method(class: &str, signature: &str) -> MethodRef {
        MethodRef {
            class: if class.is_empty() { Vec::new() } else { vec![class.to_owned()] },
            name: signature.split('(').next().unwrap_or_default().to_owned(),
            signature: signature.to_owned(),
            line: 1,
            start: 0,
            end: 1,
            module_code: false,
        }
    }

    #[test]
    fn keys_start_with_the_folder_imported() {
        let files =
            [PathBuf::from("/p/site/blog/views.py"), PathBuf::from("/p/site/shop/views.py")];
        let base = key_base(&files);
        assert_eq!(base, PathBuf::from("/p"));
        let index = method("", "index(request)");
        assert_eq!(method_key(&base, &files[0], &index), "site/blog/views.py::index(request)");
        let one = [PathBuf::from("/p/site/blog/views.py")];
        assert_eq!(method_key(&key_base(&one), &one[0], &index), "blog/views.py::index(request)");
    }

    #[test]
    fn same_named_files_keep_their_own_structograms() {
        let blog = PathBuf::from("/q/site/blog/views.py");
        let shop = PathBuf::from("/q/site/shop/views.py");
        let index = method("", "index(request)");
        let methods = [(blog.as_path(), &index), (shop.as_path(), &index)];
        // Made from /p, found again in /q; the shop's key first.
        let keys = ["site/shop/views.py::index(request)", "site/blog/views.py::index(request)"];
        assert_eq!(pair(&keys, &methods, |_, _| false), [Some(1), Some(0)]);
        // A file of its own name in another folder is not the same.
        let other = PathBuf::from("/q/other/views.py");
        assert_eq!(pair(&keys, &[(other.as_path(), &index)], |_, _| false), [None]);
        assert!(names_file(keys[0], &shop) && !names_file(keys[0], &blog));
    }

    #[test]
    fn keys_of_0_22_name_the_file_only_and_are_replaced() {
        let order = PathBuf::from("/p/shop/a/Order.java");
        let other = PathBuf::from("/p/shop/b/Order.java");
        let add = method("Order", "void add(Item item)");
        let methods = [(other.as_path(), &add), (order.as_path(), &add)];
        let keys = ["Order.java:Order:void add(Item item)"];
        // In order, or the one `prefer` names (its operation is the structogram's).
        assert_eq!(pair(&keys, &methods, |_, _| false), [Some(0), None]);
        assert_eq!(pair(&keys, &methods, |m, _| m == 1), [None, Some(0)]);
        // A key with a longer path is the better one.
        let keys =
            ["Order.java:Order:void add(Item item)", "a/Order.java:Order:void add(Item item)"];
        assert_eq!(pair(&keys, &methods, |_, _| false), [Some(0), Some(1)]);
        let new = method_key(&key_base(std::slice::from_ref(&order)), &order, &add);
        assert_eq!(better_key(keys[0], &new), "a/Order.java:Order:void add(Item item)");
        assert_eq!(better_key("x/y/a/Order.java:Order:f()", &new), "x/y/a/Order.java:Order:f()");
        assert_eq!(key_path("shop/Cart.cpp:shop::Cart:void add()"), "shop/Cart.cpp");
    }
}
