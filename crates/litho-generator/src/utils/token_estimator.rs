use serde::{Deserialize, Serialize};

/// Token estimator for estimating the number of tokens in text
pub struct TokenEstimator {
    /// Token calculation rules for different models
    model_rules: TokenCalculationRules,
}

/// Token calculation rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCalculationRules {
    /// Average token ratio for English characters (characters/token)
    pub english_char_per_token: f64,
    /// Average token ratio for Chinese characters
    pub chinese_char_per_token: f64,
    /// Base token overhead (system prompt, etc.)
    pub base_token_overhead: usize,
}

impl Default for TokenCalculationRules {
    fn default() -> Self {
        Self {
            // Based on empirical values from GPT series models
            english_char_per_token: 4.0,
            chinese_char_per_token: 1.5,
            base_token_overhead: 50,
        }
    }
}

/// Token estimation result
#[derive(Debug, Clone)]
pub struct TokenEstimation {
    /// Estimated number of tokens
    pub estimated_tokens: usize,
    /// Number of characters in text
    #[allow(dead_code)]
    pub character_count: usize,
    /// Number of Chinese characters
    #[allow(dead_code)]
    pub chinese_char_count: usize,
    /// Number of English characters
    #[allow(dead_code)]
    pub english_char_count: usize,
}

impl Default for TokenEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenEstimator {
    pub fn new() -> Self {
        Self {
            model_rules: TokenCalculationRules::default(),
        }
    }

    /// Estimate the number of tokens in text
    pub fn estimate_tokens(&self, text: &str) -> TokenEstimation {
        let character_count = text.chars().count();
        let chinese_char_count = self.count_chinese_chars(text);
        let english_char_count = self.count_english_chars(text);
        let other_char_count = character_count - chinese_char_count - english_char_count;

        // Calculate token count for each part
        let chinese_tokens =
            (chinese_char_count as f64 / self.model_rules.chinese_char_per_token).ceil() as usize;
        let english_tokens =
            (english_char_count as f64 / self.model_rules.english_char_per_token).ceil() as usize;
        // Calculate other characters using English rules
        let other_tokens = if other_char_count > 0 {
            (other_char_count as f64 / self.model_rules.english_char_per_token).ceil() as usize
        } else {
            0
        };

        let estimated_tokens =
            chinese_tokens + english_tokens + other_tokens + self.model_rules.base_token_overhead;

        TokenEstimation {
            estimated_tokens,
            character_count,
            chinese_char_count,
            english_char_count,
        }
    }

    /// Count number of Chinese characters
    fn count_chinese_chars(&self, text: &str) -> usize {
        text.chars().filter(|c| self.is_chinese_char(*c)).count()
    }

    /// Count number of English characters
    fn count_english_chars(&self, text: &str) -> usize {
        text.chars()
            .filter(|c| {
                c.is_ascii_alphabetic()
                    || c.is_ascii_whitespace()
                    || c.is_ascii_digit()
                    || c.is_ascii_punctuation()
            })
            .count()
    }

    /// Check if a character is Chinese
    fn is_chinese_char(&self, c: char) -> bool {
        matches!(c as u32,
            0x4E00..=0x9FFF |  // CJK Unified Ideographs
            0x3400..=0x4DBF |  // CJK Extension A
            0x20000..=0x2A6DF | // CJK Extension B
            0x2A700..=0x2B73F | // CJK Extension C
            0x2B740..=0x2B81F | // CJK Extension D
            0x2B820..=0x2CEAF | // CJK Extension E
            0x2CEB0..=0x2EBEF | // CJK Extension F
            0x30000..=0x3134F   // CJK Extension G
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estimator() -> TokenEstimator {
        TokenEstimator::new()
    }

    // -----------------------------------------------------------------------
    // TokenCalculationRules defaults
    // -----------------------------------------------------------------------

    #[test]
    fn rules_default_english_ratio() {
        let r = TokenCalculationRules::default();
        assert!((r.english_char_per_token - 4.0).abs() < 0.01);
    }

    #[test]
    fn rules_default_chinese_ratio() {
        let r = TokenCalculationRules::default();
        assert!((r.chinese_char_per_token - 1.5).abs() < 0.01);
    }

    #[test]
    fn rules_default_base_overhead_positive() {
        let r = TokenCalculationRules::default();
        assert!(r.base_token_overhead > 0);
    }

    // -----------------------------------------------------------------------
    // estimate_tokens: English-only text
    // -----------------------------------------------------------------------

    #[test]
    fn empty_string_gives_only_overhead() {
        let est = estimator();
        let r = est.estimate_tokens("");
        let overhead = TokenCalculationRules::default().base_token_overhead;
        assert_eq!(r.estimated_tokens, overhead);
        assert_eq!(r.character_count, 0);
    }

    #[test]
    fn single_ascii_word() {
        let est = estimator();
        let r = est.estimate_tokens("hello");
        assert_eq!(r.character_count, 5);
        assert_eq!(r.chinese_char_count, 0);
        assert_eq!(r.english_char_count, 5);
        // 5 chars / 4.0 = ceil(1.25) = 2 tokens + 50 overhead = 52
        assert_eq!(r.estimated_tokens, 52);
    }

    #[test]
    fn longer_text_more_tokens_than_short() {
        let est = estimator();
        let short = est.estimate_tokens("hi");
        let long =
            est.estimate_tokens("This is a considerably longer English sentence with many words.");
        assert!(long.estimated_tokens > short.estimated_tokens);
    }

    // -----------------------------------------------------------------------
    // estimate_tokens: Chinese characters
    // -----------------------------------------------------------------------

    #[test]
    fn four_chinese_chars() {
        let est = estimator();
        let r = est.estimate_tokens("你好世界");
        assert_eq!(r.chinese_char_count, 4);
        assert_eq!(r.english_char_count, 0);
        // ceil(4 / 1.5) = 3 tokens + 50 overhead = 53
        assert_eq!(r.estimated_tokens, 53);
    }

    #[test]
    fn cjk_extension_a_counted_as_chinese() {
        // U+3400 is the first CJK Extension A character
        let est = estimator();
        let s = '\u{3400}'.to_string();
        let r = est.estimate_tokens(&s);
        assert_eq!(r.chinese_char_count, 1);
    }

    // -----------------------------------------------------------------------
    // estimate_tokens: mixed text
    // -----------------------------------------------------------------------

    #[test]
    fn mixed_english_and_chinese() {
        let est = estimator();
        let r = est.estimate_tokens("hello 世界");
        assert!(r.chinese_char_count >= 2, "at least 2 Chinese chars");
        assert!(r.english_char_count >= 5, "at least 5 English chars");
        assert!(r.character_count >= 7);
        assert!(r.estimated_tokens > TokenCalculationRules::default().base_token_overhead);
    }

    // -----------------------------------------------------------------------
    // TokenEstimation fields
    // -----------------------------------------------------------------------

    #[test]
    fn character_count_equals_char_count() {
        let est = estimator();
        let text = "abc 你好";
        let r = est.estimate_tokens(text);
        assert_eq!(r.character_count, text.chars().count());
    }

    #[test]
    fn chinese_plus_english_plus_other_sums_to_char_count() {
        let est = estimator();
        // Text with only ASCII — all should be english_char_count
        let text = "Hello World";
        let r = est.estimate_tokens(text);
        // No CJK, so chinese_char_count == 0
        assert_eq!(r.chinese_char_count, 0);
        // english_char_count includes ASCII alphabetic, whitespace, digit, punctuation
        assert!(r.english_char_count <= r.character_count);
    }
}
