# AetherLang 工程交接

## 本地开发版本

源码在 `/Volumes/WINDOWS/work/AetherLang` 的 `work/aetherlang-upgrade`。最初起点 `aceff15`，本次续做起点 `4cb768c`，macOS/Linux 双宿主冻结为 `423e664`，16 比特异构运行时为 `10355a4`，基础整体验收源码为 `f84e9d4`，自举共享语义与 JXZ 项目链至 `5951201`。用 `git log -1 --oneline` 查看工作树当前提交。完整任务与历史复现见 [主线任务](MAINLINE_TASKS.md)，历次版本见 [升级报告](docs/UPGRADE.md)。

## 构建与验收

```sh
cd /Volumes/WINDOWS/work/AetherLang
. /Volumes/WINDOWS/work/aetherlang-upgrade-work/typed-facts-20261117/env.sh
sh scripts/build.sh llvm
OPENBLAS_LIBRARY=/opt/homebrew/opt/openblas/lib/libopenblas.dylib \
AETHER_TEST_QEMU=1 AETHER_TEST_METAL=1 AETHER_CUDAQ_BRIDGE="$WORK/cudaq/libaether_cudaq.dylib" sh scripts/test.sh llvm
```

本机使用外置 Rust 1.100.0、LLVM 21 和三路 Cargo 并发。其他环境可设置自己的 `CARGO_TARGET_DIR`、`TMPDIR`、`LLVM_SYS_211_PREFIX`；`scripts/build.sh c` 构建独立的 C-only 编译器。旧的 macOS/Linux Shell 固定包分别保存在 `$WORK/release`、`$WORK/releases`，本轮仅做本地提交。

## 本地可复现进展

- 双宿主基础安装包在 `$WORK/releases/aetherlang-{macos,linux}-c36bd04`。Shell 专用的对照包固定 `423e664`。Shell 的联验提交为 `44b3da7`，Mac/Linux 各自通过 14 个测试程序及 1,000 次同会话短管道。证据在 `$WORK/shell-joint-20261118/group-acceptance.json`。
- 主编译器 `9edda38`、`c379133` 共享已检查签名、类型及局部/全局存储身份，`c13f93f` 新增 x86_64 SysV 聚合 ABI。大型 JXZ 的生成 C 峰值 RSS 从 300,302,336 B 降为 84,508,672 B，输出逐字节相同。数据在 `$WORK/typed-ir-performance-20261118`。
- `b243b72` 对 Vec、fd、Tensor、量子 lease 实施类型化析构。原生六组与消毒器均通过，独立复核了实参中途失败和返回所有者。`d31ea10` 保留 JXZ 的目录错误，C/LLVM 各 40 项通过。
- `6b324f1`、`a90bd56` 是本机 native JIT 和已验证的磁盘缓存；`f0be8b8` 提供矩阵链规划及低秩分解。M4、RTX 3060、Fold6 Adreno 750 均有真实算子运行。16q 的数值、释放与 CUDA-Q SDK 记录在 `$OUT/q16`，最新上层应用在 `$WORK/compute-gpus-20261118`。
- `66ccafc`、`ad869dd` 支持两种源表示的常量泛型与重复初值的单次求值，含 wasm32 执行。`57ed11c` 的 Core 可直接生成 C 和 LLVM，已编译本仓库 accumulator 与标准库消费者。四代固定点及独立用例在 `$WORK/t26-production/{c-final,llvm-final}`。

- `451e2f8` 让两种表示的捕获闭包经过同一静态专门化；`f84e9d4` 统一二元运算及复合赋值，修复 LLVM 指针步长。闭包验收在 `$WORK/t25-closures/final7`；复合赋值有 121 项定向 CLI 检查。
- 自举编译器能直接构建二进制和对象、使用 JXZ 精确模块映射、读 native errno。相邻代在 Mac/Linux 生成相同的 LLVM IR（`fa4016c`）。`ee987fb` 与 `5316467` 在两主机各通过 20 项 errno 与 47 项原生驱动验收，含两次连续中断后的子进程回收。证据在 `$WORK/{t26-native-driver,t26-errno,t26-wait}`。
- `f84e9d4` 的完整 macOS 验收退出 0，日志 `$WORK/foundation-c36bd04/full.log`。CachyOS 的同提交打包、搬移和项目验收见 `$WORK/linux-mainline-20261118/c36bd04.log`；同版本再通过 27 项 JIT 与 123 项宿主内建命令。

## 当前接续点

本地分支包含 `cd1a8ef`（两种表示的 async、取消析构及实参求值顺序）、`ad846b3`（浮点常量逐操作舍入）、`477d1cc`（类型化 volatile 地址）、`3eb8e0a`（Rust 种子可变形参）和 `9e17d54`（C 中的常量控制流）。`7b2a4ae` 的量子子集读出从两种 Bell 态验证单比特不可区分与双比特可区分，`7cb41c8` 修复这一程序实际触发的泛型返回布局错误。Windows 打包入口为 `88935aa`。用 `git log -1 --oneline` 查看最后提交。

- 上次完整 macOS/Linux 包来自 `15aedda`，原生编译器能自建并构建 JXZ。独立 Shell 的自举双后端验收在 `$WORK/t26-shell-self`，流式统计独立有理数核验在 `$WORK/terminal-analysis`。
- 中间验证在 `$WORK/t26-{async,volatile,scalar-const,c-return,seed-mut-params,readout}`。移动参数、挂起恢复和早退析构均有原生回归；`t26-readout/native-fixed.json` 涵盖 CPU 与 M4 Metal。
- 常规自举入口为 `python3 scripts/build_selfhost.py --seed "$CARGO_TARGET_DIR/release/aethc" --work-dir "$WORK/regular-work" --output "$WORK/aethc-native"`。安装包使用 `scripts/package.py`，Windows x86-64 交叉包使用 `scripts/package_windows.py`。

待合入显式优化 IR 和 volatile 引用转换校验后冻结源码。冻结版本验收按 `$WORK/final-mac-20261118.py`、`$WORK/linux-mainline-20261118/final-native-delivery.py` 和 Windows 交叉包三个入口执行，核验后在此记录版本和结果。用户指定本阶段结束于 Windows 上机前，保留私有仓库和本地提交。
