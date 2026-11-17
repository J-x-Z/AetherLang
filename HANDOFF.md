# AetherLang 工程交接

## 本地开发版本

源码在 `/Volumes/WINDOWS/work/AetherLang` 的 `work/aetherlang-upgrade`。最初起点 `aceff15`，本次续做起点 `4cb768c`，本轮最近已验证的工具链提交为 `50dbde6`。用 `git log -1 --oneline` 查看工作树当前提交。完整任务与历史复现见 [主线任务](MAINLINE_TASKS.md)，历次版本见 [升级报告](docs/UPGRADE.md)。

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
- T06 已有真实 fd 复用的错关复现和 opt-in owner 第一段的双后端原生执行。T21 的 CPU/Metal 运行时及 T27 的获取、锁定、缓存正在实施。具体完成状态与长线验收保存在主线文档。

## 下一项具体任务

1. 完成 T06 第一段的负例及作用域清理。旧式浅复制复现在 `$WORK/t06-boundary`，定向执行在 `$WORK/t06-owned`。
2. 为同一冻结源码串行运行默认套件和两个 release 种子的四代自举，安装包与基准使用 `scripts/package.py`、`scripts/benchmark.py` 和 `scripts/measure_service.py`。保存比对双方的哈希。
3. T18 的 CachyOS 构建账户和旧 LLVM/cache 已实机核实，结果在 `$WORK/linux-mainline-20261118/live-preflight.json`。使用独立源码/产物目录，保留 Shell 已固定的旧包。
4. 按 T01–T29 的原验收标准继续补齐后段语言和计算工作；当前设备和后端分别记录。阶段包可以独立使用，主线按原清单继续。
