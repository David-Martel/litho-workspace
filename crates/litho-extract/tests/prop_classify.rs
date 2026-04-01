use litho_core::types::FileClassification;
use litho_extract::classify::classify_file;
use proptest::prelude::*;
use std::path::PathBuf;

proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(96))]

    #[test]
    fn tests_directory_is_classified_as_test(stem in "[a-zA-Z0-9_]{1,24}") {
        let path = PathBuf::from(format!("src/tests/{stem}.rs"));
        let class = classify_file(&path, true);
        prop_assert_eq!(class, FileClassification::Test);
    }

    #[test]
    fn rust_test_suffix_is_classified_as_test(stem in "[a-zA-Z0-9_]{1,24}") {
        let path = PathBuf::from(format!("src/{stem}_test.rs"));
        let class = classify_file(&path, false);
        prop_assert_eq!(class, FileClassification::Test);
    }

    #[test]
    fn cargo_toml_is_config(stem in "[a-zA-Z0-9_]{1,16}") {
        let path = PathBuf::from(format!("{stem}/Cargo.toml"));
        let class = classify_file(&path, false);
        prop_assert_eq!(class, FileClassification::Config);
    }
}
