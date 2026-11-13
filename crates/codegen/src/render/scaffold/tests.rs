//! Verifies that module registries import implementations without including source files.

use super::{collect_modules, render_registry};
use crate::writer::GeneratedFile;

#[test]
fn registration_modules_are_packed_to_bound_aggregate_file_size() {
    let files = (0..65_000)
        .map(|index| GeneratedFile {
            name: format!("types/owner{index:05}.rs"),
            contents: String::new(),
            exports_public_items: true,
        })
        .collect::<Vec<_>>();
    let registry = render_registry(&collect_modules(&files).unwrap());
    assert_eq!(registry.name, "modules.rs");
    assert_eq!(registry.contents.matches("#[path = ").count(), 65_000);
    assert!(registry.contents.contains("types/owner64999.rs"));
    assert!(registry.contents.lines().count() > 400);
}

#[test]
fn bridge_and_dispatch_implementations_do_not_generate_unused_glob_imports() {
    for path in ["internal/bridge/dispatch.rs", "internal/dispatch/book.rs"] {
        let files = [GeneratedFile {
            name: path.to_owned(),
            contents: "impl Example for Target {}\n".to_owned(),
            exports_public_items: false,
        }];
        let registry = render_registry(&collect_modules(&files).unwrap());
        assert!(registry.contents.contains(path));
        assert!(!registry.contents.contains("use "));
    }
}

#[test]
fn separates_private_declarations_from_public_re_exports() {
    for public in [false, true] {
        let files = [GeneratedFile {
            name: "types/book.rs".to_owned(),
            contents: "pub struct Book;\n".to_owned(),
            exports_public_items: public,
        }];
        let registry = render_registry(&collect_modules(&files).unwrap());
        assert!(registry.contents.contains("use __chigiri_types_pbook::*;"));
        assert_eq!(registry.contents.contains("pub use "), public);
    }
}

#[test]
fn paths_and_underscores_map_to_distinct_private_module_names() {
    let files = [
        "internal/owner_a.rs",
        "internal_owner/a.rs",
        "types/__chigiri_name_self.rs",
    ]
    .map(|path| GeneratedFile {
        name: path.to_owned(),
        contents: String::new(),
        exports_public_items: false,
    });
    let modules = collect_modules(&files).unwrap();
    assert_eq!(modules.len(), 3);
    assert_ne!(modules[0].name, modules[1].name);
    assert!(
        modules
            .iter()
            .all(|module| !module.name[2..].contains("__"))
    );
}
