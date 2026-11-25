//! A standard library read from a table: one type per line, tab-separated: package (segments
//! joined by `.`), name (nested types joined by `.`), kind (`c`lass, `i`nterface, `e`num), type
//! parameters (joined by `,`) and container (`-` none, `C` collection with `o`rdered and/or
//! `u`nique, `M` map, `O` optional, `W` owning).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::ir::TypeKind;
use crate::lang::{Container, LibraryType};

pub struct TableLibrary {
    text: &'static str,
    table: OnceLock<Table>,
}

struct Table {
    /// By package, then by name (nested types joined by `.`).
    types: HashMap<String, HashMap<String, LibraryType>>,
    /// Every package and every package prefix (`java`, `java.util`).
    packages: HashSet<String>,
}

impl TableLibrary {
    pub const fn new(text: &'static str) -> Self {
        Self { text, table: OnceLock::new() }
    }

    fn table(&self) -> &Table {
        self.table.get_or_init(|| {
            let mut types: HashMap<String, HashMap<String, LibraryType>> = HashMap::new();
            let mut packages = HashSet::new();
            for line in self.text.lines().filter(|l| !l.is_empty()) {
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
                    "W" => Container::Owning,
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

    /// The type `name` (nested types as several segments) in `package`.
    pub fn get(&self, package: &[String], name: &[String]) -> Option<&LibraryType> {
        self.table().types.get(&package.join("."))?.get(&name.join("."))
    }

    /// Whether `path` names a package of the library.
    pub fn is_package(&self, path: &[String]) -> bool {
        self.table().packages.contains(&path.join("."))
    }
}
