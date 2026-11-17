# AetherLang 工程交接

## 本地开发版本

源码在 `/Volumes/WINDOWS/work/AetherLang` 的 `work/aetherlang-upgrade`。最初起点 `aceff15`，本次续做起点 `4cb768c`，macOS/Linux 双宿主冻结为 `423e664`，16 比特异构运行时为 `10355a4`，本次基础代码验收提交为 `d31ea10`。用 `git log -1 --oneline` 查看工作树当前提交。完整任务与历史复现见 [主线任务](MAINLINE_TASKS.md)，历次版本见 [升级报告](docs/UPGRADE.md)。

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

- 双宿主现有安装包固定 `423e664`。Shell 的联验提交为 `44b3da7`，Mac/Linux 各自通过 14 个测试程序及 1,000 次同会话短管道。证据在 `$WORK/shell-joint-20261118/group-acceptance.json`。
- 主编译器 `9edda38`、`c379133` 共享已检查签名、类型及局部/全局存储身份，`c13f93f` 新增 x86_64 SysV 聚合 ABI。大型 JXZ 的生成 C 峰值 RSS 从 300,302,336 B 降为 84,508,672 B，输出逐字节相同。数据在 `$WORK/typed-ir-performance-20261118`。
- `b243b72` 对 Vec、fd、Tensor、量子 lease 实施类型化析构。原生六组与消毒器均通过，独立复核了实参中途失败和返回所有者。`d31ea10` 保留 JXZ 的目录错误，C/LLVM 各 40 项通过。
- `6b324f1`、`a90bd56` 是本机 native JIT 和已验证的磁盘缓存；`f0be8b8` 提供矩阵链规划及低秩分解。M4、RTX 3060、Fold6 Adreno 750 均有真实算子运行。16q 的数值、释放与 CUDA-Q SDK 记录在 `$OUT/q16`，最新上层应用在 `$WORK/compute-gpus-20261118`。
- `66ccafc`、`ad869dd` 支持两种源表示的常量泛型与重复初值的单次求值，含 wasm32 执行。`57ed11c` 的 Core 可直接生成 C 和 LLVM，已编译本仓库 accumulator 与标准库消费者。四代固定点及独立用例在 `$WORK/t26-production/{c-final,llvm-final}`。

## 下一项具体任务

T25 正在补捕获闭包、动态接口和有真实挂起/取消行为的 async。T26 正在将固定数组、类型别名、循环、泛型及枚举传播接入可自编译的 Aether 前端，以现有 JXZ 项目为下一个真实消费者。T01 随能力更新冻结支持集；T20/T29 对合并后的固定提交进行独立复核、串行集成和打包。构建时请和同目录的其他工作协调全量 Cargo。
