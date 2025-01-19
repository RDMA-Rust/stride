use rand::Rng;

/// Generate a random Packet Sequence Number (PSN)
/// Only the lower 24 bits are valid per the InfiniBand specification
pub fn generate_psn() -> u32 {
    let mut rng = rand::thread_rng();
    rng.gen::<u32>() & 0x00FFFFFF
}
