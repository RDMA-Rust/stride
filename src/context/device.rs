use sideway::ibverbs::{
    device::{DeviceInfo, DeviceList},
    device_context::DeviceContext,
};

pub fn open_device_context(device_name: Option<&str>) -> Result<DeviceContext, String> {
    let list = DeviceList::new().map_err(|e| format!("Failed to get device list: {e}"))?;

    if list.is_empty() {
        return Err("No RDMA devices found on this system".to_string());
    }

    let device = match device_name {
        Some(name) => list
            .iter()
            .find(|dev| dev.name() == name)
            .ok_or_else(|| format!("Device {name} not found"))?,
        None => {
            let device = list.get(0).unwrap();
            println!(
                "No device specified, using first avaiable device: {}",
                device.name()
            );
            device
        }
    };

    let ctx = device.open().unwrap();

    Ok(ctx)
}
