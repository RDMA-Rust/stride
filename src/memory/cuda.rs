use anyhow::Result;
use cudarc::driver::{CudaContext, CudaSlice, DevicePtr, sys::CUdeviceptr, result::malloc_sync};
use std::sync::Arc;

use crate::memory::MemoryOps;

/// CUDA-backed memory region.
///
/// This struct owns a CUDA allocation and exposes a host-side
/// view that implements the `MemoryOps` trait so it can be
/// used anywhere the existing memory backends are used.
pub struct CudaMemory {
    /// Device allocation.
    data: CUdeviceptr,
    /// CUDA device id this allocation lives on.
    cuda_device_id: u32,
    /// Shared CUDA context so we can fetch streams / synchronize.
    cuda_ctx: Arc<CudaContext>,
    /// Host-side shadow buffer used to satisfy `read`/`write`
    /// requirements of `MemoryOps`.
    ///
    /// NOTE: For now this buffer is *not* automatically kept in
    /// sync with the device allocation. The primary consumer of
    /// this type in `stride` uses the device pointer for RDMA
    /// registrations via `get_handle()`. If you need host/device
    /// data synchronization, add explicit copy calls using `cudarc`.
    host_shadow: Vec<u8>,
}

impl CudaMemory {
    /// Create a new CUDA memory region on the given device.
    pub fn new(size: usize, cuda_device_id: u32) -> Result<Self> {
        // Initialize CUDA context for the requested device.
        let cuda_ctx = CudaContext::new(cuda_device_id as usize)?;

        // Allocate zero-initialized device memory using the default stream.
        let stream = cuda_ctx.default_stream();
        let data = unsafe { malloc_sync(size)? };

        // Ensure the allocation is visible before we hand it out.
        stream.synchronize()?;

        Ok(CudaMemory {
            host_shadow: vec![0u8; size],
            data,
            cuda_device_id,
            cuda_ctx,
        })
    }
}

impl MemoryOps for CudaMemory {
    fn size(&self) -> usize {
        self.host_shadow.len()
    }

    fn write(&mut self, offset: usize, src: &[u8]) -> Result<()> {
        let available = self.host_shadow.len().saturating_sub(offset);
        let len = available.min(src.len());
        self.host_shadow[offset..offset + len].copy_from_slice(&src[..len]);
        Ok(())
    }

    fn read(&self, offset: usize, dst: &mut [u8]) -> Result<()> {
        let available = self.host_shadow.len().saturating_sub(offset);
        let len = available.min(dst.len());
        dst[..len].copy_from_slice(&self.host_shadow[offset..offset + len]);
        Ok(())
    }

    fn get_handle(&self) -> usize {
        // Expose the underlying CUDA device pointer so it can be
        // registered with RDMA as a GPU buffer (GPUDirect).
        self.data as usize
    }
}
