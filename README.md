# Stride

A comprehensive RDMA (Remote Direct Memory Access) benchmarking tool with automatic vendor-specific optimizations.

## Features (WIP)

- **Automatic Hardware Detection**
  - Identifies RDMA vendors (NVIDIA/Mellanox, Intel, etc.)
  - Auto-detects device capabilities
  - Optimizes benchmark parameters per device
  - Supports both physical and virtual functions

- **Comprehensive Benchmarks**
  - RDMA operations: Send, Write, Read
  - Performance metrics:
    - Bandwidth (GB/s)
    - Latency (μs)
    - IOPS
  - Configurable message sizes and QP attributes

- **Flexible Output Formats**
  - Human-readable console output
  - JSON/YAML export via serde
  - Custom formatting options
  - Historical result comparison

- **Benchmark Suite Management**
  - Save benchmark configurations
  - Load and modify existing suites
  - Compare results across runs
  - Export/import benchmark definitions
