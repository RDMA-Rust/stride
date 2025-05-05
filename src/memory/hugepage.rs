use anyhow::{anyhow, Result};
use memmap2::{MmapMut, MmapOptions};

use crate::memory::MemoryOps;

/// Memory-mapped hugepage implementation for better performance
pub struct HugepageMemory {
    /// The memory-mapped region
    map: MmapMut,
    /// Size of the allocated memory
    size: usize,
    /// Whether we successfully allocated hugepages
    using_hugepages: bool,
    /// Hugepage size shift that was requested
    page_size_shift: Option<u8>,
}

// The typical huge page size is 2MB
const DEFAULT_HUGEPAGE_SHIFT: u8 = 21; // 2^21 = 2MB

impl HugepageMemory {
    /// Create a new hugepage memory buffer
    ///
    /// # Arguments
    /// * `size` - Size of the buffer to allocate
    /// * `page_size_shift` - Optional page size shift (e.g., 21 for 2MB, 30 for 1GB)
    pub fn new(size: usize, page_size_shift: Option<u8>) -> Result<Self> {
        let shift = page_size_shift.unwrap_or(DEFAULT_HUGEPAGE_SHIFT);

        // Try to allocate with hugepages using the specified size
        match MmapOptions::new().huge(Some(shift)).len(size).map_anon() {
            Ok(map) => {
                let page_size_mb = 1 << (shift - 20); // Convert shift to MB
                tracing::info!(
                    "Successfully allocated memory with {}MB hugepages, size: {}",
                    page_size_mb,
                    size
                );
                Ok(Self {
                    map,
                    size,
                    using_hugepages: true,
                    page_size_shift: Some(shift),
                })
            }
            Err(e) => {
                // If the specified hugepage size failed, try the default size
                if page_size_shift.is_some() && page_size_shift != Some(DEFAULT_HUGEPAGE_SHIFT) {
                    tracing::warn!(
                        "Failed to allocate with custom hugepage size (shift={}): {},
                                  trying default 2MB pages",
                        shift,
                        e
                    );
                    Self::fallback_to_default_hugepage(size)
                } else {
                    // If default hugepages failed or it was already the default, fall back to regular memory
                    tracing::warn!(
                        "Failed to allocate hugepages: {}, falling back to regular memory",
                        e
                    );
                    Self::fallback_to_regular(size)
                }
            }
        }
    }

    /// Try to allocate with default hugepage size after custom size failed
    fn fallback_to_default_hugepage(size: usize) -> Result<Self> {
        match MmapOptions::new()
            .huge(Some(DEFAULT_HUGEPAGE_SHIFT))
            .len(size)
            .map_anon()
        {
            Ok(map) => {
                tracing::info!(
                    "Successfully allocated memory with default 2MB hugepages, size: {}",
                    size
                );
                Ok(Self {
                    map,
                    size,
                    using_hugepages: true,
                    page_size_shift: Some(DEFAULT_HUGEPAGE_SHIFT),
                })
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to allocate default hugepages: {}, falling back to regular memory",
                    e
                );
                Self::fallback_to_regular(size)
            }
        }
    }

    /// Fallback to regular memory if hugepages are not available
    fn fallback_to_regular(size: usize) -> Result<Self> {
        match MmapOptions::new().len(size).map_anon() {
            Ok(map) => {
                tracing::info!("Allocated regular memory, size: {}", size);
                Ok(Self {
                    map,
                    size,
                    using_hugepages: false,
                    page_size_shift: None,
                })
            }
            Err(e) => Err(anyhow!("Failed to allocate memory: {}", e)),
        }
    }

    /// Check if this allocation is using hugepages
    pub fn is_using_hugepages(&self) -> bool {
        self.using_hugepages
    }

    /// Get the page size shift that was applied (if any)
    pub fn page_size_shift(&self) -> Option<u8> {
        self.page_size_shift
    }

    /// Get the page size in MB (if hugepages are being used)
    pub fn page_size_mb(&self) -> Option<usize> {
        self.page_size_shift.map(|shift| 1 << (shift - 20))
    }
}

impl MemoryOps for HugepageMemory {
    fn size(&self) -> usize {
        self.size
    }

    fn write(&mut self, offset: usize, data: &[u8]) -> Result<()> {
        if offset + data.len() > self.size {
            return Err(anyhow!("Write operation would exceed buffer size"));
        }

        self.map[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn read(&self, offset: usize, data: &mut [u8]) -> Result<()> {
        if offset + data.len() > self.size {
            return Err(anyhow!("Read operation would exceed buffer size"));
        }

        data.copy_from_slice(&self.map[offset..offset + data.len()]);
        Ok(())
    }

    fn get_handle(&self) -> usize {
        self.map.as_ptr() as usize
    }
}

// Safe to send between threads
unsafe impl Send for HugepageMemory {}
// Safe to share between threads
unsafe impl Sync for HugepageMemory {}
