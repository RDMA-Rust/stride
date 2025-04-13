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

# Example test results

## Stride probe

```
# ./target/release/stride-probe
rocep65s0: ConnectX-6 (PhysicalFunction, FW: 20.36.1010)
    Port 1: HighDataRate (50 Gbps), Width4X, total data rate: 200 Gbps
```

## Perftest Write Bandwidth

```
# ib_write_bw -n 100000 -q 2 --report_gbits
# ib_write_bw -n 100000 -q 2 --report_gbits 127.0.0.1

---------------------------------------------------------------------------------------
                    RDMA_Write BW Test
 Dual-port       : OFF          Device         : rocep65s0
 Number of qps   : 2            Transport type : IB
 Connection type : RC           Using SRQ      : OFF
 PCIe relax order: ON
 ibv_wr* API     : ON
 CQ Moderation   : 1
 Mtu             : 4096[B]
 Link type       : Ethernet
 GID index       : 3
 Max inline data : 0[B]
 rdma_cm QPs     : OFF
 Data ex. method : Ethernet
---------------------------------------------------------------------------------------
 local address: LID 0000 QPN 0x0044 PSN 0xac3ddb RKey 0x1827e4 VAddr 0x007cebcad9d000
 GID: 00:00:00:00:00:00:00:00:00:00:255:255:192:168:01:01
 local address: LID 0000 QPN 0x0046 PSN 0xaf560 RKey 0x1827e4 VAddr 0x007cebcadad000
 GID: 00:00:00:00:00:00:00:00:00:00:255:255:192:168:01:01
 remote address: LID 0000 QPN 0x0043 PSN 0xd3492a RKey 0x182200 VAddr 0x007d6a6350e000
 GID: 00:00:00:00:00:00:00:00:00:00:255:255:192:168:01:01
 remote address: LID 0000 QPN 0x0045 PSN 0x314ccc RKey 0x182200 VAddr 0x007d6a6351e000
 GID: 00:00:00:00:00:00:00:00:00:00:255:255:192:168:01:01
---------------------------------------------------------------------------------------
 #bytes     #iterations    BW peak[Gb/sec]    BW average[Gb/sec]   MsgRate[Mpps]
 65536      200000           0.00               163.64             0.312124
---------------------------------------------------------------------------------------
```

## Stride Write Bandwidth

```
# ./target/release/stride-perf write bw -s 65535 -n 100000 -q 2
# ./target/release/stride-perf write bw -s 65535 -n 100000 -a 127.0.0.1 -q 2

------------------------------  RDMA Write Bandwidth Test  -------------------------------
Device         : rocep65s0
Transport      : InfiniBand
QP Count       : 2
Connection Type: RC
MTU            : 4096
GID Type       : RoceV1
Rx Depth       : 512
Tx Depth       : 128

----------------------------------  Connection Details  ----------------------------------
QP #00000000: (Local QPN: 0x0041 PSN: 0x5e5cfc) -> (Remote QPN: 0x003f PSN: 0x824602)
QP #00000001: (Local QPN: 0x0042 PSN: 0x115140) -> (Remote QPN: 0x0040 PSN: 0xde2b89)
Local  GID: GID: fe80:0000:0000:0000:966d:aeff:fe61:9eea
Remote GID: GID: fe80:0000:0000:0000:966d:aeff:fe61:9eea

----------------------------------  Bandwidth Results  -----------------------------------
 Size (B)     | Iterations   | Avg BW (Gb/s)      | MsgRate (Mpps)     | Time
--------------+--------------+--------------------+--------------------+------------------
 65535        | 200000       | 180.0395           | 0.3434             | 0.58
------------------------------------------------------------------------------------------
```

## Perftest Write Latency

```
# ib_write_lat -s 65536 -n 100000
# ib_write_lat -s 65536 -n 100000 127.0.0.1

---------------------------------------------------------------------------------------
                    RDMA_Write Latency Test
 Dual-port       : OFF          Device         : rocep65s0
 Number of qps   : 1            Transport type : IB
 Connection type : RC           Using SRQ      : OFF
 PCIe relax order: OFF
 ibv_wr* API     : ON
 TX depth        : 1
 Mtu             : 4096[B]
 Link type       : Ethernet
 GID index       : 3
 Max inline data : 220[B]
 rdma_cm QPs     : OFF
 Data ex. method : Ethernet
---------------------------------------------------------------------------------------
 local address: LID 0000 QPN 0x004a PSN 0x30b280 RKey 0x1820e5 VAddr 0x0079e8ee28f000
 GID: 00:00:00:00:00:00:00:00:00:00:255:255:192:168:01:01
 remote address: LID 0000 QPN 0x0049 PSN 0xa4b114 RKey 0x17f7b6 VAddr 0x007a71eac27000
 GID: 00:00:00:00:00:00:00:00:00:00:255:255:192:168:01:01
---------------------------------------------------------------------------------------
 #bytes #iterations    t_min[usec]    t_max[usec]  t_typical[usec]    t_avg[usec]    t_stdev[usec]   99% percentile[usec]   99.9% percentile[usec]
 65536   100000          6.50           18.46        7.33              7.36             0.15            8.26                    9.43
---------------------------------------------------------------------------------------
```

## Stride Write Latency

```
# ./target/release/stride-perf write lat -s 65535 -n 100000
# ./target/release/stride-perf write lat -s 65535 -n 100000 -a 127.0.0.1

--------------------------------------------------------  RDMA Write Latency Test  ---------------------------------------------------------
Device         : rocep65s0
Transport      : InfiniBand
QP Count       : 1
Connection Type: RC
MTU            : 4096
GID Type       : RoceV1
Rx Depth       : 512
Tx Depth       : 1

-----------------------------------------------------------  Connection Details  -----------------------------------------------------------
QP #00000000: (Local QPN: 0x003a PSN: 0xe65ab8) -> (Remote QPN: 0x0039 PSN: 0xc9171f)
Local  GID: GID: fe80:0000:0000:0000:966d:aeff:fe61:9eea
Remote GID: GID: fe80:0000:0000:0000:966d:aeff:fe61:9eea

------------------------------------------------------------  Latency Results  -------------------------------------------------------------
 Size (B)     | Iterations   | Min (us)     | Max (us)       | Avg (us)     | Stdev (us)   | P50 (us)     | P99 (us)     | P999 (us)
--------------+--------------+--------------+----------------+--------------+--------------+--------------+--------------+------------------
 65535        | 100000       | 6.098        | 200.643        | 6.209        | 0.737        | 6.159        | 6.599        | 9.735
--------------------------------------------------------------------------------------------------------------------------------------------
```
