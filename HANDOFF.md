# AetherLang 工程交接

## 本地开发版本

源码在 `/Volumes/WINDOWS/work/AetherLang` 的 `work/aetherlang-upgrade`。最初起点 `aceff15`，本次续做起点 `4cb768c`，可安装的 macOS 基础冻结为 `c5ed0b6`，异构运行时快照为 `4d36291`。用 `git log -1 --oneline` 查看工作树当前提交。完整任务与历史复现见 [主线任务](MAINLINE_TASKS.md)，历次版本见 [升级报告](docs/UPGRADE.md)。

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

## 下一项具体任务

1. T18 继续检查 CachyOS 冻结快照 `4d36291` 的两套 release、自举和包外 JXZ。原生 Clang 23 发现的 strtod、mkdtemp ABI 已分别在 `0bdfe56`、`c8456f5` 修复，120 项宿主 ABI 命令通过。日志在 `WORK/linux-mainline-20261118`。
2. T24 使用同一 compute 队列和预算在 CPU、Metal、Vulkan 执行 16q，检查振幅、梯度、错误释放。T23 复用队列实现科学算子与工作负载。
3. T25 实现 const generics。T26 将 Core 自身的模块读取、解析和名称查找放入自举构建链。各自用正反例及真实执行复验。
4. 基础冻结安装包在 `WORK/releases/aetherlang-foundations-20261118`，七次采样比较在 `WORK/final-review-20261118/comparison.json`。最终按主线保留独立代码复核、Shell 各宿主联验和固定包。
