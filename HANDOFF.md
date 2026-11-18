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

最近的语言提交是 `18baf08`。用 `git log -1 --oneline` 查包括打包、文档和后续移植在内的当前提交。保持本地分支、私有仓库，按用户指定在 Windows 上机前停下。

- `67d05e0` 的 packed/union/指针重定位由 Mac 与 Linux 各自的 120 项命令验证。原生编译器已用两后端构建独立 Shell，各通过 48 项。证据在 `$WORK/{t26-native-layout,t26-shell-self}`。
- `10d1b4c` 的所有者与作用域析构通过 151 项命令。`042ba7a` 的原生 LSP 有 25 个协议场景。生成的编译器再编译 JXZ，生成的 JXZ 以两个后端各通过原有集成 40 项和注册表 14 项。见 `$WORK/{t26-owned,t26-jxz-closed}`。
- `5f95130` 新增自举常量泛型，`43279aa` 支持捕获闭包。`eedc7e1` 提供常规二进制入口、转导出与效果检查。该版本自举编译真实 f64 张量和反向微分、CPU/Metal 应用与规划程序。见 `$WORK/t26-math-native/{math,apps,planning}`。
- `18baf08` 支持元组值与投影。自举固定点 105 项、元组 138 项、所有权 151 项通过。见 `$WORK/t26-tuples/{final,final-tests-fixed,final-owned}`。
- `5f01665` 的流式 CSV 和配对统计已由 Rust 和自举编译器各跑 96 个命令，用 Python 精确分数独立核对样本方差、协方差和最小二乘系数。见 `$WORK/terminal-analysis`。
- 常规入口的连续两代生成了相同的 C 和 LLVM。构建命令为 `python3 scripts/build_selfhost.py --seed "$CARGO_TARGET_DIR/release/aethc" --work-dir "$WORK/regular-work" --output "$WORK/aethc-native"`。独立复现用的编译器、原始计时和内存数据在 `$WORK/t26-default-release`。

下一项是合并并验证原生编译器的剩余共享语义、生成两宿主正式包，完成 Windows 交叉构建及原生验收脚本。正式 T26/T29 证据见主线任务文档，上机验收由用户安排在以后。
