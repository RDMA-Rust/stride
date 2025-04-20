use serde::{Deserialize, Serialize};
use sideway::ibverbs::address::Gid;
use std::fmt::Display;
use tabled::{
    settings::{object::Columns, style::HorizontalLine, Style, Width},
    Table, Tabled,
};

// Display configuration constants
pub const DEFAULT_HEADER_WIDTH: usize = 90;
pub const DEFAULT_LAT_HEADER_WIDTH: usize = 140;

pub const DEFAULT_ROWS_PER_COLUMN: usize = 10;
pub const DEFAULT_COLUMN_SPACING: usize = 10;
pub const DEFAULT_KEY_VALUE_SPACING: usize = 1; // Includes ":    " spacing
pub const DEFAULT_HEADER_MARGIN_LEN: usize = 2; // Space length around header text

// Bandwidth and message rate column widths
pub const SIZE_COLUMN_WIDTH: usize = 12;
pub const ITERATIONS_COLUMN_WIDTH: usize = 12;
pub const BANDWIDTH_COLUMN_WIDTH: usize = 18;
pub const MSG_RATE_COLUMN_WIDTH: usize = 18;
pub const SEPARATOR_WIDTH: usize = 3; // Width of a single separator character
pub const COLUMN_COUNT: usize = 5; // Total number of columns

// Latency column widths
pub const LAT_SIZE_COLUMN_WIDTH: usize = 12;
pub const LAT_ITER_COLUMN_WIDTH: usize = 12;
pub const LAT_MIN_COLUMN_WIDTH: usize = 12;
pub const LAT_MAX_COLUMN_WIDTH: usize = 14;
pub const LAT_TYP_COLUMN_WIDTH: usize = 12;
pub const LAT_AVG_COLUMN_WIDTH: usize = 12;
pub const LAT_STDEV_COLUMN_WIDTH: usize = 12;
pub const LAT_P99_COLUMN_WIDTH: usize = 12;
pub const LAT_COLUMN_COUNT: usize = 9; // Total number of columns

pub const START_AND_END_SPACES_WIDTH: usize = 2; // Width of start and end spaces

macro_rules! header_width {
    () => {
        DEFAULT_HEADER_WIDTH
    };
    ($w:expr) => {
        $w
    };
    (for_test $test_type:expr) => {
        match $test_type {
            TestType::SendLatency | TestType::WriteLatency | TestType::ReadLatency => {
                DEFAULT_LAT_HEADER_WIDTH
            }
            _ => DEFAULT_HEADER_WIDTH,
        }
    };
}

#[derive(Tabled)]
pub struct TestConfiguration {
    pub device: String,
    pub transport: String,
    pub qp_count: u32,
    pub connection_type: String,
    pub mtu: u32,
    pub gid_type: String,
    pub rx_depth: u32,
    pub tx_depth: u32,
    pub post_list: u32,
    pub test_type: TestType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestType {
    SendBandwidth,
    SendLatency,
    WriteBandwidth,
    WriteLatency,
    ReadBandwidth,
    ReadLatency,
}

impl TestType {
    pub fn is_latency(&self) -> bool {
        matches!(
            self,
            TestType::SendLatency | TestType::WriteLatency | TestType::ReadLatency
        )
    }
}

impl Display for TestType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestType::SendBandwidth => write!(f, "RDMA Send Bandwidth Test"),
            TestType::SendLatency => write!(f, "RDMA Send Latency Test"),
            TestType::WriteBandwidth => write!(f, "RDMA Write Bandwidth Test"),
            TestType::WriteLatency => write!(f, "RDMA Write Latency Test"),
            TestType::ReadBandwidth => write!(f, "RDMA Read Bandwidth Test"),
            TestType::ReadLatency => write!(f, "RDMA Read Latency Test"),
        }
    }
}

#[derive(Tabled, Clone)]
pub struct QueuePairDetail {
    pub qp_index: u32,
    pub local_qpn: u32,
    pub local_psn: u32,
    pub remote_qpn: u32,
    pub remote_psn: u32,
}

impl TestConfiguration {
    fn to_config_fields(&self) -> Vec<ConfigField> {
        vec![
            ConfigField::new("Device", &self.device),
            ConfigField::new("Transport", &self.transport),
            ConfigField::new("QP Count", self.qp_count),
            ConfigField::new("Connection Type", &self.connection_type),
            ConfigField::new("MTU", self.mtu),
            ConfigField::new("GID Type", &self.gid_type),
            ConfigField::new("Rx Depth", self.rx_depth),
            ConfigField::new("Tx Depth", self.tx_depth),
            ConfigField::new("Post List", self.post_list),
        ]
    }
}

impl Display for TestConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fields = self.to_config_fields();
        let formatter = ColumnFormatter::new(fields, DEFAULT_ROWS_PER_COLUMN);

        debug_assert!(
            formatter.validates_width(DEFAULT_HEADER_WIDTH),
            "Formatted output width {} exceeds header width {}",
            formatter.get_total_width(),
            DEFAULT_HEADER_WIDTH
        );

        write!(f, "{}", formatter.format())
    }
}

#[derive(Serialize, Deserialize, Tabled, Clone, Debug)]
pub struct BandwidthResult {
    #[tabled(rename = "Size (B)")]
    pub size: u32,
    #[tabled(rename = "Iterations")]
    pub iterations: u32,
    #[tabled(rename = "Avg BW (Gb/s)", display = "format_bandwidth")]
    pub bandwidth: f64,
    #[tabled(rename = "MsgRate (Mpps)", display = "format_msg_rate")]
    pub msg_rate: f64,
    #[tabled(rename = "Time")]
    pub time: String,
}

#[derive(Serialize, Deserialize, Tabled, Clone, Debug)]
pub struct LatencyResult {
    #[tabled(rename = "Size (B)")]
    pub size: u32,
    #[tabled(rename = "Iterations")]
    pub iterations: u32,
    #[tabled(rename = "Min (us)", display = "format_common_float")]
    pub min_latency: f64,
    #[tabled(rename = "Max (us)", display = "format_common_float")]
    pub max_latency: f64,
    #[tabled(rename = "Avg (us)", display = "format_common_float")]
    pub avg_latency: f64,
    #[tabled(rename = "Stdev (us)", display = "format_common_float")]
    pub stdev_latency: f64,
    #[tabled(rename = "P50 (us)", display = "format_common_float")]
    pub typical_latency: f64,
    #[tabled(rename = "P99 (us)", display = "format_common_float")]
    pub p99_latency: f64,
    #[tabled(rename = "P999 (us)", display = "format_common_float")]
    pub p999_latency: f64,
}

pub struct DisplayOutput {
    config: TestConfiguration,
    qp_details: Vec<QueuePairDetail>,
    gid_info: Vec<Gid>,
    bw_results: Option<BandwidthResult>,
    lat_results: Option<LatencyResult>,
    // Collections for multiple results when using --all-sizes
    bw_results_collection: Vec<BandwidthResult>,
    lat_results_collection: Vec<LatencyResult>,
}

fn format_bandwidth(f: &f64) -> String {
    format!("{:.4}", f)
}

fn format_msg_rate(f: &f64) -> String {
    format!("{:.4}", f)
}

fn format_common_float(f: &f64) -> String {
    format!("{:.3}", f)
}

pub fn min_required_table_width() -> usize {
    SIZE_COLUMN_WIDTH + ITERATIONS_COLUMN_WIDTH +
    BANDWIDTH_COLUMN_WIDTH + MSG_RATE_COLUMN_WIDTH +
    // Account for minimum width needed for time column and separators
    10 + (SEPARATOR_WIDTH * (COLUMN_COUNT - 1)) + START_AND_END_SPACES_WIDTH
}

// Function to get the minimum width required for latency table
pub fn min_required_latency_table_width() -> usize {
    LAT_SIZE_COLUMN_WIDTH
        + LAT_ITER_COLUMN_WIDTH
        + LAT_MIN_COLUMN_WIDTH
        + LAT_MAX_COLUMN_WIDTH
        + LAT_TYP_COLUMN_WIDTH
        + LAT_AVG_COLUMN_WIDTH
        + LAT_STDEV_COLUMN_WIDTH
        + LAT_P99_COLUMN_WIDTH
        + 10
        + (SEPARATOR_WIDTH * (LAT_COLUMN_COUNT - 1))
        + START_AND_END_SPACES_WIDTH
}

fn create_header(text: &str, width: usize, margin_len: usize) -> String {
    let text_len = text.len();
    let total_padding = width - text_len - margin_len * 2;
    let left_padding = total_padding / 2;
    let right_padding = total_padding - left_padding;

    format!(
        "{}{}{}{}{}",
        "-".repeat(left_padding),
        " ".repeat(margin_len),
        text,
        " ".repeat(margin_len),
        "-".repeat(right_padding)
    )
}

impl DisplayOutput {
    pub fn new(
        config: TestConfiguration,
        qp_details: Vec<QueuePairDetail>,
        gid_info: Vec<Gid>,
    ) -> Self {
        Self {
            config,
            qp_details,
            gid_info,
            bw_results: None,
            lat_results: None,
            bw_results_collection: Vec::new(),
            lat_results_collection: Vec::new(),
        }
    }

    pub fn set_bandwidth_results(&mut self, results: BandwidthResult) {
        self.bw_results = Some(results);
    }

    pub fn set_latency_results(&mut self, results: LatencyResult) {
        self.lat_results = Some(results);
    }

    // Add a bandwidth result to the collection for all-sizes mode
    pub fn add_bandwidth_result(&mut self, results: BandwidthResult) {
        self.bw_results_collection.push(results);
    }

    // Add a latency result to the collection for all-sizes mode
    pub fn add_latency_result(&mut self, results: LatencyResult) {
        self.lat_results_collection.push(results);
    }

    // Display a consolidated table of bandwidth results
    pub fn display_bandwidth_collection(&self, header_width: usize) {
        if self.bw_results_collection.is_empty() {
            return;
        }

        println!(
            "{}",
            create_header("Bandwidth Results", header_width, DEFAULT_HEADER_MARGIN_LEN)
        );

        // Use header width for the table width
        let table_width = header_width;

        // Calculate remaining width for the last column
        let defined_width = SIZE_COLUMN_WIDTH
            + ITERATIONS_COLUMN_WIDTH
            + BANDWIDTH_COLUMN_WIDTH
            + MSG_RATE_COLUMN_WIDTH
            + (SEPARATOR_WIDTH * (COLUMN_COUNT - 1))
            + START_AND_END_SPACES_WIDTH;

        let remaining_width = table_width.saturating_sub(defined_width);
        let time_column_width = std::cmp::max(10, remaining_width);

        // Style for the table
        let style = Style::ascii()
            .horizontals([(1, HorizontalLine::inherit(Style::modern()).horizontal('-'))])
            .remove_top()
            .remove_left()
            .remove_right()
            .remove_horizontal()
            .intersection_bottom('-');

        // Create table with multiple rows
        let table = Table::new(self.bw_results_collection.clone())
            .with(style)
            .with(Width::increase(table_width))
            .modify(Columns::single(0), Width::truncate(SIZE_COLUMN_WIDTH))
            .modify(Columns::single(0), Width::increase(SIZE_COLUMN_WIDTH))
            .modify(Columns::single(1), Width::truncate(ITERATIONS_COLUMN_WIDTH))
            .modify(Columns::single(1), Width::increase(ITERATIONS_COLUMN_WIDTH))
            .modify(Columns::single(2), Width::truncate(BANDWIDTH_COLUMN_WIDTH))
            .modify(Columns::single(2), Width::increase(BANDWIDTH_COLUMN_WIDTH))
            .modify(Columns::single(3), Width::truncate(MSG_RATE_COLUMN_WIDTH))
            .modify(Columns::single(3), Width::increase(MSG_RATE_COLUMN_WIDTH))
            .modify(Columns::single(4), Width::truncate(time_column_width))
            .modify(Columns::single(4), Width::increase(time_column_width))
            .to_string();

        println!("{}", table);
    }

    // Display a consolidated table of latency results
    pub fn display_latency_collection(&self, header_width: usize) {
        if self.lat_results_collection.is_empty() {
            return;
        }

        println!(
            "{}",
            create_header("Latency Results", header_width, DEFAULT_HEADER_MARGIN_LEN)
        );

        // Use header width for the table width
        let table_width = header_width;

        // Calculate remaining width for the last column
        let defined_width = LAT_SIZE_COLUMN_WIDTH
            + LAT_ITER_COLUMN_WIDTH
            + LAT_MIN_COLUMN_WIDTH
            + LAT_MAX_COLUMN_WIDTH
            + LAT_TYP_COLUMN_WIDTH
            + LAT_AVG_COLUMN_WIDTH
            + LAT_STDEV_COLUMN_WIDTH
            + LAT_P99_COLUMN_WIDTH
            + (SEPARATOR_WIDTH * (LAT_COLUMN_COUNT - 1))
            + START_AND_END_SPACES_WIDTH;

        let remaining_width = table_width.saturating_sub(defined_width);
        let p999_column_width = std::cmp::max(10, remaining_width);

        // Style for the table
        let style = Style::ascii()
            .horizontals([(1, HorizontalLine::inherit(Style::modern()).horizontal('-'))])
            .remove_top()
            .remove_left()
            .remove_right()
            .remove_horizontal()
            .intersection_bottom('-');

        // Create table with multiple rows
        let table = Table::new(self.lat_results_collection.clone())
            .with(style)
            .with(Width::increase(header_width))
            .modify(Columns::single(0), Width::truncate(LAT_SIZE_COLUMN_WIDTH))
            .modify(Columns::single(0), Width::increase(LAT_SIZE_COLUMN_WIDTH))
            .modify(Columns::single(1), Width::truncate(LAT_ITER_COLUMN_WIDTH))
            .modify(Columns::single(1), Width::increase(LAT_ITER_COLUMN_WIDTH))
            .modify(Columns::single(2), Width::truncate(LAT_MIN_COLUMN_WIDTH))
            .modify(Columns::single(2), Width::increase(LAT_MIN_COLUMN_WIDTH))
            .modify(Columns::single(3), Width::truncate(LAT_MAX_COLUMN_WIDTH))
            .modify(Columns::single(3), Width::increase(LAT_MAX_COLUMN_WIDTH))
            .modify(Columns::single(4), Width::truncate(LAT_TYP_COLUMN_WIDTH))
            .modify(Columns::single(4), Width::increase(LAT_TYP_COLUMN_WIDTH))
            .modify(Columns::single(5), Width::truncate(LAT_AVG_COLUMN_WIDTH))
            .modify(Columns::single(5), Width::increase(LAT_AVG_COLUMN_WIDTH))
            .modify(Columns::single(6), Width::truncate(LAT_STDEV_COLUMN_WIDTH))
            .modify(Columns::single(6), Width::increase(LAT_STDEV_COLUMN_WIDTH))
            .modify(Columns::single(7), Width::truncate(LAT_P99_COLUMN_WIDTH))
            .modify(Columns::single(7), Width::increase(LAT_P99_COLUMN_WIDTH))
            .modify(Columns::single(8), Width::truncate(p999_column_width))
            .modify(Columns::single(8), Width::increase(p999_column_width))
            .to_string();

        println!("{}", table);
    }

    fn format_qp_details(&self) -> String {
        let mut output = String::new();

        for detail in &self.qp_details {
            output.push_str(&format!(
                "QP #{:08}: (Local QPN: 0x{:04x} PSN: 0x{:06x}) -> (Remote QPN: 0x{:04x} PSN: 0x{:06x})\n",
                detail.qp_index, detail.local_qpn, detail.local_psn, detail.remote_qpn, detail.remote_psn
            ));
        }

        output.push_str(&format!("Local  GID: GID: {}\n", self.gid_info[0],));
        output.push_str(&format!("Remote GID: GID: {}", self.gid_info[1],));

        output
    }

    pub fn display(&self) {
        let header_width = header_width!(for_test self.config.test_type);

        println!(
            "{}",
            create_header(
                &format!("{}", self.config.test_type),
                header_width,
                DEFAULT_HEADER_MARGIN_LEN
            )
        );
        println!("{}", self.config);

        println!(
            "{}",
            create_header(
                "Connection Details",
                header_width,
                DEFAULT_HEADER_MARGIN_LEN
            )
        );
        println!("{}\n", self.format_qp_details());

        let style = Style::ascii()
            .horizontals([(1, HorizontalLine::inherit(Style::modern()).horizontal('-'))])
            .remove_top()
            .remove_left()
            .remove_right()
            .remove_horizontal()
            .intersection_bottom('-');

        if let Some(results) = &self.bw_results {
            println!(
                "{}",
                create_header("Bandwidth Results", header_width, DEFAULT_HEADER_MARGIN_LEN)
            );

            // Ensure our header width can accommodate the table
            let table_width = header_width;
            let required_width = min_required_table_width();

            // Assert to catch potential layout issues during development
            debug_assert!(
                table_width >= required_width,
                "Header width {} is insufficient for table minimum width {}",
                table_width,
                required_width
            );

            // Calculate remaining width for the last column
            let defined_width = SIZE_COLUMN_WIDTH
                + ITERATIONS_COLUMN_WIDTH
                + BANDWIDTH_COLUMN_WIDTH
                + MSG_RATE_COLUMN_WIDTH
                + (SEPARATOR_WIDTH * (COLUMN_COUNT - 1))
                + START_AND_END_SPACES_WIDTH;

            let remaining_width = table_width.saturating_sub(defined_width);

            // Set time column to fill remaining space (with a reasonable minimum)
            let time_column_width = std::cmp::max(10, remaining_width);

            // Improved table formatting with constants
            let table = Table::new([results])
                .with(style.clone())
                .with(Width::increase(table_width))
                .modify(Columns::single(0), Width::truncate(SIZE_COLUMN_WIDTH))
                .modify(Columns::single(0), Width::increase(SIZE_COLUMN_WIDTH))
                .modify(Columns::single(1), Width::truncate(ITERATIONS_COLUMN_WIDTH))
                .modify(Columns::single(1), Width::increase(ITERATIONS_COLUMN_WIDTH))
                .modify(Columns::single(2), Width::truncate(BANDWIDTH_COLUMN_WIDTH))
                .modify(Columns::single(2), Width::increase(BANDWIDTH_COLUMN_WIDTH))
                .modify(Columns::single(3), Width::truncate(MSG_RATE_COLUMN_WIDTH))
                .modify(Columns::single(3), Width::increase(MSG_RATE_COLUMN_WIDTH))
                .modify(Columns::single(4), Width::truncate(time_column_width))
                .modify(Columns::single(4), Width::increase(time_column_width))
                .to_string();

            println!("{}", table);
        }

        if let Some(results) = &self.lat_results {
            println!(
                "{}",
                create_header("Latency Results", header_width, DEFAULT_HEADER_MARGIN_LEN)
            );

            // Ensure our header width can accommodate the table
            let table_width = header_width;
            let required_width = min_required_latency_table_width();

            // Assert to catch potential layout issues during development
            debug_assert!(
                table_width >= required_width,
                "Header width {} is insufficient for table minimum width {}",
                table_width,
                required_width
            );

            // Calculate remaining width for the last column
            let defined_width = LAT_SIZE_COLUMN_WIDTH
                + LAT_ITER_COLUMN_WIDTH
                + LAT_MIN_COLUMN_WIDTH
                + LAT_MAX_COLUMN_WIDTH
                + LAT_TYP_COLUMN_WIDTH
                + LAT_AVG_COLUMN_WIDTH
                + LAT_STDEV_COLUMN_WIDTH
                + LAT_P99_COLUMN_WIDTH
                + (SEPARATOR_WIDTH * (LAT_COLUMN_COUNT - 1))
                + START_AND_END_SPACES_WIDTH;

            let remaining_width = table_width.saturating_sub(defined_width);

            // Set time column to fill remaining space (with a reasonable minimum)
            let p999_column_width = std::cmp::max(10, remaining_width);

            let table = Table::new([results])
                .with(style.clone())
                .with(Width::increase(header_width))
                // Column widths based on constants
                .modify(Columns::single(0), Width::truncate(LAT_SIZE_COLUMN_WIDTH))
                .modify(Columns::single(0), Width::increase(LAT_SIZE_COLUMN_WIDTH))
                .modify(Columns::single(1), Width::truncate(LAT_ITER_COLUMN_WIDTH))
                .modify(Columns::single(1), Width::increase(LAT_ITER_COLUMN_WIDTH))
                .modify(Columns::single(2), Width::truncate(LAT_MIN_COLUMN_WIDTH))
                .modify(Columns::single(2), Width::increase(LAT_MIN_COLUMN_WIDTH))
                .modify(Columns::single(3), Width::truncate(LAT_MAX_COLUMN_WIDTH))
                .modify(Columns::single(3), Width::increase(LAT_MAX_COLUMN_WIDTH))
                .modify(Columns::single(4), Width::truncate(LAT_TYP_COLUMN_WIDTH))
                .modify(Columns::single(4), Width::increase(LAT_TYP_COLUMN_WIDTH))
                .modify(Columns::single(5), Width::truncate(LAT_AVG_COLUMN_WIDTH))
                .modify(Columns::single(5), Width::increase(LAT_AVG_COLUMN_WIDTH))
                .modify(Columns::single(6), Width::truncate(LAT_STDEV_COLUMN_WIDTH))
                .modify(Columns::single(6), Width::increase(LAT_STDEV_COLUMN_WIDTH))
                .modify(Columns::single(7), Width::truncate(LAT_P99_COLUMN_WIDTH))
                .modify(Columns::single(7), Width::increase(LAT_P99_COLUMN_WIDTH))
                .modify(Columns::single(8), Width::truncate(p999_column_width))
                .modify(Columns::single(8), Width::increase(p999_column_width))
                .to_string();

            println!("{}", table);
        }

        if self.bw_results_collection.len() > 0 {
            self.display_bandwidth_collection(header_width);
        }

        if self.lat_results_collection.len() > 0 {
            self.display_latency_collection(header_width);
        }
    }
}

#[derive(Debug)]
struct ConfigField {
    key: String,
    value: String,
}

impl ConfigField {
    fn new<K, V>(key: K, value: V) -> Self
    where
        K: Into<String>,
        V: ToString,
    {
        Self {
            key: key.into(),
            value: value.to_string(),
        }
    }
}

struct ColumnFormatter {
    fields: Vec<ConfigField>,
    max_key_width: usize,
    max_value_width: usize,
    rows_per_column: usize,
    total_columns: usize,
    column_spacing: usize,
}

impl ColumnFormatter {
    fn new(fields: Vec<ConfigField>, rows_per_column: usize) -> Self {
        let max_key_width = fields.iter().map(|f| f.key.len()).max().unwrap_or(0);

        let max_value_width = fields.iter().map(|f| f.value.len()).max().unwrap_or(0);

        let total_fields = fields.len();
        let total_columns = total_fields.div_ceil(rows_per_column);

        Self {
            fields,
            max_key_width,
            max_value_width,
            rows_per_column,
            total_columns,
            column_spacing: DEFAULT_COLUMN_SPACING,
        }
    }

    fn get_single_column_width(&self) -> usize {
        self.max_key_width + self.max_value_width + DEFAULT_KEY_VALUE_SPACING
    }

    fn get_total_width(&self) -> usize {
        if self.total_columns <= 1 {
            self.get_single_column_width()
        } else {
            self.get_single_column_width() * self.total_columns
                + self.column_spacing * (self.total_columns - 1)
        }
    }

    fn validates_width(&self, max_width: usize) -> bool {
        self.get_total_width() <= max_width
    }

    fn format(&self) -> String {
        let mut result = String::new();

        for row in 0..self.rows_per_column {
            let mut row_str = String::new();

            for col in 0..self.total_columns {
                let idx = col * self.rows_per_column + row;
                if idx < self.fields.len() {
                    let field = &self.fields[idx];
                    let formatted_field = format!(
                        "{:<key_width$}:{:spacing$}{:<value_width$}",
                        field.key,
                        "",
                        field.value,
                        key_width = self.max_key_width,
                        spacing = DEFAULT_KEY_VALUE_SPACING,
                        value_width = self.max_value_width
                    );
                    row_str.push_str(&formatted_field);

                    if col < self.total_columns - 1 {
                        row_str.push_str(&" ".repeat(self.column_spacing));
                    }
                }
            }

            if !row_str.trim().is_empty() {
                result.push_str(&row_str);
                result.push('\n');
            }
        }

        result
    }
}
