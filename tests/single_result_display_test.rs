use sideway::ibverbs::address::Gid;
use stride::cli::plan::OutputConfig;
use stride::utils::display::{
    BandwidthResult, DisplayOutput, QueuePairDetail, TestConfiguration, TestType,
};

#[test]
fn test_single_bandwidth_result_display_with_footer() {
    println!("Testing single bandwidth result display with footer...");

    // Create test configuration similar to actual usage
    let config = TestConfiguration {
        device: "mlx5_0".to_string(),
        transport: "InfiniBand".to_string(),
        qp_count: 1,
        connection_type: "RC".to_string(),
        mtu: 2048,
        gid_type: "RoceV1".to_string(),
        rx_depth: 512,
        tx_depth: 128,
        post_list: 1,
        test_type: TestType::WriteBandwidth,
        uses_immediate_data: false, // This test uses regular write, not write-with-imm
    };

    let qp_details = vec![QueuePairDetail {
        qp_index: 0,
        local_qpn: 0x017b,
        local_psn: 0xe64072,
        remote_qpn: 0x017a,
        remote_psn: 0xe63be7,
    }];

    let gid = Gid::default();
    let gid_info = vec![gid, gid];

    let output_config = OutputConfig {
        tui_enabled: true,
        trace_enabled: false,
    };

    let mut display = DisplayOutput::new(config, qp_details, gid_info, output_config);

    // Create a bandwidth result similar to your example
    let bw_result = BandwidthResult {
        size: 65536,
        iterations: 1000,
        bandwidth: 92.6333,
        msg_rate: 0.1767,
        time: "0.01".to_string(),
    };

    // Set the result and display it
    display.set_bandwidth_results(bw_result);

    println!("--- Starting display output ---");
    display.display();
    println!("--- End display output ---");

    println!("If you see a footer line after the bandwidth result above, the fix is working!");
}
