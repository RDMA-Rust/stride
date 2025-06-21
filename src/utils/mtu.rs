use anyhow::Result;
use sideway::ibverbs::device::DeviceInfo;
use sideway::ibverbs::device_context::{DeviceContext, Mtu};
use tracing::{debug, warn};

/// Query the maximum MTU supported by a specific port
pub fn query_port_max_mtu(ctx: &DeviceContext, port_num: u8) -> Result<Mtu> {
    let port_attr = ctx.query_port(port_num)?;
    let max_mtu = port_attr.max_mtu();

    debug!(
        device = ctx.name(),
        port = port_num,
        max_mtu = ?max_mtu,
        active_mtu = ?port_attr.active_mtu(),
        "Queried port MTU capabilities"
    );

    Ok(max_mtu)
}

/// Negotiate MTU between requested and maximum supported
/// Returns the negotiated MTU and whether it was downgraded
pub fn negotiate_mtu(
    requested_mtu: Mtu,
    max_supported_mtu: Mtu,
    device_name: &str,
    port_num: u8,
) -> (Mtu, bool) {
    let requested_value = mtu_to_value(requested_mtu);
    let max_value = mtu_to_value(max_supported_mtu);

    if requested_value <= max_value {
        // Requested MTU is acceptable
        debug!(
            device = device_name,
            port = port_num,
            requested_mtu = ?requested_mtu,
            "MTU negotiation: Using requested MTU"
        );
        (requested_mtu, false)
    } else {
        // Need to downgrade to maximum supported
        warn!(
            device = device_name,
            port = port_num,
            requested_mtu = ?requested_mtu,
            negotiated_mtu = ?max_supported_mtu,
            "MTU negotiation: Downgrading MTU from {:?} to {:?} (device maximum)",
            requested_mtu, max_supported_mtu
        );
        (max_supported_mtu, true)
    }
}

/// Convert MTU enum to numeric value for comparison
pub fn mtu_to_value(mtu: Mtu) -> u32 {
    match mtu {
        Mtu::Mtu256 => 256,
        Mtu::Mtu512 => 512,
        Mtu::Mtu1024 => 1024,
        Mtu::Mtu2048 => 2048,
        Mtu::Mtu4096 => 4096,
    }
}

/// Convert numeric value to MTU enum, selecting the largest MTU that doesn't exceed the value
pub fn value_to_mtu(value: u32) -> Mtu {
    match value {
        0..=256 => Mtu::Mtu256,
        257..=512 => Mtu::Mtu512,
        513..=1024 => Mtu::Mtu1024,
        1025..=2048 => Mtu::Mtu2048,
        _ => Mtu::Mtu4096,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mtu_to_value() {
        assert_eq!(mtu_to_value(Mtu::Mtu256), 256);
        assert_eq!(mtu_to_value(Mtu::Mtu512), 512);
        assert_eq!(mtu_to_value(Mtu::Mtu1024), 1024);
        assert_eq!(mtu_to_value(Mtu::Mtu2048), 2048);
        assert_eq!(mtu_to_value(Mtu::Mtu4096), 4096);
    }

    #[test]
    fn test_value_to_mtu() {
        assert_eq!(value_to_mtu(256), Mtu::Mtu256);
        assert_eq!(value_to_mtu(300), Mtu::Mtu512);
        assert_eq!(value_to_mtu(1024), Mtu::Mtu1024);
        assert_eq!(value_to_mtu(1500), Mtu::Mtu2048);
        assert_eq!(value_to_mtu(5000), Mtu::Mtu4096);
    }

    #[test]
    fn test_negotiate_mtu() {
        // Test no downgrade needed
        let (negotiated, downgraded) = negotiate_mtu(Mtu::Mtu1024, Mtu::Mtu4096, "test", 1);
        assert_eq!(negotiated, Mtu::Mtu1024);
        assert!(!downgraded);

        // Test downgrade needed
        let (negotiated, downgraded) = negotiate_mtu(Mtu::Mtu4096, Mtu::Mtu1024, "test", 1);
        assert_eq!(negotiated, Mtu::Mtu1024);
        assert!(downgraded);
    }

    #[test]
    fn test_simple_negotiation() {
        // Test that local validation works correctly
        let (negotiated, downgraded) = negotiate_mtu(Mtu::Mtu4096, Mtu::Mtu2048, "test", 1);
        assert_eq!(negotiated, Mtu::Mtu2048);
        assert!(downgraded);

        // Test that no downgrade is needed
        let (negotiated, downgraded) = negotiate_mtu(Mtu::Mtu1024, Mtu::Mtu4096, "test", 1);
        assert_eq!(negotiated, Mtu::Mtu1024);
        assert!(!downgraded);
    }
}
