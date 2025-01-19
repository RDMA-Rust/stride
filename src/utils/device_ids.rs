//! Device identification tables for various RDMA vendors.
//! References:
//! - Broadcom: https://github.com/linux-rdma/rdma-core/blob/master/providers/bnxt_re/main.c
//! - MLX5: https://github.com/linux-rdma/rdma-core/blob/master/providers/mlx5/mlx5.c
//! - Intel: https://github.com/linux-rdma/rdma-core/blob/master/providers/irdma/ice_devids.h
//!          https://github.com/linux-rdma/rdma-core/blob/master/providers/irdma/i40e_devids.h
//! - Other vendors...
use serde::{Deserialize, Serialize};
use strum::Display;

// Vendor IDs
pub const VENDOR_ID_ALIBABA: u32 = 0x1ded;
pub const VENDOR_ID_AMAZON: u32 = 0x1d0f;
pub const VENDOR_ID_BROADCOM: u32 = 0x14e4;
pub const VENDOR_ID_CHELSIO: u32 = 0x1425;
pub const VENDOR_ID_HISILICON: u32 = 0x19e5;
pub const VENDOR_ID_INTEL: u32 = 0x8086;
pub const VENDOR_ID_NVIDIA: u32 = 0x15b3;
pub const VENDOR_ID_NVIDIA_VF: u32 = 0x02c9;
pub const VENDOR_ID_QLOGIC: u32 = 0x1077;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Vendor {
    Alibaba(AlibabaModel),
    Amazon(AmazonModel),
    Broadcom(BroadcomModel),
    Chelsio(ChelsioModel),
    HiSilicon(HiSiliconModel),
    Intel(IntelModel),
    Nvidia(NvidiaModel),
    QLogic(QLogicModel),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
pub enum AlibabaModel {
    ERDMA,

    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
pub enum AmazonModel {
    EFA,

    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
pub enum BroadcomModel {
    NetXtreme,
    NetXtremeE,
    NetXtremeC,

    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
pub enum ChelsioModel {
    ChelsioT4,
    ChelsioT5,
    ChelsioT6,

    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
pub enum HiSiliconModel {
    HNS,

    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
pub enum IntelModel {
    C822N,
    E810C,
    E810XXV,
    E822C,
    E822L,
    E823C,
    E823L,

    X722,
    XL710,

    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
pub enum NvidiaModel {
    // ConnectX series
    ConnectIB,
    ConnectX4,
    ConnectX4Lx,
    ConnectX5,
    ConnectX5Ex,
    ConnectX6,
    ConnectX6Dx,
    ConnectX6Lx,
    ConnectX7,
    ConnectX8,
    ConnectX9,

    // BlueField DPU series
    BlueField,
    BlueField2,
    BlueField3,
    BlueField4,

    // Generated Virtual Function
    Mlx5GenVF,

    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
pub enum QLogicModel {
    QL41000,
    QL45000,

    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
pub enum FunctionType {
    PhysicalFunction,
    VirtualFunction,
    ScalableFunction,

    Unknown,
}

#[derive(Debug, Clone)]
pub struct DeviceIdEntry<T> {
    pub part_id: u32,
    pub model: T,
    pub is_vf: bool,
    pub comment: &'static str,
}

macro_rules! device_list {
    ($(($part_id:expr, $model:expr, $is_vf:expr, $comment:expr)),* $(,)?) => {
        &[
            $(
                DeviceIdEntry {
                    part_id: $part_id,
                    model: $model,
                    is_vf: $is_vf,
                    comment: $comment,
                },
            )*
        ]
    };
}

// Alibaba Devices
#[rustfmt::skip]
pub const ALIBABA_DEVICES: &[DeviceIdEntry<AlibabaModel>] = device_list![
    (0x107f, AlibabaModel::ERDMA, false, "Alibaba Elastic RDMA Adapter"),
];

// Amazon Devices
#[rustfmt::skip]
pub const AMAZON_DEVICES: &[DeviceIdEntry<AmazonModel>] = device_list![
    (0xefa0, AmazonModel::EFA, false, "Amazon EFA"),
    (0xefa1, AmazonModel::EFA, false, "Amazon EFA"),
    (0xefa2, AmazonModel::EFA, false, "Amazon EFA"),
];

// Broadcom Devices
#[rustfmt::skip]
pub const BROADCOM_DEVICES: &[DeviceIdEntry<BroadcomModel>] = device_list![
    (0x1605, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57454 NPAR"),
    (0x1606, BroadcomModel::NetXtremeE, true, "Broadcom NetXtreme-E BCM57454 Virtual Function"),
    (0x1614, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57454"),
    (0x16c0, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57417 NPAR"),
    (0x16c1, BroadcomModel::NetXtremeE, true, "Broadcom NetXtreme-E BCM57414 Virtual Function"),
    (0x16ce, BroadcomModel::NetXtremeC, false, "Broadcom NetXtreme-C BCM57311"),
    (0x16cf, BroadcomModel::NetXtremeC, false, "Broadcom NetXtreme-C BCM57312"),
    (0x16d6, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57412"),
    (0x16d7, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57414"),
    (0x16d8, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57416 Cu"),
    (0x16d9, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57417 Cu"),
    (0x16df, BroadcomModel::NetXtremeC, false, "Broadcom NetXtreme-C BCM57314"),
    (0x16e2, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57417"),
    (0x16e3, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57416"),
    (0x16e5, BroadcomModel::NetXtremeC, true, "Broadcom NetXtreme-C BCM57314 Virtual Function"),
    (0x16ed, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57414 NPAR"),
    (0x16eb, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57412 NPAR"),
    (0x16ef, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57416 NPAR"),
    (0x1750, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57508"),
    (0x1751, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57504"),
    (0x1752, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57502"),
    (0x1803, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57508 NPAR"),
    (0x1804, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57504 NPAR"),
    (0x1805, BroadcomModel::NetXtremeE, false, "Broadcom NetXtreme-E BCM57502 NPAR"),
    (0x1807, BroadcomModel::NetXtremeE, true, "Broadcom NetXtreme-E BCM5750X Virtual Function"),
    (0x1809, BroadcomModel::NetXtremeE, true, "Broadcom NetXtreme-E BCM5750X Gen P5 Virtual Function HV"),
    (0xd800, BroadcomModel::NetXtreme, true, "Broadcom NetXtreme BCM880XX Virtual Function"),
    (0xd802, BroadcomModel::NetXtreme, false, "Broadcom NetXtreme BCM58802"),
    (0xd804, BroadcomModel::NetXtreme, false, "Broadcom NetXtreme BCM8804 SR"),
];

// Chelsio Devices
#[rustfmt::skip]
pub const CHELSIO_DEVICES: &[DeviceIdEntry<ChelsioModel>] = device_list![
    (0x4000, ChelsioModel::ChelsioT4, false, "Chelsio T440-DBG"),
    (0x4001, ChelsioModel::ChelsioT4, false, "Chelsio T420-CR"),
    (0x4002, ChelsioModel::ChelsioT4, false, "Chelsio T422-CR"),
    (0x4003, ChelsioModel::ChelsioT4, false, "Chelsio T440-CR"),
    (0x5000, ChelsioModel::ChelsioT5, false, "Chelsio T580-DBG"),
    (0x5001, ChelsioModel::ChelsioT5, false, "Chelsio T520-CR"),
    (0x5002, ChelsioModel::ChelsioT5, false, "Chelsio T522-CR"),
    (0x5003, ChelsioModel::ChelsioT5, false, "Chelsio T540-CR"),
    (0x6001, ChelsioModel::ChelsioT5, false, "Chelsio T6225-CR"),
    (0x6002, ChelsioModel::ChelsioT5, false, "Chelsio T6225-SO-CR"),
    (0x6003, ChelsioModel::ChelsioT5, false, "Chelsio T6425-CR"),
];

// Nvidia Devices
#[rustfmt::skip]
pub const NVIDIA_DEVICES: &[DeviceIdEntry<NvidiaModel>] = device_list![
    (0x1011, NvidiaModel::ConnectIB, false, "Connect-IB"),
    (0x1012, NvidiaModel::ConnectIB, true, "Connect-IB Virtual Function"),
    (0x1013, NvidiaModel::ConnectX4, false, "ConnectX-4"),
    (0x1014, NvidiaModel::ConnectX4, true, "ConnectX-4 Virtual Function"),
    (0x1015, NvidiaModel::ConnectX4Lx, false, "ConnectX-4 Lx"),
    (0x1016, NvidiaModel::ConnectX4Lx, true, "ConnectX-4 Lx Virtual Function"),
    (0x1017, NvidiaModel::ConnectX5, false, "ConnectX-5 PCIe 3.0"),
    (0x1018, NvidiaModel::ConnectX5, true, "ConnectX-5 Virtual Function"),
    (0x1019, NvidiaModel::ConnectX5Ex, false, "ConnectX-5 Ex"),
    (0x101a, NvidiaModel::ConnectX5Ex, true, "ConnectX-5 Ex Virtual Function"),
    (0x101b, NvidiaModel::ConnectX6, false, "ConnectX-6"),
    (0x101c, NvidiaModel::ConnectX6, true, "ConnectX-6 Virtual Function"),
    (0x101d, NvidiaModel::ConnectX6Dx, false, "ConnectX-6 Dx"),
    (0x101e, NvidiaModel::Mlx5GenVF, true, "ConnectX family mlx5Gen Virtual Function"),
    (0x101f, NvidiaModel::ConnectX6Lx, false, "ConnectX-6 Lx"),
    (0x1021, NvidiaModel::ConnectX7, false, "ConnectX-7"),
    (0x1023, NvidiaModel::ConnectX8, false, "ConnectX-8"),
    (0x1025, NvidiaModel::ConnectX9, false, "ConnectX-9"),
    (0xa2d2, NvidiaModel::BlueField, false, "BlueField integrated ConnectX-5 network controller"),
    (0xa2d3, NvidiaModel::BlueField, true, "BlueField integrated ConnectX-5 network controller Virtual Function"),
    (0xa2d6, NvidiaModel::BlueField2, false, "BlueField-2 integrated ConnectX-6 Dx network controller"),
    (0xa2dc, NvidiaModel::BlueField3, false, "BlueField-3 integrated ConnectX-7 network controller"),
    (0xa2df, NvidiaModel::BlueField4, false, "BlueField-4 integrated ConnectX-8 network controller"),
];

// HiSilicon Devices
#[rustfmt::skip]
pub const HISILICON_DEVICES: &[DeviceIdEntry<HiSiliconModel>] = device_list![
    (0xa222, HiSiliconModel::HNS, false, "HNS GE/10GE/25GE RDMA Network Controller"),
    (0xa223, HiSiliconModel::HNS, false, "HNS GE/10GE/25GE RDMA Network Controller"),
    (0xa224, HiSiliconModel::HNS, false, "HNS GE/10GE/25GE/50GE RDMA Network Controller"),
    (0xa225, HiSiliconModel::HNS, false, "HNS GE/10GE/25GE/50GE RDMA Network Controller"),
    (0xa226, HiSiliconModel::HNS, false, "HNS GE/10GE/25GE/50GE/100GE RDMA Network Controller"),
    (0xa227, HiSiliconModel::HNS, false, "HNS GE/10GE/25GE/50GE/100GE RDMA Network Controller"),
    (0xa228, HiSiliconModel::HNS, false, "HNS GE/10GE/25GE/50GE/100GE/200GE RDMA Network Controller"),
    (0xa22f, HiSiliconModel::HNS, true, "HNS GE/10GE/25GE/50GE/100GE/200GE RDMA Network Controller Virtual Function"),
];

// Intel Devices
#[rustfmt::skip]
pub const INTEL_DEVICES: &[DeviceIdEntry<IntelModel>] = device_list![
    (0x1572, IntelModel::XL710, false, "Intel XL710"),
    (0x374c, IntelModel::X722, false, "Intel X722"),
];

// QLogic Devices
#[rustfmt::skip]
pub const QLOGIC_DEVICES: &[DeviceIdEntry<QLogicModel>] = device_list![
    (0x1629, QLogicModel::QL45000, false, "QLogic FastLinQ QL45000 Series"),
    (0x1634, QLogicModel::QL45000, false, "QLogic FastLinQ QL45000 Series 40GbE Controller"),
    (0x1636, QLogicModel::QL45000, false, "QLogic FastLinQ QL45000 Series"),
    (0x1644, QLogicModel::QL45000, false, "QLogic FastLinQ QL45000 Series 100GbE Controller"),
    (0x1654, QLogicModel::QL45000, false, "QLogic FastLinQ QL45000 Series 50GbE Controller"),
    (0x1656, QLogicModel::QL45000, false, "QLogic FastLinQ QL45000 Series 25GbE Controller"),
    (0x1664, QLogicModel::QL45000, true, "QLogic FastLinQ QL45000 Series Virtual Function"),
    (0x1666, QLogicModel::QL45000, false, "QLogic FastLinQ QL45000 Series 10GbE Controller"),
    (0x8070, QLogicModel::QL41000, false, "QLogic FastLinQ QL41000 Series"),
    (0x8090, QLogicModel::QL41000, true, "QLogic FastLinQ QL41000 Series Virtual Function"),
    (0x8170, QLogicModel::QL41000, false, "QLogic FastLinQ QL41000 Series"),
    (0x8190, QLogicModel::QL41000, true, "QLogic FastLinQ QL41000 Series Virtual Function"),
];

// Helper functions for identifying devices
pub fn is_vf_by_pattern(vendor_id: u32, part_id: u32) -> bool {
    match vendor_id {
        VENDOR_ID_NVIDIA | VENDOR_ID_NVIDIA_VF => (part_id & 1) == 1,
        VENDOR_ID_INTEL => matches!(part_id, 0x37cd | 0x37d9),
        _ => false,
    }
}

pub trait VendorLookup {
    type Model;
    fn lookup_device(part_id: u32) -> Option<&'static DeviceIdEntry<Self::Model>>;
    fn fallback_model() -> Self::Model;
    fn create_vendor(model: Self::Model) -> Vendor;
}

macro_rules! impl_vendor_lookup {
    ($vendor:ident) => {
        paste::paste! {
            impl VendorLookup for [<$vendor Model>] {
                type Model = Self;

                fn lookup_device(part_id: u32) -> Option<&'static DeviceIdEntry<Self::Model>> {
                    [<$vendor:upper _DEVICES>].iter().find(|entry| entry.part_id == part_id)
                }

                fn fallback_model() -> Self::Model {
                    Self::Unknown
                }

                fn create_vendor(model: Self::Model) -> Vendor {
                    Vendor::$vendor(model)
                }
            }
        }
    };
}

impl_vendor_lookup!(Alibaba);
impl_vendor_lookup!(Amazon);
impl_vendor_lookup!(Broadcom);
impl_vendor_lookup!(Chelsio);
impl_vendor_lookup!(HiSilicon);
impl_vendor_lookup!(Intel);
impl_vendor_lookup!(Nvidia);
impl_vendor_lookup!(QLogic);

pub(crate) fn lookup_vendor<T>(vendor_id: u32, part_id: u32) -> (Vendor, FunctionType, &'static str)
where
    T: VendorLookup,
    T::Model: Copy,
    <T as VendorLookup>::Model: 'static,
{
    if let Some(entry) = T::lookup_device(part_id) {
        (
            T::create_vendor(entry.model),
            if entry.is_vf {
                FunctionType::VirtualFunction
            } else {
                FunctionType::PhysicalFunction
            },
            entry.comment,
        )
    } else {
        let is_vf = is_vf_by_pattern(vendor_id, part_id);
        (
            T::create_vendor(T::fallback_model()),
            if is_vf {
                FunctionType::VirtualFunction
            } else {
                FunctionType::PhysicalFunction
            },
            "",
        )
    }
}
