use sideway::ibverbs::{
    device::{DeviceInfo, DeviceList},
    device_context::DeviceContext,
};

pub fn open_device_context(device_name: &str) -> Result<DeviceContext, String> {
    let list = DeviceList::new().unwrap();

    let device = list.iter().find(|dev| dev.name() == device_name).unwrap();

    let ctx = device.open().unwrap();

    Ok(ctx)
}
