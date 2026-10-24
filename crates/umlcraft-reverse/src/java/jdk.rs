//! The public types of the JDK, from `jdk.txt`, which `tools/jdk-types/JdkTypes.java` writes.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::ir::TypeKind;
use crate::lang::{Container, Library, LibraryType};

const TABLE: &str = include_str!("jdk.txt");

pub struct Jdk;

struct Table {
    /// By package, then by name (nested types joined by `.`).
    types: HashMap<String, HashMap<String, LibraryType>>,
    /// Every package and every package prefix (`java`, `java.util`).
    packages: HashSet<String>,
}

fn table() -> &'static Table {
    static TABLE_CELL: OnceLock<Table> = OnceLock::new();
    TABLE_CELL.get_or_init(|| {
        let mut types: HashMap<String, HashMap<String, LibraryType>> = HashMap::new();
        let mut packages = HashSet::new();
        for line in TABLE.lines().filter(|l| !l.is_empty()) {
            let fields: Vec<&str> = line.split('\t').collect();
            let [package, name, kind, parameters, container] = fields[..] else { continue };
            let kind = match kind {
                "i" => TypeKind::Interface,
                "e" => TypeKind::Enum,
                _ => TypeKind::Class,
            };
            let container = match container {
                "M" => Container::Map,
                "O" => Container::Optional,
                c if c.starts_with('C') => {
                    Container::Collection { ordered: c.contains('o'), unique: c.contains('u') }
                }
                _ => Container::None,
            };
            let parameters = parameters.split(',').filter(|p| !p.is_empty()).map(str::to_owned);
            let entry = LibraryType { kind, parameters: parameters.collect(), container };
            types.entry(package.to_owned()).or_default().insert(name.to_owned(), entry);
            let mut prefix = String::new();
            for segment in package.split('.') {
                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(segment);
                packages.insert(prefix.clone());
            }
        }
        Table { types, packages }
    })
}

impl Library for Jdk {
    fn get(&self, package: &[String], name: &[String]) -> Option<&LibraryType> {
        table().types.get(&package.join("."))?.get(&name.join("."))
    }

    fn is_package(&self, path: &[String]) -> bool {
        table().packages.contains(&path.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(path: &[&str]) -> Vec<String> {
        path.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn knows_collections_and_packages() {
        let list = Jdk.get(&s(&["java", "util"]), &s(&["List"])).unwrap();
        assert_eq!(list.kind, TypeKind::Interface);
        assert_eq!(list.parameters, ["E"]);
        assert_eq!(list.container, Container::Collection { ordered: true, unique: false });
        let entry = Jdk.get(&s(&["java", "util"]), &s(&["Map", "Entry"])).unwrap();
        assert_eq!(entry.parameters, ["K", "V"]);
        assert_eq!(Jdk.get(&s(&["java", "util"]), &s(&["Map"])).unwrap().container, Container::Map);
        assert!(Jdk.get(&s(&["java", "lang"]), &s(&["String"])).is_some());
        assert!(Jdk.is_package(&s(&["java"])));
        assert!(Jdk.is_package(&s(&["java", "util", "concurrent"])));
        assert!(!Jdk.is_package(&s(&["java", "util", "List"])));
    }
}
