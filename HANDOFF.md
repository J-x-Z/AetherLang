# AetherLang 工程交接

## 本地开发版本

源码在 `/Volumes/WINDOWS/work/AetherLang` 的 `work/aetherlang-upgrade`。最初起点 `aceff15`，本次续做起点 `4cb768c`，macOS/Linux 双宿主冻结为 `423e664`，16 比特异构运行时为 `10355a4`，const generics 为 `66ccafc`，直接 LLVM 自举为 `06207c6`。用 `git log -1 --oneline` 查看工作树当前提交。完整任务与历史复现见 [主线任务](MAINLINE_TASKS.md)，历次版本见 [升级报告](docs/UPGRADE.md)。

## 构建与验收

```sh
cd /Volumes/WINDOWS/work/AetherLang
. /Volumes/WINDOWS/work/aetherlang-upgrade-work/typed-facts-20261117/env.sh
sh scripts/build.sh llvm
OPENBLAS_LIBRARY=/opt/homebrew/opt/openblas/lib/libopenblas.dylib \
AETHER_TEST_QEMU=1 AETHER_TEST_METAL=1 AETHER_CUDAQ_BRIDGE="$WORK/cudaq/libaether_cudaq.dylib" sh scripts/test.sh llvm
```

本机使用外置 Rust 1.100.0、LLVM 21 和三路 Cargo 并发。其他环境可设置自己的 `CARGO_TARGET_DIR`、`TMPDIR`、`LLVM_SYS_211_PREFIX`；`scripts/build.sh c` 构建独立的 C-only 编译器。旧的 macOS/Linux Shell 固定包分别保存在 `$WORK/release`、`$WORK/releases`，本轮仅做本地提交。

## 已落地

- 已提交数值/方法/投影的跨层修复、递归泛型、向量对齐、窄整数 C ABI、目标布局、Darwin 部署目标、无运行库编译及受支持的中断入口。32 位双后端的 WebAssembly 已实际运行。
- 已提交四代 Core 自举修复、Script 名称卫生、解析深度保护、LSP 跨文件导航、接口一致性、运行时契约、可恢复的 JXZ 产物发布和本地版本安装。
- LLVM/JNI/OpenBLAS/Metal 各有真实宿主库消费者。新 alloc/TOML/graph 有消费者、分配失败和消毒器验证。JXZ 读有长度的二进制流，保留 POSIX 错误。
- 数据按套件存储在 `$WORK/typed-facts-20261117`、`$WORK/r02-r04`、`$WORK/r07`、`$WORK/contracts` 等。完整证据以对应编译器哈希和命令为准。

- `47c4649` 修复 LLVM O2 删除中断帧写入；QEMU x86_64 的软件中断及带错误码的 #GP 在 C/LLVM O0/O2 四组均正确返回。证据在 `$OUT/qemu-final`。
- `2919a19` 增加 Bytes、UTF-8 与拥有字节键的 Map，`58e87d0` 把 Vec 的真实分配 ABI 带到 wasm32。ABI 修复 `3013c5d` 经过 52 个目标布局命令。JXZ `50dbde6` 提供与 build 共用配置的 stdio LSP。
- T06 显式 owner 和析构已本地提交，证据 `WORK/t06-owned`。JXZ 已可获取、锁定、更新和离线复建，多版本冲突与临时文件故障注入在 `WORK/registry`。T21/T22 的 Aether 队列已在 Apple M4、RTX 3060、Adreno 750 执行，参见升级报告与 `OUT/vulkan-build`。

- `423e664` 在 Aether 中实现增量 SHA-256。CachyOS 的 C-only 与 LLVM 包各自完成 40 项 JXZ 项目验收、14 项注册表验收与 116 项自举检查。macOS 和 Linux 包分别见 `WORK/releases/aetherlang-{macos,linux}-7f9e628`。
- `10355a4` 在 M4、RTX 3060、Adreno 750 执行同一 16 比特电路、经典角度更新和全部振幅对照。证据 `OUT/q16`。同一宿主 CUDA-Q QPP 走 SDK 接口。计算算子 `2268784` 的独立数值基准与原生 Metal 证据在 `WORK/compute-applications-20261118/final`。
- `9839f2f` 让 Core 读取自己的模块图。`06207c6` 的上一代 Core 编译完整的直接 LLVM 发射器，连续三代生成相同 LLVM IR，末代还原相同 C；99 项 LLVM 验收与 131 项 C 验收通过，见 `WORK/t26-llvm/{final-entry,c-final}`。完整 Rust 主编译器仍有更广的语言支持。
- `66ccafc` 在 Core/Script 中实现目标字宽感知的整数与 bool 常量泛型、布局、推导及跨模块检查，含两后端原生运行和 wasm32 执行。Shell 关联分支 `work/aetherlang-host-upgrade` 的本地提交 `e1e99bb` 固定双宿主包，两端 Aether Shell 均已运行项目原生、脚本和 PTY 测试，见 `WORK/shell-joint-20261118/joint-acceptance.json`。

## 下一项具体任务

1. T02/T19 在固定源码上验证新的类型降低和大型 JXZ 的编译器峰值内存，核对优化前后的真实输出。T06 将析构贯通 Vec、Tensor、QuantumLease 与失败路径。
2. T23 验收图 JIT 的磁盘缓存、损坏恢复及收缩规划。T25 在常量泛型基础上推进重复表达式、捕获、动态分派、挂起与恢复。
3. T26 以现有 accumulator 和 JXZ 的源码为目标迁移自举前端及驱动，对照 Rust 种子的能力。T28 记录同一 Shell 进程多批工作后的资源。T20/T29 对最终固定源码独立复核、串行集成和打包。
