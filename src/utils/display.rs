use serde::{Deserialize, Serialize};
use sideway::ibverbs::address::Gid;
use std::fmt::Display;
use std::io;

use crate::cli::plan::OutputConfig;
use crate::utils::table::{
    print_single_row_with_width, print_table_with_width, TableFormatter, TableRow,
};

// Macro for conditional TUI output
macro_rules! tui_println {
    ($self:expr, $($arg:tt)*) => {
        if $self.output_config.tui_enabled {
            println!($($arg)*);
        }
    };
}

// Display configuration constants
pub const DEFAULT_HEADER_WIDTH: usize = 90;
pub const DEFAULT_LAT_HEADER_WIDTH: usize = 140;

pub const DEFAULT_ROWS_PER_COLUMN: usize = 10;
pub const DEFAULT_COLUMN_SPACING: usize = 10;
pub const DEFAULT_KEY_VALUE_SPACING: usize = 1; // Includes ":    " spacing
pub const DEFAULT_HEADER_MARGIN_LEN: usize = 2; // Space length around header text
pub const KEY_VALUE_PREFIX_PADDING: usize = 1; // Space padding before key-value pairs

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
    pub uses_immediate_data: bool,
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

#[derive(Clone)]
pub struct QueuePairDetail {
    pub qp_index: u32,
    pub local_qpn: u32,
    pub local_psn: u32,
    pub remote_qpn: u32,
    pub remote_psn: u32,
}

impl TestConfiguration {
    fn to_config_fields(&self) -> Vec<ConfigField> {
        let mut fields = vec![
            ConfigField::new("Device", &self.device),
            ConfigField::new("Transport", &self.transport),
            ConfigField::new("QP Count", self.qp_count),
            ConfigField::new("Connection Type", &self.connection_type),
            ConfigField::new("MTU", self.mtu),
            ConfigField::new("GID Type", &self.gid_type),
        ];

        // Only include Rx Depth for operations that use receive queues
        // Send operations always need Rx Depth, Write operations only if using immediate data
        let needs_rx = match self.test_type {
            TestType::SendLatency | TestType::SendBandwidth => true,
            TestType::WriteLatency | TestType::WriteBandwidth => self.uses_immediate_data,
            TestType::ReadLatency | TestType::ReadBandwidth => false,
        };

        if needs_rx {
            fields.push(ConfigField::new("Rx Depth", self.rx_depth));
        }

        fields.push(ConfigField::new("Tx Depth", self.tx_depth));
        fields.push(ConfigField::new("Post List", self.post_list));

        fields
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BandwidthResult {
    pub size: u32,
    pub iterations: u32,
    pub bandwidth: f64,
    pub msg_rate: f64,
    pub time: String,
}

impl TableRow for BandwidthResult {
    fn header() -> Vec<&'static str> {
        vec![
            "Size (B)",
            "Iterations",
            "Avg BW (Gb/s)",
            "MsgRate (Mpps)",
            "Time",
        ]
    }

    fn values(&self) -> Vec<String> {
        vec![
            self.size.to_string(),
            self.iterations.to_string(),
            format_bandwidth(&self.bandwidth),
            format_msg_rate(&self.msg_rate),
            self.time.clone(),
        ]
    }

    fn column_widths() -> Vec<usize> {
        vec![
            SIZE_COLUMN_WIDTH,
            ITERATIONS_COLUMN_WIDTH,
            BANDWIDTH_COLUMN_WIDTH,
            MSG_RATE_COLUMN_WIDTH,
            16, // Time column default width (matches original format)
        ]
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LatencyResult {
    pub size: u32,
    pub iterations: u32,
    pub min_latency: f64,
    pub max_latency: f64,
    pub avg_latency: f64,
    pub stdev_latency: f64,
    pub typical_latency: f64,
    pub p99_latency: f64,
    pub p999_latency: f64,
}

impl TableRow for LatencyResult {
    fn header() -> Vec<&'static str> {
        vec![
            "Size (B)",
            "Iterations",
            "Min (us)",
            "Max (us)",
            "P50 (us)",
            "Avg (us)",
            "Stdev (us)",
            "P99 (us)",
            "P999 (us)",
        ]
    }

    fn values(&self) -> Vec<String> {
        vec![
            self.size.to_string(),
            self.iterations.to_string(),
            format_common_float(&self.min_latency),
            format_common_float(&self.max_latency),
            format_common_float(&self.typical_latency),
            format_common_float(&self.avg_latency),
            format_common_float(&self.stdev_latency),
            format_common_float(&self.p99_latency),
            format_common_float(&self.p999_latency),
        ]
    }

    fn column_widths() -> Vec<usize> {
        vec![
            LAT_SIZE_COLUMN_WIDTH,
            LAT_ITER_COLUMN_WIDTH,
            LAT_MIN_COLUMN_WIDTH,
            LAT_MAX_COLUMN_WIDTH,
            LAT_TYP_COLUMN_WIDTH,
            LAT_AVG_COLUMN_WIDTH,
            LAT_STDEV_COLUMN_WIDTH,
            LAT_P99_COLUMN_WIDTH,
            10, // P999 column default width
        ]
    }
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
    output_config: OutputConfig,
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
        output_config: OutputConfig,
    ) -> Self {
        Self {
            config,
            qp_details,
            gid_info,
            bw_results: None,
            lat_results: None,
            bw_results_collection: Vec::new(),
            lat_results_collection: Vec::new(),
            output_config,
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

    // Print a bandwidth result immediately for streaming output
    pub fn print_bandwidth_result_immediately(
        &self,
        results: &BandwidthResult,
        header_width: usize,
    ) {
        let required_width = min_required_table_width();
        debug_assert!(
            header_width >= required_width,
            "Header width {} is insufficient for table minimum width {}",
            header_width,
            required_width
        );

        if self.output_config.tui_enabled {
            if let Err(e) = print_single_row_with_width(results, header_width) {
                eprintln!("Error displaying bandwidth result: {}", e);
            }
        }
    }

    // Print a latency result immediately for streaming output
    pub fn print_latency_result_immediately(&self, results: &LatencyResult, header_width: usize) {
        let required_width = min_required_latency_table_width();
        debug_assert!(
            header_width >= required_width,
            "Header width {} is insufficient for table minimum width {}",
            header_width,
            required_width
        );

        if self.output_config.tui_enabled {
            if let Err(e) = print_single_row_with_width(results, header_width) {
                eprintln!("Error displaying latency result: {}", e);
            }
        }
    }

    // Initialize streaming table headers for bandwidth results
    pub fn init_bandwidth_streaming_table(
        &self,
        header_width: usize,
    ) -> Result<Option<TableFormatter>, io::Error> {
        tui_println!(
            self,
            "{}",
            create_header("Bandwidth Results", header_width, DEFAULT_HEADER_MARGIN_LEN)
        );

        if self.output_config.tui_enabled {
            let mut formatter = TableFormatter::with_total_width::<BandwidthResult>(header_width);
            formatter.print_header()?;
            Ok(Some(formatter))
        } else {
            Ok(None)
        }
    }

    // Initialize streaming table headers for latency results
    pub fn init_latency_streaming_table(
        &self,
        header_width: usize,
    ) -> Result<Option<TableFormatter>, io::Error> {
        tui_println!(
            self,
            "{}",
            create_header("Latency Results", header_width, DEFAULT_HEADER_MARGIN_LEN)
        );

        if self.output_config.tui_enabled {
            let mut formatter = TableFormatter::with_total_width::<LatencyResult>(header_width);
            formatter.print_header()?;
            Ok(Some(formatter))
        } else {
            Ok(None)
        }
    }

    // Display only the headers and configuration, not the results tables
    pub fn display_headers_only(&self, header_width: usize) {
        tui_println!(
            self,
            "{}",
            create_header(
                &format!("{}", self.config.test_type),
                header_width,
                DEFAULT_HEADER_MARGIN_LEN
            )
        );
        tui_println!(self, "{}", self.config);

        tui_println!(
            self,
            "{}",
            create_header(
                "Connection Details",
                header_width,
                DEFAULT_HEADER_MARGIN_LEN
            )
        );
        tui_println!(self, "{}\n", self.format_qp_details());
    }

    // Display a consolidated table of bandwidth results
    pub fn display_bandwidth_collection(&self, header_width: usize) {
        if self.bw_results_collection.is_empty() {
            return;
        }

        tui_println!(
            self,
            "{}",
            create_header("Bandwidth Results", header_width, DEFAULT_HEADER_MARGIN_LEN)
        );

        if self.output_config.tui_enabled {
            if let Err(e) = print_table_with_width(&self.bw_results_collection, header_width) {
                eprintln!("Error displaying bandwidth results: {}", e);
            }
        }
    }

    // Display a consolidated table of latency results
    pub fn display_latency_collection(&self, header_width: usize) {
        if self.lat_results_collection.is_empty() {
            return;
        }

        tui_println!(
            self,
            "{}",
            create_header("Latency Results", header_width, DEFAULT_HEADER_MARGIN_LEN)
        );

        if self.output_config.tui_enabled {
            if let Err(e) = print_table_with_width(&self.lat_results_collection, header_width) {
                eprintln!("Error displaying latency results: {}", e);
            }
        }
    }

    fn format_qp_details(&self) -> String {
        let mut output = String::new();

        for detail in &self.qp_details {
            output.push_str(&format!(
                "{:padding$}QP #{:08}: (Local QPN: 0x{:04x} PSN: 0x{:06x}) -> (Remote QPN: 0x{:04x} PSN: 0x{:06x})\n",
                "",
                detail.qp_index, detail.local_qpn, detail.local_psn, detail.remote_qpn, detail.remote_psn,
                padding = KEY_VALUE_PREFIX_PADDING
            ));
        }

        output.push_str(&format!(
            "{:padding$}Local  GID: GID: {}\n",
            "",
            self.gid_info[0],
            padding = KEY_VALUE_PREFIX_PADDING
        ));
        output.push_str(&format!(
            "{:padding$}Remote GID: GID: {}",
            "",
            self.gid_info[1],
            padding = KEY_VALUE_PREFIX_PADDING
        ));

        output
    }

    pub fn display(&self) {
        let header_width = header_width!(for_test self.config.test_type);

        tui_println!(
            self,
            "{}",
            create_header(
                &format!("{}", self.config.test_type),
                header_width,
                DEFAULT_HEADER_MARGIN_LEN
            )
        );
        tui_println!(self, "{}", self.config);

        tui_println!(
            self,
            "{}",
            create_header(
                "Connection Details",
                header_width,
                DEFAULT_HEADER_MARGIN_LEN
            )
        );
        tui_println!(self, "{}\n", self.format_qp_details());

        if let Some(results) = &self.bw_results {
            tui_println!(
                self,
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

            if let Err(e) = print_single_row_with_width(results, table_width) {
                eprintln!("Error displaying bandwidth result: {}", e);
            }
        }

        if let Some(results) = &self.lat_results {
            tui_println!(
                self,
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

            if let Err(e) = print_single_row_with_width(results, table_width) {
                eprintln!("Error displaying latency result: {}", e);
            }
        }

        if !self.bw_results_collection.is_empty() {
            self.display_bandwidth_collection(header_width);
        }

        if !self.lat_results_collection.is_empty() {
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
        KEY_VALUE_PREFIX_PADDING
            + self.max_key_width
            + self.max_value_width
            + DEFAULT_KEY_VALUE_SPACING
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
                        "{:padding$}{:<key_width$}:{:spacing$}{:<value_width$}",
                        "",
                        field.key,
                        "",
                        field.value,
                        padding = KEY_VALUE_PREFIX_PADDING,
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
