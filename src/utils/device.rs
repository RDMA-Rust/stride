use serde::{Deserialize, Serialize};
use sideway::ibverbs::device::DeviceInfo;
use sideway::ibverbs::device_context::DeviceAttr;

use super::device_ids::{
    lookup_vendor, AlibabaModel, AmazonModel, BroadcomModel, ChelsioModel, FunctionType,
    HiSiliconModel, IntelModel, NvidiaModel, QLogicModel, Vendor, VENDOR_ID_ALIBABA,
    VENDOR_ID_AMAZON, VENDOR_ID_BROADCOM, VENDOR_ID_CHELSIO, VENDOR_ID_HISILICON, VENDOR_ID_INTEL,
    VENDOR_ID_NVIDIA, VENDOR_ID_NVIDIA_VF, VENDOR_ID_QLOGIC,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDetail {
    pub name: String,
    pub vendor: Vendor,
    pub product_desc: &'static str,
    pub numa_node: i32,
    pub function_type: FunctionType,
    pub firmware_version: String,
}

impl DeviceDetail {
    pub fn from_device_attr(attr: &DeviceAttr) -> Self {
        let vendor_id = attr.vendor_id();
        let part_id = attr.vendor_part_id();
        let firmware_version = attr.firmware_version();

        let (vendor, function_type, product_desc) = match vendor_id {
            VENDOR_ID_ALIBABA => lookup_vendor::<AlibabaModel>(vendor_id, part_id),
            VENDOR_ID_AMAZON => lookup_vendor::<AmazonModel>(vendor_id, part_id),
            VENDOR_ID_BROADCOM => lookup_vendor::<BroadcomModel>(vendor_id, part_id),
            VENDOR_ID_CHELSIO => lookup_vendor::<ChelsioModel>(vendor_id, part_id),
            VENDOR_ID_HISILICON => lookup_vendor::<HiSiliconModel>(vendor_id, part_id),
            VENDOR_ID_INTEL => lookup_vendor::<IntelModel>(vendor_id, part_id),
            VENDOR_ID_NVIDIA | VENDOR_ID_NVIDIA_VF => {
                lookup_vendor::<NvidiaModel>(vendor_id, part_id)
            }
            VENDOR_ID_QLOGIC => lookup_vendor::<QLogicModel>(vendor_id, part_id),
            _ => (Vendor::Unknown, FunctionType::Unknown, ""),
        };

        DeviceDetail {
            name: "".to_string(),
            numa_node: -1,
            vendor,
            product_desc,
            function_type,
            firmware_version,
        }
    }

    pub fn description(&self) -> String {
        format!(
            "{} ({:?}, FW: {})",
            self.product_desc, self.function_type, self.firmware_version
        )
    }
}

pub fn probe_devices(
    detailed: bool,
    _show_numa: bool,
    filter_device: Option<&str>,
    tui_enabled: bool,
) -> anyhow::Result<()> {
    let device_list = sideway::ibverbs::device::DeviceList::new()?;

    for device in &device_list {
        if let Some(filter) = filter_device {
            if device.name() != filter {
                continue;
            }
        }

        let context = device.open()?;
        let attr = context.query_device()?;
        let info = DeviceDetail::from_device_attr(&attr);

        if tui_enabled {
            println!("{}: {}", context.name(), info.description());
        }

        if detailed {
            // Print detailed device information
            // ...
        }

        for i in 1..=attr.phys_port_cnt() {
            let port_attr = context.query_port(i)?;
            if tui_enabled {
                println!(
                    "    Port {i}: {:?} ({} Gbps), {:?}, total data rate: {} Gbps",
                    port_attr.active_speed(),
                    port_attr.active_speed().to_throughput(),
                    port_attr.active_width(),
                    port_attr.active_speed().to_throughput()
                        * ((port_attr.active_width() as u32) as f64)
                );
            }
        }
    }

    Ok(())
}
