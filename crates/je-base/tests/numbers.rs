//! Porte de `src/vs/base/test/common/numbers.test.ts` (suíte `isPointWithinTriangle`), caso a caso e na mesma ordem.
//!
//! O arquivo original tem ainda a suíte `formatTokenCount`, portada abaixo na mesma ordem.

use je_base::numbers::{format_token_count, is_point_within_triangle};

// suite('isPointWithinTriangle')

#[test]
fn should_return_true_if_the_point_is_within_the_triangle() {
    let result = is_point_within_triangle(0.25, 0.25, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0);
    assert!(result);
}

#[test]
fn should_return_false_if_the_point_is_outside_the_triangle() {
    let result = is_point_within_triangle(2.0, 2.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0);
    assert!(!result);
}

#[test]
fn should_return_true_if_the_point_is_on_the_edge_of_the_triangle() {
    let result = is_point_within_triangle(0.5, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0);
    assert!(result);
}

// suite('formatTokenCount')

#[test]
fn returns_m_for_counts_above_900k() {
    assert_eq!(format_token_count(1_000_000.0), "1M");
    assert_eq!(format_token_count(935_997.0), "1M");
    assert_eq!(format_token_count(1_050_000.0), "1M");
    assert_eq!(format_token_count(1_100_000.0), "1.1M");
    assert_eq!(format_token_count(1_500_000.0), "1.5M");
    assert_eq!(format_token_count(1_990_000.0), "1.9M");
    assert_eq!(format_token_count(2_000_000.0), "2M");
    assert_eq!(format_token_count(2_500_000.0), "2.5M");
}

#[test]
fn returns_k_for_counts_between_1000_and_900k() {
    assert_eq!(format_token_count(200_000.0), "200K");
    assert_eq!(format_token_count(128_000.0), "128K");
    assert_eq!(format_token_count(1_000.0), "1K");
    assert_eq!(format_token_count(900_000.0), "900K");
}

#[test]
fn returns_raw_number_for_counts_below_1000() {
    assert_eq!(format_token_count(500.0), "500");
    assert_eq!(format_token_count(0.0), "0");
}
