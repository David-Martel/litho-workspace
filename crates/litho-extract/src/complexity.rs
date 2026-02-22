use litho_core::types::{ComplexityMetrics, Language};

/// Compute approximate cyclomatic complexity and basic LOC metrics.
///
/// The complexity score is a keyword-count heuristic: one point per branch
/// keyword (if, for, while, match, etc.) plus a base of 1.  This avoids the
/// overhead of a full AST walk for metric-only purposes while still producing
/// useful relative rankings.
///
/// The `functions` and `classes` fields are initialised to 0 here; the caller
/// is expected to fill them in from the interface list after extraction.
pub fn compute_complexity(content: &str, language: &Language) -> ComplexityMetrics {
    let _loc = content.lines().count();
    let non_blank = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();

    let branch_keywords: &[&str] = match language {
        Language::Rust => &[
            " if ", " for ", " while ", " match ", " else if ", "else {", "?",
        ],
        Language::Python => &[
            " if ", " for ", " while ", " elif ", " except ", " with ",
        ],
        Language::TypeScript | Language::JavaScript => &[
            " if ", " for ", " while ", " switch ", " catch ", " else if ",
            " case ", " ? ", "&&", "||",
        ],
        Language::CSharp => &[
            " if ", " for ", " foreach ", " while ", " switch ", " catch ",
            " case ", " else if ",
        ],
        Language::Java => &[
            " if ", " for ", " while ", " switch ", " catch ", " case ",
            " else if ",
        ],
        Language::Go => &[
            " if ", " for ", " switch ", " select ", " case ", " else if ",
        ],
        _ => &[" if ", " for ", " while ", " switch ", " case "],
    };

    let branches: usize = branch_keywords
        .iter()
        .map(|kw| content.matches(kw).count())
        .sum();

    ComplexityMetrics {
        cyclomatic: (branches + 1) as f64,
        lines_of_code: non_blank,
        // Filled in by the caller once interfaces are extracted.
        functions: 0,
        classes: 0,
    }
}

/// Total lines including blanks and comments (used for `ExtractedFile::lines_of_code`).
pub fn total_lines(content: &str) -> usize {
    content.lines().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_has_complexity_one() {
        let m = compute_complexity("", &Language::Rust);
        assert_eq!(m.cyclomatic, 1.0);
        assert_eq!(m.lines_of_code, 0);
    }

    #[test]
    fn if_branch_increments_complexity() {
        let code = "fn foo() {\n    if x > 0 {\n        bar();\n    }\n}";
        let m = compute_complexity(code, &Language::Rust);
        assert!(m.cyclomatic > 1.0);
    }
}
