use convert_case::{Case, Casing};

/// Util function that convert a rust module path to the ClassPath used in Fitnesse.
pub fn from_rust_module_path_to_class_path(rust_module_path: &str) -> String {
    join(
        rust_module_path
            .split("::")
            .map(|s| s.to_case(Case::Pascal)),
        ".",
    )
}

fn join<T: AsRef<str>>(iterator: impl Iterator<Item = T>, separator: &str) -> String {
    let mut result = String::new();
    let mut first = true;
    for part in iterator {
        if !first {
            result += separator;
        }
        result += part.as_ref();
        first = false
    }
    result
}
