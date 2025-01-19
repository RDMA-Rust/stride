use anyhow::Result;
use std::pin::Pin;

use crate::memory::MemoryOps;

pub struct SystemMemory {
    data: Pin<Box<[u8]>>,
    numa_node: Option<i32>,
}

impl SystemMemory {
    pub fn new(size: usize, numa_node: Option<i32>) -> Result<Self> {
        let vec = vec![0u8; size];
        Ok(SystemMemory {
            data: Pin::new(vec.into_boxed_slice()),
            numa_node,
        })
    }
}

impl MemoryOps for SystemMemory {
    fn size(&self) -> usize {
        self.data.len()
    }

    fn write(&mut self, offset: usize, data: &[u8]) -> Result<()> {
        let available = self.data.len().saturating_sub(offset);
        let len = available.min(data.len());
        self.data[offset..offset + len].copy_from_slice(&data[..len]);
        Ok(())
    }

    fn read(&self, offset: usize, data: &mut [u8]) -> Result<()> {
        let available = self.data.len().saturating_sub(offset);
        let len = available.min(data.len());
        data[..len].copy_from_slice(&self.data[offset..offset + len]);
        Ok(())
    }

    fn get_handle(&self) -> usize {
        self.data.as_ptr() as usize
    }
}
