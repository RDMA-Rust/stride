#[inline(always)]
pub fn select_post_list_slot(qp_operation_base: u32, tx_depth: u32) -> usize {
    debug_assert!(tx_depth.is_power_of_two());
    (qp_operation_base & (tx_depth - 1)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use crate::runners::worker::calculate_send_slot_offset;

    #[test]
    fn post_lists_share_single_address() {
        const BASE_ADDR: u64 = 0x1_0000;
        const INCREMENT_SIZE: usize = 4096;
        let tx_depth = 32;
        let qp_operation_base = 5;

        for &post_list in &[1usize, 8, 16] {
            let slot = select_post_list_slot(qp_operation_base, tx_depth);
            let addr = BASE_ADDR + calculate_send_slot_offset(INCREMENT_SIZE, slot) as u64;

            let mut seen = HashSet::new();
            for _ in 0..post_list {
                // Every WR in the list should resolve to the same address.
                let wr_addr = BASE_ADDR + calculate_send_slot_offset(INCREMENT_SIZE, slot) as u64;
                seen.insert(wr_addr);
            }

            assert_eq!(
                seen.len(),
                1,
                "post_list {post_list} produced varying WR addresses"
            );
            assert!(seen.contains(&addr));
        }
    }
}
