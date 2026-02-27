/// Token compression for source code sent to LLM prompts.
///
/// Strips comments and collapses whitespace to reduce per-file prompt token
/// counts by roughly 30–40 % without destroying the structural signal an LLM
/// needs to understand the code.
///
/// # Language support
///
/// | `language_hint` values | Comment syntax handled |
/// |------------------------|------------------------|
/// | `"rs"`, `"c"`, `"cpp"`, `"cs"`, `"java"`, `"js"`, `"ts"`, `"jsx"`, `"tsx"`, `"kt"`, `"swift"`, `None` | `//` line comments, `/* … */` block comments |
/// | `"py"` | `#` line comments, `""" … """` and `''' … '''` triple-quoted strings are kept as-is (they are part of the language semantics), only bare `#` comments are stripped |
///
/// String literals that contain comment-like sequences are **preserved** so
/// that `let s = "// not a comment"` is never mangled.
///
/// # Example
///
/// ```
/// use litho_generator::utils::token_compress::compress_source_for_llm;
///
/// let rust_src = r#"
/// // top-level comment
/// fn hello() {
///     let x = 1; // inline
///     /* block */ let y = 2;
/// }
/// "#;
///
/// let compressed = compress_source_for_llm(rust_src, Some("rs"));
/// assert!(!compressed.contains("top-level comment"));
/// assert!(!compressed.contains("inline"));
/// assert!(!compressed.contains("block"));
/// assert!(compressed.contains("fn hello()"));
/// assert!(compressed.contains("let x = 1;"));
/// assert!(compressed.contains("let y = 2;"));
/// ```
pub fn compress_source_for_llm(source: &str, language_hint: Option<&str>) -> String {
    let lang = language_hint.unwrap_or("").to_ascii_lowercase();
    let is_python = lang == "py";

    let stripped = if is_python {
        strip_python_comments(source)
    } else {
        strip_c_style_comments(source)
    };

    collapse_whitespace(&stripped)
}

// ---------------------------------------------------------------------------
// C-style comment stripping (Rust / TS / C# / Java / Kotlin / Swift / …)
// ---------------------------------------------------------------------------

/// Strip `//` line comments and `/* … */` block comments while leaving string
/// literals intact.
fn strip_c_style_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;

    while i < len {
        // Entering a double-quoted string literal
        if bytes[i] == b'"' {
            let (literal, consumed) = consume_string_literal(bytes, i, b'"');
            out.push_str(literal);
            i += consumed;
            continue;
        }

        // Entering a single-quoted char literal (Rust / C / C++ / C# / Java)
        if bytes[i] == b'\'' {
            let (literal, consumed) = consume_string_literal(bytes, i, b'\'');
            out.push_str(literal);
            i += consumed;
            continue;
        }

        // Entering a backtick template literal (JS / TS)
        if bytes[i] == b'`' {
            let (literal, consumed) = consume_backtick_literal(bytes, i);
            out.push_str(literal);
            i += consumed;
            continue;
        }

        // Potential comment start
        if i + 1 < len && bytes[i] == b'/' {
            // Line comment: //
            if bytes[i + 1] == b'/' {
                // Skip until end-of-line (keep the newline itself)
                i += 2;
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                // Consume trailing spaces left on the same line
                // (the newline is emitted below on the next iteration)
                continue;
            }

            // Block comment: /* … */
            if bytes[i + 1] == b'*' {
                i += 2;
                while i + 1 < len {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    // Preserve newlines inside block comments so line structure
                    // is maintained (avoids joining code lines).
                    if bytes[i] == b'\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
                continue;
            }
        }

        // Ordinary character
        // SAFETY: bytes[i] is valid UTF-8 since we iterate over a &str by byte
        // index, but we need to advance by the full Unicode scalar width.
        let ch = source[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

/// Consume a string literal delimited by `quote` (either `"` or `'`).
/// Returns `(literal_slice, bytes_consumed)`.
fn consume_string_literal<'a>(bytes: &'a [u8], start: usize, quote: u8) -> (&'a str, usize) {
    let mut i = start + 1; // skip opening quote
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escaped character
            continue;
        }
        if bytes[i] == quote {
            i += 1; // skip closing quote
            break;
        }
        i += 1;
    }
    let consumed = i - start;
    // SAFETY: we're slicing a valid UTF-8 &str at char boundaries because
    // string delimiters and backslash are all ASCII (single-byte).
    (
        std::str::from_utf8(&bytes[start..i]).unwrap_or(""),
        consumed,
    )
}

/// Consume a JS/TS backtick template literal (no escape analysis needed for
/// our purposes — we just skip to the matching unescaped backtick).
fn consume_backtick_literal(bytes: &[u8], start: usize) -> (&str, usize) {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'`' {
            i += 1;
            break;
        }
        i += 1;
    }
    let consumed = i - start;
    (
        std::str::from_utf8(&bytes[start..i]).unwrap_or(""),
        consumed,
    )
}

// ---------------------------------------------------------------------------
// Python comment stripping
// ---------------------------------------------------------------------------

/// Strip `#` comments from Python source while preserving triple-quoted
/// strings (which carry docstring semantics meaningful to LLMs).
fn strip_python_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;

    while i < len {
        // Triple-quoted strings: """ or '''
        if i + 2 < len
            && ((bytes[i] == b'"' && bytes[i + 1] == b'"' && bytes[i + 2] == b'"')
                || (bytes[i] == b'\'' && bytes[i + 1] == b'\'' && bytes[i + 2] == b'\''))
        {
            let delim = bytes[i];
            // Emit the entire triple-quoted block unchanged
            let end_marker = [delim, delim, delim];
            out.push(delim as char);
            out.push(delim as char);
            out.push(delim as char);
            i += 3;
            loop {
                if i + 2 < len
                    && bytes[i] == end_marker[0]
                    && bytes[i + 1] == end_marker[1]
                    && bytes[i + 2] == end_marker[2]
                {
                    out.push(delim as char);
                    out.push(delim as char);
                    out.push(delim as char);
                    i += 3;
                    break;
                }
                if i >= len {
                    break;
                }
                let ch = source[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
            continue;
        }

        // Single-quoted string: " or '
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            let (literal, consumed) = consume_string_literal(bytes, i, quote);
            out.push_str(literal);
            i += consumed;
            continue;
        }

        // Hash comment: skip to end of line
        if bytes[i] == b'#' {
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Ordinary character
        let ch = source[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

// ---------------------------------------------------------------------------
// Whitespace collapsing
// ---------------------------------------------------------------------------

/// Collapse consecutive blank lines to one and trim trailing spaces from each
/// line, while preserving leading indentation.
fn collapse_whitespace(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut blank_run = 0usize;

    for line in source.lines() {
        // Trim only trailing whitespace; leading indentation is meaningful.
        let trimmed = line.trim_end();

        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run == 1 {
                out.push('\n');
            }
            // Suppress additional consecutive blank lines.
        } else {
            blank_run = 0;
            out.push_str(trimmed);
            out.push('\n');
        }
    }

    // Strip a possible leading blank line introduced by the first newline in
    // multi-line raw string literals in tests.
    out.trim_start_matches('\n').to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Rust / C-style: line comments
    // -----------------------------------------------------------------------

    #[test]
    fn rust_line_comment_stripped() {
        let src = "fn foo() {} // this is a comment\n";
        let out = compress_source_for_llm(src, Some("rs"));
        assert!(!out.contains("this is a comment"));
        assert!(out.contains("fn foo()"));
    }

    #[test]
    fn rust_standalone_comment_line_removed() {
        let src = "// standalone comment\nfn bar() {}\n";
        let out = compress_source_for_llm(src, Some("rs"));
        assert!(!out.contains("standalone comment"));
        assert!(out.contains("fn bar()"));
    }

    #[test]
    fn rust_block_comment_stripped() {
        let src = "let x = /* block */ 42;\n";
        let out = compress_source_for_llm(src, Some("rs"));
        assert!(!out.contains("block"));
        assert!(out.contains("let x ="));
        assert!(out.contains("42;"));
    }

    #[test]
    fn rust_multiline_block_comment_stripped() {
        let src = "/*\n * multi\n * line\n */\nfn baz() {}\n";
        let out = compress_source_for_llm(src, Some("rs"));
        assert!(!out.contains("multi"));
        assert!(!out.contains("line"));
        assert!(out.contains("fn baz()"));
    }

    #[test]
    fn rust_doc_comment_stripped() {
        // /// doc comments are still // comments
        let src = "/// This is a doc comment\npub fn documented() {}\n";
        let out = compress_source_for_llm(src, Some("rs"));
        assert!(!out.contains("doc comment"));
        assert!(out.contains("pub fn documented()"));
    }

    // -----------------------------------------------------------------------
    // Rust / C-style: string literals are preserved
    // -----------------------------------------------------------------------

    #[test]
    fn string_literal_with_comment_syntax_preserved() {
        let src = r#"let s = "// not a comment";"#;
        let out = compress_source_for_llm(src, Some("rs"));
        assert!(out.contains("// not a comment"), "got: {out}");
    }

    #[test]
    fn string_literal_with_hash_preserved_in_rust() {
        let src = r##"let s = "# also not a comment";"##;
        let out = compress_source_for_llm(src, Some("rs"));
        assert!(out.contains("# also not a comment"), "got: {out}");
    }

    #[test]
    fn block_comment_inside_string_preserved() {
        let src = r#"let s = "/* stay */";"#;
        let out = compress_source_for_llm(src, Some("rs"));
        assert!(out.contains("/* stay */"), "got: {out}");
    }

    // -----------------------------------------------------------------------
    // TypeScript / other C-style aliases
    // -----------------------------------------------------------------------

    #[test]
    fn typescript_line_comment_stripped() {
        let src = "const x = 1; // ts comment\n";
        let out = compress_source_for_llm(src, Some("ts"));
        assert!(!out.contains("ts comment"));
        assert!(out.contains("const x = 1;"));
    }

    #[test]
    fn csharp_block_comment_stripped() {
        let src = "/* C# block */ int y = 5;\n";
        let out = compress_source_for_llm(src, Some("cs"));
        assert!(!out.contains("C# block"));
        assert!(out.contains("int y = 5;"));
    }

    #[test]
    fn no_language_hint_defaults_to_c_style() {
        let src = "// comment\nlet z = 3;\n";
        let out = compress_source_for_llm(src, None);
        assert!(!out.contains("comment"));
        assert!(out.contains("let z = 3;"));
    }

    // -----------------------------------------------------------------------
    // Python
    // -----------------------------------------------------------------------

    #[test]
    fn python_hash_comment_stripped() {
        let src = "x = 1  # inline comment\n";
        let out = compress_source_for_llm(src, Some("py"));
        assert!(!out.contains("inline comment"), "got: {out}");
        assert!(out.contains("x = 1"), "got: {out}");
    }

    #[test]
    fn python_standalone_hash_comment_stripped() {
    let src = "# full line comment\ny = 2\n";
        let out = compress_source_for_llm(src, Some("py"));
        assert!(!out.contains("full line comment"), "got: {out}");
        assert!(out.contains("y = 2"), "got: {out}");
    }

    #[test]
    fn python_triple_double_quote_docstring_preserved() {
        let src = "def foo():\n    \"\"\"This is a docstring.\"\"\"\n    pass\n";
        let out = compress_source_for_llm(src, Some("py"));
        assert!(out.contains("This is a docstring."), "got: {out}");
        assert!(out.contains("def foo():"), "got: {out}");
    }

    #[test]
    fn python_triple_single_quote_docstring_preserved() {
        let src = "def bar():\n    '''Another docstring.'''\n    return 1\n";
        let out = compress_source_for_llm(src, Some("py"));
        assert!(out.contains("Another docstring."), "got: {out}");
    }

    #[test]
    fn python_hash_inside_string_preserved() {
        let src = "msg = \"hello # world\"\n";
        let out = compress_source_for_llm(src, Some("py"));
        assert!(out.contains("# world"), "got: {out}");
    }

    // -----------------------------------------------------------------------
    // Blank-line collapsing
    // -----------------------------------------------------------------------

    #[test]
    fn consecutive_blank_lines_collapsed_to_one() {
        let src = "fn a() {}\n\n\n\nfn b() {}\n";
        let out = compress_source_for_llm(src, Some("rs"));
        // Must not contain more than one consecutive blank line
        assert!(!out.contains("\n\n\n"), "got: {out}");
        assert!(out.contains("fn a()"));
        assert!(out.contains("fn b()"));
    }

    #[test]
    fn single_blank_line_kept() {
        let src = "fn a() {}\n\nfn b() {}\n";
        let out = compress_source_for_llm(src, Some("rs"));
        assert!(out.contains("\n\n"), "expected one blank line preserved, got: {out}");
    }

    // -----------------------------------------------------------------------
    // Indentation preservation
    // -----------------------------------------------------------------------

    #[test]
    fn leading_indentation_preserved() {
        let src = "fn outer() {\n    let x = 1;\n        let y = 2;\n}\n";
        let out = compress_source_for_llm(src, Some("rs"));
        assert!(out.contains("    let x = 1;"), "got: {out}");
        assert!(out.contains("        let y = 2;"), "got: {out}");
    }

    #[test]
    fn trailing_spaces_removed() {
        let src = "fn foo()   \n{\n}\n";
        let out = compress_source_for_llm(src, Some("rs"));
        // "fn foo()" should have no trailing spaces
        for line in out.lines() {
            assert_eq!(line, line.trim_end(), "trailing space on line: {:?}", line);
        }
    }

    // -----------------------------------------------------------------------
    // Round-trip: output is always valid UTF-8 and non-empty for non-empty input
    // -----------------------------------------------------------------------

    #[test]
    fn output_is_valid_utf8_for_unicode_source() {
        let src = "// こんにちは\nlet x = \"世界\";  // 世界\n";
        let out = compress_source_for_llm(src, Some("rs"));
        // Should not panic and should preserve the string literal content
        assert!(out.contains("世界"));
    }

    #[test]
    fn empty_input_gives_empty_output() {
        let out = compress_source_for_llm("", Some("rs"));
        assert!(out.is_empty() || out.trim().is_empty());
    }
}
