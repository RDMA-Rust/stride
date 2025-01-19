use sideway::ibverbs::address::GidEntry;
use std::fmt::Display;
use tabled::{
    settings::{object::Columns, Style, Width},
    Table, Tabled,
};

// Display configuration constants
pub const DEFAULT_HEADER_WIDTH: usize = 90;
pub const DEFAULT_ROWS_PER_COLUMN: usize = 10;
pub const DEFAULT_COLUMN_SPACING: usize = 10;
pub const DEFAULT_KEY_VALUE_SPACING: usize = 1; // Includes ":    " spacing
pub const DEFAULT_HEADER_MARGIN_LEN: usize = 2; // Space length around header text

macro_rules! header_width {
    () => {
        DEFAULT_HEADER_WIDTH
    };
    ($w:expr) => {
        $w
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
}

#[derive(Tabled)]
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

#[derive(Tabled)]
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

pub struct DisplayOutput {
    config: TestConfiguration,
    qp_details: Vec<QueuePairDetail>,
    gid_info: Vec<GidEntry>,
    results: Option<BandwidthResult>,
}

fn format_bandwidth(f: &f64) -> String {
    format!("{:.4}", f)
}

fn format_msg_rate(f: &f64) -> String {
    format!("{:.4}", f)
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
        gid_info: Vec<GidEntry>,
    ) -> Self {
        Self {
            config,
            qp_details,
            gid_info,
            results: None,
        }
    }

    pub fn set_results(&mut self, results: BandwidthResult) {
        self.results = Some(results);
    }

    fn format_qp_details(&self) -> String {
        let mut output = String::new();

        for detail in &self.qp_details {
            output.push_str(&format!(
                "QP #{:08}: (Local QPN: 0x{:04x} PSN: 0x{:06x}) -> (Remote QPN: 0x{:04x} PSN: 0x{:06x})\n",
                detail.qp_index, detail.local_qpn, detail.local_psn, detail.remote_qpn, detail.remote_psn
            ));
        }

        unsafe {
            output.push_str(&format!(
                "Local  GID: Index: {}, GID: {}\n",
                self.gid_info.get(0).unwrap_unchecked().gid_index(),
                self.gid_info.get(0).unwrap_unchecked().gid(),
            ));
            output.push_str(&format!(
                "Remote GID: Index: {}, GID: {}",
                self.gid_info.get(1).unwrap_unchecked().gid_index(),
                self.gid_info.get(1).unwrap_unchecked().gid(),
            ));
        }

        output
    }

    pub fn display(&self) {
        println!(
            "{}",
            create_header(
                "Test Configuration",
                header_width!(),
                DEFAULT_HEADER_MARGIN_LEN
            )
        );
        println!("{}", self.config);

        println!(
            "{}",
            create_header("QP Details", header_width!(), DEFAULT_HEADER_MARGIN_LEN)
        );
        println!("{}\n", self.format_qp_details());

        if let Some(results) = &self.results {
            println!(
                "{}",
                create_header("Results", header_width!(), DEFAULT_HEADER_MARGIN_LEN)
            );
            let table = Table::new([results])
                .with(Style::psql())
                .with(Width::wrap(header_width!()))
                .with(Width::increase(header_width!()))
                .modify(Columns::single(0), Width::increase(10))
                .to_string();
            println!("{}", table);
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
        let total_columns = (total_fields + rows_per_column - 1) / rows_per_column;

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
