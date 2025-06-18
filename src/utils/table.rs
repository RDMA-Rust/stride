use std::io::{self, Write};

pub trait TableRow {
    fn header() -> Vec<&'static str>;
    fn values(&self) -> Vec<String>;
    fn column_widths() -> Vec<usize>;
}

pub struct TableFormatter {
    column_widths: Vec<usize>,
    headers: Vec<String>,
    header_printed: bool,
    target_width: Option<usize>, // Store the target width for footer alignment
}

impl TableFormatter {
    pub fn get_column_widths(&self) -> &[usize] {
        &self.column_widths
    }

    pub fn new<T: TableRow>() -> Self {
        let headers = T::header().iter().map(|s| s.to_string()).collect();
        let column_widths = T::column_widths();

        Self {
            column_widths,
            headers,
            header_printed: false,
            target_width: None,
        }
    }

    pub fn with_total_width<T: TableRow>(total_width: usize) -> Self {
        let headers = T::header().iter().map(|s| s.to_string()).collect();
        let mut column_widths = T::column_widths();

        // Calculate minimum required width for natural table
        let natural_content_width: usize = column_widths.iter().sum();
        let separators_width = 2 + (column_widths.len() - 1) * 3; // leading + trailing + " | " separators
        let natural_total_width = natural_content_width + separators_width;

        // Compile-time style assertion for debugging
        if total_width < natural_total_width {
            panic!("TableFormatter::with_total_width(): target width {} is smaller than natural table width {}. Cannot compress table!",
                   total_width, natural_total_width);
        }

        // Adjust the last column to fill remaining space
        // Row format: " col1 | col2 | col3 | col4 | col5 "
        // = 1 leading + col1 + 3 sep + col2 + 3 sep + col3 + 3 sep + col4 + 3 sep + col5 + 1 trailing
        let used_width: usize = column_widths.iter().take(column_widths.len() - 1).sum();

        if total_width > used_width + separators_width {
            let remaining = total_width - used_width - separators_width;
            if let Some(last_width) = column_widths.last_mut() {
                *last_width = remaining;
            }
        }

        // Verify the final result matches target width exactly
        let final_content_width: usize = column_widths.iter().sum();
        let final_total_width = final_content_width + separators_width;

        debug_assert_eq!(
            final_total_width, total_width,
            "TableFormatter width calculation error! Expected: {}, Actual: {}, Column widths: {:?}",
            total_width, final_total_width, column_widths
        );

        Self {
            column_widths,
            headers,
            header_printed: false,
            target_width: Some(total_width),
        }
    }

    pub fn print_header(&mut self) -> io::Result<()> {
        if self.header_printed {
            return Ok(());
        }

        // Print header row
        self.print_row(&self.headers.clone())?;

        // Print separator line
        self.print_separator()?;

        self.header_printed = true;
        Ok(())
    }

    pub fn print_row_data<T: TableRow>(&mut self, data: &T) -> io::Result<()> {
        if !self.header_printed {
            self.print_header()?;
        }

        let values = data.values();
        self.print_row(&values)
    }

    pub fn print_collection<T: TableRow>(&mut self, data: &[T]) -> io::Result<()> {
        if !self.header_printed {
            self.print_header()?;
        }

        for item in data {
            let values = item.values();
            self.print_row(&values)?;
        }

        // Print closing separator for collection
        self.print_bottom_separator()?;

        Ok(())
    }

    pub fn print_bottom_separator(&self) -> io::Result<()> {
        // Use target width if available (for header alignment), otherwise calculate from table
        let separator_width = if let Some(target) = self.target_width {
            target
        } else {
            // Calculate the actual width of the table based on current column widths
            self.column_widths.iter().sum::<usize>()
                + (self.column_widths.len() * 2) // spaces around each column
                + (self.column_widths.len() - 1) * 3 // " | " separators
                + 2 // leading/trailing spaces
        };

        // Create a plain line of dashes that matches the header width
        let separator = "-".repeat(separator_width);

        println!("{}", separator);
        io::stdout().flush()?;

        Ok(())
    }

    fn print_row(&self, values: &[String]) -> io::Result<()> {
        let mut output = String::new();

        for (i, (value, &width)) in values.iter().zip(self.column_widths.iter()).enumerate() {
            if i == 0 {
                output.push(' '); // Leading space
            } else {
                output.push_str(" | "); // Pipe separator with spaces
            }

            // Truncate or pad to exact width
            let formatted = if value.len() > width {
                value.chars().take(width).collect::<String>()
            } else {
                format!("{:<width$}", value, width = width)
            };

            output.push_str(&formatted);
        }

        output.push(' '); // Trailing space
        println!("{}", output);
        io::stdout().flush()?;

        Ok(())
    }

    fn print_separator(&self) -> io::Result<()> {
        // Build separator using the actual current column widths (which may be expanded)
        let dummy_values: Vec<String> = self.column_widths.iter().map(|&w| "x".repeat(w)).collect();
        let mut sample_row = String::new();
        for (i, (value, &width)) in dummy_values
            .iter()
            .zip(self.column_widths.iter())
            .enumerate()
        {
            if i == 0 {
                sample_row.push(' ');
            } else {
                sample_row.push_str(" | ");
            }
            let formatted = format!("{:<width$}", value, width = width);
            sample_row.push_str(&formatted);
        }
        sample_row.push(' ');

        // Build separator by replacing each character in the sample row
        let mut separator = String::new();
        for ch in sample_row.chars() {
            match ch {
                ' ' => separator.push('-'),
                '|' => separator.push('┼'),
                _ => separator.push('-'),
            }
        }

        println!("{}", separator);
        io::stdout().flush()?;

        Ok(())
    }
}

// Utility function to create a one-off table without formatter state
pub fn print_table<T: TableRow>(data: &[T]) -> io::Result<()> {
    if data.is_empty() {
        return Ok(());
    }

    let mut formatter = TableFormatter::new::<T>();
    formatter.print_collection(data)
}

// Utility function to print a single row table (useful for individual results)
pub fn print_single_row<T: TableRow>(data: &T) -> io::Result<()> {
    let mut formatter = TableFormatter::new::<T>();
    formatter.print_row_data(data)
}

// Utility function with custom total width
pub fn print_table_with_width<T: TableRow>(data: &[T], total_width: usize) -> io::Result<()> {
    if data.is_empty() {
        return Ok(());
    }

    let mut formatter = TableFormatter::with_total_width::<T>(total_width);
    formatter.print_collection(data)
}

pub fn print_single_row_with_width<T: TableRow>(data: &T, total_width: usize) -> io::Result<()> {
    let mut formatter = TableFormatter::with_total_width::<T>(total_width);
    formatter.print_row_data(data)?;
    formatter.print_bottom_separator()
}
