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

## 下一项具体任务

主线已验收 27/29 项。T25 的四种能力联合测试至 `d7bac32`。T26 的上一代种子、固定点和项目运行分别见 `$WORK/t26-{abi,language-integrated,project-lsp}`。`8cfd1a1` 用独立 C 调用端和被调用端验证了 Apple arm64 与 x86_64 SysV 的原生聚合 ABI。`b55a11e` 可对两种源表示检查引用、实例化枚举及方法、传播错误、生成 C/LLVM，并从仓库 JXZ 源码构建 JXZ。该 JXZ 在两后端完成含三包依赖、失效重建、Unicode 参数、失败状态和清理的 42 项命令。相邻代 LLVM IR SHA-256 为 `36c05defcc8adcdda3f972169e2bcd3cc2b7f585e76dceb4b080421364cd188d`。`497bd70` 补上文件模块、内联模块及缺失对象路径，`5951201` 修复所有 match 分支均退出时对函数返回的误判。

接下来在同一自举源码上联合验证紧凑布局、所有者析构、常量泛型和 JXZ 调用的语言服务。用生成的 JXZ 和自举编译器一起跑 jxz/tests/integration.py；T29 待 T26 的共享程序、完整固定点与双宿主归档验收后更新。

## 11 月 18 日新增验收范围

`67d05e0` 的 packed/union/指针重定位由 macOS 和 Linux 各自的 120 项命令验证。Mac 的 Rust 种子另以两个后端链接和运行同一含非对齐指针的可执行文件。自举构建的 aether-shell 两个原生程序各通过 48 项既有验收。资源语义提交 `10d1b4c` 有 151 项命令、30 次资源程序运行，证据在 `$WORK/{t26-native-layout,t26-shell-self,t26-owned/integrated}`。

阶段验收补上终端科学数据工作流、Windows 上机前移植与 README。两台登记的 Windows 机器当前离线，用户要求本阶段不推送、保持私有，稍后再做 Windows 原生验收。`$WORK/t26-math-native` 是隔离的公共导出/常规 CLI/效果检查实现和已运行的 AD、CPU/Metal 工作负载。集成后复测固定点与双宿主包。