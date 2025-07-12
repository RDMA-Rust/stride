use stride::utils::display::{BandwidthResult, DEFAULT_HEADER_WIDTH};
use stride::utils::table::{TableFormatter, TableRow};

#[test]
fn test_table_width_alignment() {
    // Test that expanded tables match header width exactly
    let bw_result = BandwidthResult {
        size: 2,
        iterations: 1000,
        bandwidth: 0.0259,
        msg_rate: 1.6218,
        time: "0.00".to_string(),
    };

    // Create formatter with header width expansion
    let formatter = TableFormatter::with_total_width::<BandwidthResult>(DEFAULT_HEADER_WIDTH);
    let widths = formatter.get_column_widths();

    // Build actual row format
    let values = bw_result.values();
    let mut actual_row = String::new();
    for (i, (value, &width)) in values.iter().zip(widths.iter()).enumerate() {
        if i == 0 {
            actual_row.push(' '); // Leading space
        } else {
            actual_row.push_str(" | "); // Pipe separator with spaces
        }

        let formatted = format!("{:<width$}", value, width = width);
        actual_row.push_str(&formatted);
    }
    actual_row.push(' '); // Trailing space

    // Verify row matches header width exactly
    assert_eq!(
        actual_row.chars().count(),
        DEFAULT_HEADER_WIDTH,
        "Table row width {} does not match header width {}. Row: '{}'",
        actual_row.chars().count(),
        DEFAULT_HEADER_WIDTH,
        actual_row
    );

    // Build separator by character replacement
    let mut separator = String::new();
    for ch in actual_row.chars() {
        match ch {
            ' ' => separator.push('-'),
            '|' => separator.push('┼'),
            _ => separator.push('-'),
        }
    }

    // Verify separator matches header width in character count
    assert_eq!(
        separator.chars().count(),
        DEFAULT_HEADER_WIDTH,
        "Table separator character count {} does not match header width {}. Separator: '{}'",
        separator.chars().count(),
        DEFAULT_HEADER_WIDTH,
        separator
    );
}

#[test]
fn test_natural_width_table() {
    // Test that natural width tables work correctly
    let formatter = TableFormatter::new::<BandwidthResult>();
    let widths = formatter.get_column_widths();

    // Should match the original column widths
    let expected_widths = BandwidthResult::column_widths();
    assert_eq!(
        widths, expected_widths,
        "Natural width table should preserve original column widths"
    );
}

#[test]
fn test_width_calculation_edge_cases() {
    // Test with minimum viable header width
    let natural_widths = BandwidthResult::column_widths();
    let min_width = natural_widths.iter().sum::<usize>() + 2 + (natural_widths.len() - 1) * 3;

    // This should work without panic
    let formatter = TableFormatter::with_total_width::<BandwidthResult>(min_width);
    let final_widths = formatter.get_column_widths();

    // Last column should not be expanded since we're at minimum width
    assert_eq!(
        final_widths[final_widths.len() - 1],
        natural_widths[natural_widths.len() - 1]
    );
}

#[test]
#[should_panic(expected = "Cannot compress table")]
fn test_width_too_small_panics() {
    // Test that trying to compress below natural width panics
    let natural_widths = BandwidthResult::column_widths();
    let min_width = natural_widths.iter().sum::<usize>() + 2 + (natural_widths.len() - 1) * 3;

    // This should panic
    TableFormatter::with_total_width::<BandwidthResult>(min_width - 1);
}
