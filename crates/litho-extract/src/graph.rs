use litho_core::types::Dependency;
use std::collections::HashMap;

/// Build an adjacency-list dependency graph from per-file dependency lists.
///
/// The returned map has the form `file_path -> [dependency_target, ...]`.
pub fn build_dependency_graph(
    all_deps: &[(String, Vec<Dependency>)],
) -> HashMap<String, Vec<String>> {
    all_deps
        .iter()
        .map(|(file_path, deps)| {
            let targets = deps.iter().map(|d| d.target.clone()).collect();
            (file_path.clone(), targets)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use litho_core::types::Dependency;

    #[test]
    fn empty_input_yields_empty_graph() {
        let graph = build_dependency_graph(&[]);
        assert!(graph.is_empty());
    }

    #[test]
    fn single_file_with_deps() {
        let deps = vec![
            Dependency {
                source: "src/main.rs".into(),
                target: "std::io".into(),
                kind: "use".into(),
            },
            Dependency {
                source: "src/main.rs".into(),
                target: "crate::util".into(),
                kind: "use".into(),
            },
        ];
        let input = vec![("src/main.rs".to_string(), deps)];
        let graph = build_dependency_graph(&input);
        assert_eq!(graph["src/main.rs"].len(), 2);
        assert!(graph["src/main.rs"].contains(&"std::io".to_string()));
    }
}
