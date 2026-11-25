//! The public types of the JDK, from `jdk.txt`, which `tools/jdk-types/JdkTypes.java` writes.

use crate::library::TableLibrary;

pub static JDK: TableLibrary = TableLibrary::new(include_str!("jdk.txt"));

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::TypeKind;
    use crate::lang::Container;

    fn s(path: &[&str]) -> Vec<String> {
        path.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn knows_collections_and_packages() {
        let list = JDK.get(&s(&["java", "util"]), &s(&["List"])).unwrap();
        assert_eq!(list.kind, TypeKind::Interface);
        assert_eq!(list.parameters, ["E"]);
        assert_eq!(list.container, Container::Collection { ordered: true, unique: false });
        let entry = JDK.get(&s(&["java", "util"]), &s(&["Map", "Entry"])).unwrap();
        assert_eq!(entry.parameters, ["K", "V"]);
        assert_eq!(JDK.get(&s(&["java", "util"]), &s(&["Map"])).unwrap().container, Container::Map);
        assert!(JDK.get(&s(&["java", "lang"]), &s(&["String"])).is_some());
        assert!(JDK.is_package(&s(&["java"])));
        assert!(JDK.is_package(&s(&["java", "util", "concurrent"])));
        assert!(!JDK.is_package(&s(&["java", "util", "List"])));
    }
}
