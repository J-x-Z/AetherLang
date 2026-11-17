# AetherLang 工程交接

## 本地开发版本

源码在 `/Volumes/WINDOWS/work/AetherLang` 的 `work/aetherlang-upgrade`。最初起点 `aceff15`，本次续做起点 `4cb768c`，本轮最近已验证的标准库提交为 `0d113b3`。用 `git log -1 --oneline` 查看工作树当前提交。完整任务与历史复现见 [主线任务](MAINLINE_TASKS.md)，历次版本见 [升级报告](docs/UPGRADE.md)。

## 构建与验收

```sh
cd /Volumes/WINDOWS/work/AetherLang
. /Volumes/WINDOWS/work/aetherlang-upgrade-work/typed-facts-20261117/env.sh
sh scripts/build.sh llvm
OPENBLAS_LIBRARY=/opt/homebrew/opt/openblas/lib/libopenblas.dylib \
AETHER_TEST_METAL=1 AETHER_CUDAQ_BRIDGE="$WORK/cudaq/libaether_cudaq.dylib" sh scripts/test.sh llvm
```

本机使用外置 Rust 1.100.0、LLVM 21 和三路 Cargo 并发。其他环境可设置自己的 `CARGO_TARGET_DIR`、`TMPDIR`、`LLVM_SYS_211_PREFIX`；`scripts/build.sh c` 构建独立的 C-only 编译器。旧的 macOS/Linux Shell 固定包分别保存在 `$WORK/release`、`$WORK/releases`，本轮仅做本地提交。

## 已落地

- 已提交数值/方法/投影的跨层修复、递归泛型、向量对齐、窄整数 C ABI、目标布局、Darwin 部署目标、无运行库编译及受支持的中断入口。32 位双后端的 WebAssembly 已实际运行。
- 已提交四代 Core 自举修复、Script 名称卫生、解析深度保护、LSP 跨文件导航、接口一致性、运行时契约、可恢复的 JXZ 产物发布和本地版本安装。
- LLVM/JNI/OpenBLAS/Metal 各有真实宿主库消费者。新 alloc/TOML/graph 有消费者、分配失败和消毒器验证。JXZ 读有长度的二进制流，保留 POSIX 错误。
- 数据按套件存储在 `$WORK/typed-facts-20261117`、`$WORK/r02-r04`、`$WORK/r07`、`$WORK/contracts` 等。完整证据以对应编译器哈希和命令为准。

## 下一项具体任务

收齐 tuple 跨层修复、精确包入口与 JXZ 本地路径依赖。用最终源码串行执行统一验收，再用 release 编译器构建 JXZ/自举，测量基准并生成带校验清单的本机包。单项验收状态及后段扩展仍逐项保留于主线任务表。
