use anyhow::Result;

pub mod aligned;
pub mod hugepage;
pub mod system;

pub trait MemoryOps {
    fn size(&self) -> usize;

    fn write(&mut self, offset: usize, src: &[u8]) -> Result<()>;

    fn read(&self, offset: usize, dst: &mut [u8]) -> Result<()>;

    fn get_handle(&self) -> usize;
}

/// Base configuration for all memory types
#[derive(Debug, Clone, Copy)]
pub struct MemoryConfig {
    pub size: usize,
    pub numa_node: Option<i32>,
}

impl MemoryConfig {
    pub fn new(size: usize) -> Self {
        Self {
            size,
            numa_node: None,
        }
    }

    pub fn with_numa_node(mut self, node: i32) -> Self {
        self.numa_node = Some(node);
        self
    }
}

/// Configuration for system memory
#[derive(Debug, Clone, Copy)]
pub struct SystemConfig {
    pub config: MemoryConfig,
}

impl SystemConfig {
    pub fn new(size: usize) -> Self {
        Self {
            config: MemoryConfig::new(size),
        }
    }

    pub fn with_numa_node(mut self, node: i32) -> Self {
        self.config = self.config.with_numa_node(node);
        self
    }
}

/// Configuration for aligned memory
#[derive(Debug, Clone, Copy)]
pub struct AlignedConfig {
    pub config: MemoryConfig,
    pub alignment: usize,
}

impl AlignedConfig {
    pub fn new(size: usize, alignment: usize) -> Self {
        Self {
            config: MemoryConfig::new(size),
            alignment,
        }
    }

    pub fn with_numa_node(mut self, node: i32) -> Self {
        self.config = self.config.with_numa_node(node);
        self
    }
}

/// Configuration for hugepage memory
#[derive(Debug, Clone, Copy)]
pub struct HugepageConfig {
    pub config: MemoryConfig,
    pub page_size_mb: Option<usize>, // Allow specifying custom hugepage size (default: 2MB)
}

impl HugepageConfig {
    pub fn new(size: usize) -> Self {
        Self {
            config: MemoryConfig::new(size),
            page_size_mb: None, // Use system default (typically 2MB)
        }
    }

    pub fn with_numa_node(mut self, node: i32) -> Self {
        self.config = self.config.with_numa_node(node);
        self
    }

    pub fn with_page_size(mut self, size_mb: usize) -> Self {
        self.page_size_mb = Some(size_mb);
        self
    }
}

/// Memory type enumeration
#[derive(Debug, Clone, Copy)]
pub enum MemoryType {
    System(SystemConfig),
    Aligned(AlignedConfig),
    Hugepages(HugepageConfig),
}

/// Memory allocator factory
pub struct MemoryAllocator;

impl MemoryAllocator {
    /// Allocate memory using the specified memory type configuration
    pub fn allocate(mem_type: MemoryType) -> Result<Box<dyn MemoryOps>> {
        match mem_type {
            MemoryType::System(config) => Ok(Box::new(system::SystemMemory::new(
                config.config.size,
                config.config.numa_node,
            )?) as Box<dyn MemoryOps>),
            MemoryType::Aligned(config) => Ok(Box::new(aligned::AlignedMemory::new(
                config.config.size,
                Some(config.alignment),
                false,
            )?) as Box<dyn MemoryOps>),
            MemoryType::Hugepages(config) => {
                let page_size_shift = config.page_size_mb.map(|size_mb| {
                    // Convert size in MB to shift value (e.g., 2MB = 21, 1GB = 30)
                    (size_mb * 1024 * 1024).trailing_zeros() as u8
                });

                Ok(Box::new(hugepage::HugepageMemory::new(
                    config.config.size,
                    page_size_shift,
                )?) as Box<dyn MemoryOps>)
            }
        }
    }

    /// Convenience function to create system memory
    pub fn system(size: usize) -> Result<Box<dyn MemoryOps>> {
        Self::allocate(MemoryType::System(SystemConfig::new(size)))
    }

    /// Convenience function to create aligned memory
    pub fn aligned(size: usize, alignment: usize) -> Result<Box<dyn MemoryOps>> {
        Self::allocate(MemoryType::Aligned(AlignedConfig::new(size, alignment)))
    }

    /// Convenience function to create hugepage memory
    pub fn hugepages(size: usize) -> Result<Box<dyn MemoryOps>> {
        Self::allocate(MemoryType::Hugepages(HugepageConfig::new(size)))
    }
}
