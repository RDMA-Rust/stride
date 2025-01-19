use anyhow::Result;

pub mod system;

pub trait MemoryOps {
    fn size(&self) -> usize;

    fn write(&mut self, offset: usize, src: &[u8]) -> Result<()>;

    fn read(&self, offset: usize, dst: &mut [u8]) -> Result<()>;

    fn get_handle(&self) -> usize;
}
