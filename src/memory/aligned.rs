use anyhow::{anyhow, Result};
use std::alloc::{alloc, dealloc, Layout};
use std::ptr::NonNull;
use std::slice;

use crate::memory::MemoryOps;

/// Default cache line size (64 bytes on most architectures)
pub const DEFAULT_CACHE_LINE_SIZE: usize = 64;

/// Huge page size (2MB)
pub const HUGE_PAGE_SIZE: usize = 2 * 1024 * 1024;

/// AlignedMemory provides memory allocation aligned to cache line boundaries
///
/// This implementation mimics perftest's host_memory.c to provide better performance
/// through proper memory alignment and optional huge pages support.
pub struct AlignedMemory {
    /// Raw pointer to the allocated memory
    ptr: NonNull<u8>,
    /// Size of the allocated memory
    size: usize,
    /// Alignment used for this allocation
    alignment: usize,
    /// Original layout used for deallocation
    layout: Layout,
    /// Whether huge pages are being used
    _use_huge_pages: bool,
}

impl AlignedMemory {
    /// Create a new aligned memory buffer
    ///
    /// # Arguments
    /// * `size` - Size of the buffer to allocate
    /// * `alignment` - Memory alignment (defaults to cache line size if None)
    /// * `use_huge_pages` - Whether to try using huge pages (not implemented yet)
    pub fn new(size: usize, alignment: Option<usize>, use_huge_pages: bool) -> Result<Self> {
        let alignment = alignment.unwrap_or(DEFAULT_CACHE_LINE_SIZE);

        // Ensure alignment is a power of two
        if !alignment.is_power_of_two() {
            return Err(anyhow!("Alignment must be a power of two"));
        }

        // If using huge pages, round the size up to huge page boundary
        let size = if use_huge_pages {
            size.div_ceil(HUGE_PAGE_SIZE) * HUGE_PAGE_SIZE
        } else {
            size
        };

        // Create a layout with the requested alignment
        let layout = Layout::from_size_align(size, alignment)
            .map_err(|e| anyhow!("Failed to create memory layout: {}", e))?;

        // Allocate the memory
        let ptr = unsafe {
            let ptr = alloc(layout);
            if ptr.is_null() {
                return Err(anyhow!("Memory allocation failed"));
            }
            NonNull::new_unchecked(ptr)
        };

        // Initialize the memory to zero
        unsafe {
            ptr.as_ptr().write_bytes(0, size);
        }

        Ok(Self {
            ptr,
            size,
            alignment,
            layout,
            _use_huge_pages: use_huge_pages,
        })
    }

    /// Get a reference to the allocated memory as a slice
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.size) }
    }

    /// Get a mutable reference to the allocated memory as a slice
    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.size) }
    }
}

impl MemoryOps for AlignedMemory {
    fn size(&self) -> usize {
        self.size
    }

    fn write(&mut self, offset: usize, data: &[u8]) -> Result<()> {
        if offset + data.len() > self.size {
            return Err(anyhow!("Write operation would exceed buffer size"));
        }

        let slice = self.as_slice_mut();
        slice[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn read(&self, offset: usize, data: &mut [u8]) -> Result<()> {
        if offset + data.len() > self.size {
            return Err(anyhow!("Read operation would exceed buffer size"));
        }

        let slice = self.as_slice();
        data.copy_from_slice(&slice[offset..offset + data.len()]);
        Ok(())
    }

    fn get_handle(&self) -> usize {
        self.ptr.as_ptr() as usize
    }
}

impl Drop for AlignedMemory {
    fn drop(&mut self) {
        unsafe {
            dealloc(self.ptr.as_ptr(), self.layout);
        }
    }
}

// Safe to send across threads
unsafe impl Send for AlignedMemory {}
// Safe to share across threads
unsafe impl Sync for AlignedMemory {}
