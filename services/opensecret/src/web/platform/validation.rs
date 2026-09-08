use lazy_static::lazy_static;
use regex::Regex;
use std::borrow::Cow;
use validator::ValidationError;

lazy_static! {
    // Allows:
    // - ASCII Alphanumeric characters (a-z, A-Z, 0-9)
    // - Common symbols used in organization names (&@!-+)
    // - Parentheses ()
    // - Underscore and dot for technical names (_.)
    // - Spaces (will be trimmed)
    static ref ALPHANUMERIC_WITH_SYMBOLS: Regex = Regex::new(r"^[a-zA-Z0-9\s&@!()\-+._]+$").unwrap();
    static ref ALPHANUMERIC_ONLY: Regex = Regex::new(r"^[a-zA-Z0-9_]+$").unwrap();
}

pub fn validate_alphanumeric_with_symbols(value: &str) -> Result<(), ValidationError> {
    let trimmed = value.trim();
    if !ALPHANUMERIC_WITH_SYMBOLS.is_match(trimmed) {
        return Err(
            ValidationError::new("invalid_characters").with_message(Cow::Borrowed(
                "Only ASCII alphanumeric characters and &@!()-_+. are allowed",
            )),
        );
    }
    Ok(())
}

pub fn validate_alphanumeric_only(value: &str) -> Result<(), ValidationError> {
    if !ALPHANUMERIC_ONLY.is_match(value) {
        return Err(ValidationError::new("alphanumeric_only"));
    }
    Ok(())
}

pub fn validate_secret_size(value: &str) -> Result<(), ValidationError> {
    // Check if base64 decoded size would be more than 1MB
    let decoded_size = (value.len() * 3) / 4; // Approximate base64 decoded size
    if decoded_size > 1_048_576 {
        // 1MB = 1,048,576 bytes
        return Err(ValidationError::new("secret_size"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_names_with_symbols() {
        let valid_names = vec![
            // Basic alphanumeric
            "MyCompany123",
            "UPPERCASE",
            "lowercase",
            "12345",
            // With spaces (will be trimmed)
            "My Company",
            "  Trimmed Space  ",
            "Multiple   Spaces   Here",
            // With special characters
            "Company & Partners",
            "Tech.io",
            "Project (Beta)",
            "Company-Name",
            "Product+Service",
            "My_Project",
            "Company.com",
            "Project@2024",
            "Product!",
            // Complex combinations
            "My Company & Partners (2024)",
            "Tech.io - Beta Release",
            "Project_Name@Version2.0",
            "Company+Product!",
            "My-Amazing.Project_v1",
            // Multiple special characters
            "Company!!Name",
            "Project--2024",
            "Name...Star",
            "Company___Division",
        ];

        for name in valid_names {
            assert!(
                validate_alphanumeric_with_symbols(name).is_ok(),
                "Should accept valid name: {}",
                name
            );
        }
    }

    #[test]
    fn test_invalid_names_with_symbols() {
        let invalid_names = vec![
            // Invalid special characters
            "Company$Name",
            "Project#Tag",
            "Name*Star",
            "Company\\Division",
            "Project/Name",
            "Company%20",
            "Name^Power",
            "Company=Product",
            "Project{Dev}",
            // Empty or whitespace only
            "",
            "   ",
            "\t",
            "\n",
            // Unicode characters
            "Company™",
            "Café",
            "Company•Name",
            "Project→Future",
        ];

        for name in invalid_names {
            assert!(
                validate_alphanumeric_with_symbols(name).is_err(),
                "Should reject invalid name: {}",
                name
            );
        }
    }

    #[test]
    fn test_alphanumeric_only() {
        // Valid cases
        let valid_strings = vec![
            "abc123",
            "ABC123",
            "123456",
            "abcABC123",
            "abc_123",
            "ABC_DEF",
            "RESEND_API_KEY",
        ];

        for s in valid_strings {
            assert!(
                validate_alphanumeric_only(s).is_ok(),
                "Should accept valid string: {}",
                s
            );
        }

        // Invalid cases
        let invalid_strings = vec![
            "abc 123", "abc-123", "abc.123", "abc@123", "", " ", "abc!123",
        ];

        for s in invalid_strings {
            assert!(
                validate_alphanumeric_only(s).is_err(),
                "Should reject invalid string: {}",
                s
            );
        }
    }

    #[test]
    fn test_secret_size() {
        // Test valid sizes
        assert!(
            validate_secret_size("A".repeat(1_000_000).as_str()).is_ok(),
            "Should accept 1MB secret"
        );
        assert!(
            validate_secret_size("").is_ok(),
            "Should accept empty secret"
        );

        // Test invalid sizes
        assert!(
            validate_secret_size("A".repeat(2_000_000).as_str()).is_err(),
            "Should reject >1MB secret"
        );
    }
}
