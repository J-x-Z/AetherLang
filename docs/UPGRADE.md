# AetherLang 集中升级交付

## LLVM 21 与 23 的迁移决定

稳定种子、CI 默认和现有发行包继续使用 LLVM 21.1。需要更快的本机编译时可用 `sh scripts/build.sh llvm23` 显式选择 23.1。C-only 种子仍可独立构建。同一工具链仅链接一个 LLVM major；两个版本使用同一套 Aether 代码生成逻辑。

在 Apple M4 上使用同一 Rust 1.100.0、同一份 Rust 编译器源码、release 配置和 macOS arm64 目标比较 LLVM 21.1.8 与 23.1.2。实验包含对应的 llvm-sys 绑定。两边都用同一 Apple Clang、SDK 和链接命令，每项预热两次后交替测八对，表中为中位数。

| 操作 | LLVM 21.1.8 | LLVM 23.1.2 |
| --- | ---: | ---: |
| JXZ 生成 O2 对象，耗时 / 峰值 RSS | 2,363.62 ms / 122.92 MiB | 1,013.42 ms / 137.52 MiB |
| 回归分析程序生成 O2 对象，耗时 / 峰值 RSS | 97.00 ms / 43.28 MiB | 78.33 ms / 55.91 MiB |
| 程序读入 25 万行 CSV 并回归，耗时 / 峰值 RSS | 50.59 ms / 1.66 MiB | 50.40 ms / 1.66 MiB |
| JXZ 对象 / 可执行文件 | 104,568 / 106,000 B | 103,680 / 106,000 B |
| 回归程序对象 / 可执行文件 | 8,816 / 34,104 B | 8,752 / 34,104 B |

最终两个 release 种子重新编译这两项负载，四个对象分别与测量时的对象逐字节相同。两个程序的编译在八对测量中均提速。CSV 运行时间落在样本波动范围内，十个输出字段符合数学预期，所有输出 JSON 逐字节相同。LLVM 23 输出的新浮点 IR（例如 `f0x7FEFFFFFFFFFFFFF`）会被本机 Apple Clang 21.0 拒绝。相同量子测试用稳定种子加系统 Clang，或 23 种子加配套 Clang，各通过 31 项。保存的对照实验与原始失败分别在外置工作目录 `llvm23-kernel-quantum-20261118/compare-host/{results.json,run.py,provenance.md}` 和 `suite23-seed.log`。

试用 Clang 23 还暴露了宿主 C 后端的实际错误。Aether 允许通过字节指针传递不透明文件句柄，生成的 C 直接把该指针当作系统 `FILE *` 使用。`a98f550` 按系统函数的句柄参数和返回值位置转换，保留变参格式检查。相同源文件通过 C/LLVM、O0/O2 共 20 项，错误句柄类型仍有诊断，裸机同名声明可单独生成。Clang 对部分字符串的 char 符号差异仍会给出警告。

LLVM 22 的 SLP 向量化器能识别新的浮点乘加模式。LLVM 23 更改了浮点 IR 文字格式及条件分支 C API。上游发布到 23.1.6，本机实测版本为 23.1.2。23.1.4 起包含一项 x86 带错误码中断处理函数调用时的栈对齐修复；本轮使用的 PIT IRQ 没有硬件错误码。Metal、Vulkan 和 CUDA-Q 的设备程序使用各自的编译链。此次迁移评估衡量的是宿主编译器。[LLVM 22 向量化](https://releases.llvm.org/22.1.0/docs/ReleaseNotes.html#changes-to-vectorizers) · [LLVM 23 IR](https://releases.llvm.org/23.1.0/docs/ReleaseNotes.html#changes-to-the-llvm-ir) · [LLVM 23 C API](https://releases.llvm.org/23.1.0/docs/ReleaseNotes.html#changes-to-the-c-api) · [x86 修正](https://github.com/llvm/llvm-project/commit/bb46d66418b91a6f17312ad3ff8d3713d66b89ae) · [23.1.6 发布](https://github.com/llvm/llvm-project/releases/tag/llvmorg-23.1.6)

macOS 上最终 `sh scripts/test.sh llvm23 seed` 以状态 0 完成，日志为 `WORK/llvm23-kernel-quantum-20261118/opt-in-final.log`。其中包含 239 个 Rust 单元及集成测试，JXZ 在两种后端各跑 40 个集成与 14 个注册表场景，20 项模板检查、35 项 SDK 运行检查和 22 项 CUDA-Q ABI 及错误路径检查。另有带 SDK 解析器的独立导出回执。默认 LLVM 21 release 构建、189 个单元测试和本轮文件句柄回归重新通过。下节自举固定点与 QEMU 验收使用独立入口。Linux 和 Windows 宿主包仍使用各自已验收的版本。

## 自举内核与量子计算新能力

常规自举编译器可以检查带 `#![no_std]` 的 x86-64 裸机程序，发射带目标布局和 `noredzone` 的 LLVM IR，交给目标 Clang 生成 ELF64 可重定位对象。外部启动桩和链接脚本仍由内核提供。复审发现，Clang 包装器返回成功时可能改写优化产物的架构。修复后发射器在发布前核对三元组和完整布局，错误时保留旧文件。修正版连续两代发射的 C 为 3,696,820 字节、LLVM IR 为 5,057,497 字节，各自逐字节相同。LLVM 21 和 23 各通过 24 项，包括 O0/O2 的 QEMU 双向 SysV ABI 调用、结构返回、寄存器耗尽及错误目标输出保留。[交叉编译回归](../tests/bootstrap_kernel_target.py)

两个版本各启动八个页帧池镜像，覆盖种子/自举、C/LLVM、O0/O2。O2 下种子 LLVM 的页帧池代码段两版均为 481 B；自举 LLVM 则从 930 B 变为 943 B。另一个 x86-64 QEMU 客体在 100 Hz 周期时钟下运行普通 Aether 函数；Aether 中断处理函数连续记录八次有进展的样本。宿主检查中断地址属于工作函数，按已完成步数重新计算校验和。该处理函数仍由种子编译。下一步是让自举编译器支持这个机器入口。[页帧池](../tests/kernel_boot.py) · [周期时钟](../tests/kernel_timer.py)

修正版自举编译器在同一轮通过 CPU、Metal 和 qpp 的 20 项模板检查。参数化电路完全由 Aether 构造。同一角度可绑定多个旋转门，绑定结果有独立生命周期，分配失败可重试。三量子位的 CPU、CUDA-Q qpp、Apple M4 Metal 结果均与全部八个解析复振幅比较；M4 的最大单分量误差为 7.49×10⁻⁸，所有提交完成并且内存记账归零。对于具体的十门电路，可调用 NVIDIA SDK 导出 Quake MLIR，并用 SDK 解析和验证寄存器、门序、控制位、目标位及角度。当前模板在导出前绑定数值，未绑定的 Quake 函数参数留待下一步。CUDA-Q SDK 在这台 Mac 上使用 CPU，Metal 加速由 Aether 自有运行时提供。[参数模板](../stdlib/quantum_template.aeth) · [Quake 示例](../examples/quantum/export_quake.aeth) · [设备矩阵](QUANTUM.md)

## Windows 11 原生交付与设备验证（历史版本）

本节记录先前测试环境的结果。该 Windows 设备不属于维护者，本次发行按维护者自己的双启动笔记本另行验收。

Windows 包冻结于 `98402df`，SHA-256 为 `3b946dd8aa457dbdef7944d5a0588a67c0702e45eef32ecaa336b3a9f91b507b`。在 Windows x64 build 26200 解压后核验了 168 个载荷文件，并用 llvm-mingw UCRT Clang 21.1.8 和 Python 3.10.11 从包内 PowerShell 入口执行。编译器套件用 C/LLVM 在 O0/O2 编译运行并发射 COFF，还在 Windows 上从 Aether 源码生成了两份下一代编译器，三份编译器对同一入口发射完全相同的 LLVM IR。25 项编译器检查包含 LSP 的入口，内层运行 24 个协议和 CLI 场景。JXZ 35 项及终端 CSV 10 项通过。该机单次耗时为编译器套件 25.22 秒、JXZ 13.03 秒、CSV 2.86 秒。各平台发行基线及编译速度、RSS、产物和运行性能比较见下节。

真机验收修复了两处影响实际工作的错误。旧 JXZ 可初始化项目，却在写入依赖和发布输出时被自己的 Win32 目录锁拦截。现在 NTFS 独占创建临时锁文件，最后一个继承句柄关闭时系统清除它。C、LLVM 编译的项目都完成发布；杀掉父进程、保留用户同名文件、拒绝损坏记录和空目录清理都有回归。输出记录和事务日志以二进制模式写入约定的 LF。原生 Windows 的 CSV 程序曾把标准输入中的 `0x1A` 误判为流结束。现在只在读取用的副本上启用二进制模式。五个有效样本经文件和重定向得到相同斜率 2.2、截距 4 和样本方差 2.5、13。macOS 的 JXZ 集成 40 项、注册表 14 项及统计 96 项复验通过。

线上协议另在这台 Windows 上用系统 curl 8.22.0 对回环 HTTP 注册表做了 11 项检查。从空缓存获取三包菱形图，14 个请求包含百分号编码的中文和空格。关闭服务器、确认端口已关闭并清空构建产物后，两后端均可用缓存重建，程序输出 `registry=17`，锁文件哈希保持一致。回执和复现脚本在 `WORK/windows-native-20261118/evidence-registry-117efa2.zip`。

另一个 Windows 包交付可复用的 Vulkan DLL 和加载器对象。C/LLVM 生成的 16 比特程序在 RTX 4060 Laptop GPU 上完成 42 次提交，全部 65,536 个振幅最大 L1 误差为 `7.81873961e-8`，队列峰值记账 2,097,216 字节，释放后为零。CUDA-Q SDK 独立探针在 Apple M4 实测报告 CPU，Linux RTX 3060 的 cuStateVec GPU 有先前 SDK 实机证据。Windows 4060 的 GPU 验收使用 Aether 自有 Vulkan 路径。证据在 `WORK/windows-native-20261118/{native-acceptance.json,evidence-117efa2.zip}`、`WORK/windows-gpu-20261118/windows-vulkan-results.json` 和 `WORK/cudaq/device-query-20261118/runtime/results.json`。

## 内核初始基线与第一次后端对照

Rust 种子可用 C 或 LLVM 为 `x86_64-unknown-none` 生成裸机对象。已有启动汇编负责建栈、页表、GDT 和 IDT；Aether 中断入口在 QEMU 中执行并返回过两种 CPU 栈帧。ARM64 有裸机对象与裸入口反汇编证据。该阶段的自举编译器还只有宿主固定点；后续裸机目标进展见上节。[裸机验收](../tests/freestanding.py) · [启动夹具](../tests/regressions/freestanding/interrupt_boot.S)

旧 Rust 种子会在普通 x86-64 裸机函数里使用栈指针以下的 red zone，同栈中断可能覆盖局部变量。`d22a1d9` 为该目标的每个 LLVM 函数设置 `noredzone`，并让 C 编译入口传 `-mno-red-zone`。普通独立翻译单元的两条回归在旧版分别失败，修复后通过。另在 QEMU TCG 启动 C/LLVM、O0/O2 四个原有镜像，均按预期退出。对手工编译发射的 C 源码，外部构建也须传相同参数。

**自举 C 与直接 LLVM 的第一次内核负载对照。** `c7ab3ab` 增加由 Aether 编写的页帧池，在 x86-64 QEMU TCG 客体使用四页对齐 RAM 检验唯一分配、耗尽、非法与重复释放、复用，并实际读写每页首尾。启动桩先传入错误页数检查失败分支，再走正确参数。自举编译器发射 C，Clang 以裸机目标和仅通用寄存器模式生成对象，客体在开启 SSE 前运行。种子编译器的 C 和直接 LLVM 使用相同目标，并在进入 Aether 前启用 SSE2。三路都用相同的链接脚本和内核自带的字节填充函数；种子 C 在 O0 下会引用 `memset`。六个镜像都通过，此外共用启动汇编的原有四个中断镜像也重新启动通过。复现入口是 [kernel_boot.py](../tests/kernel_boot.py)，回执为 `WORK/kernel-pages-20261118/verified-8293b4a.json`。

| 构建路径 | 页帧池目标代码 O0 | 页帧池目标代码 O2 |
| --- | ---: | ---: |
| 自举 Aether → C → Clang，仅通用寄存器 | 6,059 B | 887 B |
| Rust 种子 → C → Clang | 6,946 B | 887 B |
| Rust 种子 → 直接 LLVM | 1,774 B | 481 B |

表中是页帧池对象的代码段字节数，启动、对齐、辅助函数和调试信息另计。这些值不衡量运行速度。同一输入上直接 LLVM 的 O2 代码更小；其 O0 普通函数使用了 XMM 寄存器。自举 C 已经能承载这一类内核主体。该次实验的裸机直接 LLVM 路径由种子提供；常规自举编译器后来在 `7dcd8d8` 加入同一目标。

## 2026-11-18 宿主编译器与 JXZ 交付

在起点 `aceff15` 之后的 `e37a4f2` 里，常规入口 `src_aether/core/aethc.aeth` 已能从 Rust 种子生成、编译下一代并运行实际项目。最终 `13941f6` 在 macOS arm64 和 Linux x86-64 上均由 C-only、LLVM 两种种子各通过 144 项四代验收。常规入口相邻两代生成的 C 在两台宿主上也是同一份，共 3,670,518 字节。直接 LLVM IR 逐代在各自宿主上完全相同，macOS 为 5,021,655 字节，Linux 为 5,021,682 字节。机器码生成与链接继续交给宿主 Clang。Rust 种子保留内联汇编、跨目标代码生成和向量等能力。两种源表示都能使用捕获闭包、常量泛型、借用的动态接口和协作式任务。

多轮独立复审修复了四类能改变用户程序行为的问题。临时泛型 Result 用于 match 或 `?` 曾令 LLVM 编译器崩溃。新测试分别覆盖成功和错误载荷。取消异步子任务与参数中途返回曾破坏 owner 的求值或析构顺序，现用可观察的析构序列核对。常量表达式按每个运算的 f32/f64 精度舍入，短路分支仍做名称与类型检查。原子操作、限定 volatile 指针和有序栅栏在检查、C 和 LLVM 中共用同一类型，单独的 pthread 程序和优化后 IR 验证内存顺序。关键提交为 `7cb41c8`、`cd1a8ef`、`ad846b3`、`477d1cc`、`e37a4f2`。

Linux 原套件在最后的包中复现了一个 macOS 默认环境看不出的错误。exec 失败后先写诊断会丢失原始 errno，将缺少语言服务器报告成不可执行。交付修正 `61f94cb` 在输出和清理前取得系统错误。原始 Linux 失败收录于开发证据；测试还在 Darwin 与 Linux 上主动使诊断调用改写 errno，从而确定地区分 127 与 126。五个变化文件都在 JXZ 及其测试内。随后在 macOS 上经普通符号链接调用发行包，Rust 种子暴露了标准库定位错误。种子提交 `e8ef17b` 解析可执行文件及开发 profile 的真实路径，保留模块覆盖和安装隔离；新增四类别名测试及开发配置复验。

最终功能提交 `13941f6` 修复按值接收者的泛型实例化。普通的 `Box<i64>.into_value()` 曾因检查器将具体类型与原始 Self 模板比较而被拒绝。接收者调整现在只决定按值、借用或裸指针的传递形式，再由普通调用在实例化后检查实际类型。方法检查 109 项、协作式 async 236 项、owner 166 项通过，原正例保留。完整测试入口为 `sh scripts/test.sh llvm`，另可选 `seed` 或 `selfhost` 仅运行一侧的验收。

**终端计算实测。** macOS arm64 上用同一提交、release 工具链和输入比较 Rust 与 Aether 实现，预热一次、交替七次取中位数。程序处理 250,000 条 CSV 记录，斜率、截距、决定系数相同。测量来自 `WORK/accept-472e6fd/perf/measurements.json`，可用 `scripts/benchmark_host.py` 重做。

| 操作 | Rust 种子 | 自举编译器 | 峰值 RSS（Rust / 自举） |
| --- | ---: | ---: | ---: |
| JXZ 检查 | 145.87 ms | 16.09 ms | 71,745,536 / 11,976,704 B |
| JXZ 发射 C | 253.93 ms | 22.37 ms | 77,824,000 / 12,009,472 B |
| JXZ 生成 LLVM O2 对象 | 2,336.42 ms | 1,808.51 ms | 128,598,016 / 97,976,320 B |
| 运行流式回归 | 51.37 ms | 49.94 ms | 1,736,704 / 1,720,320 B |

编译器分别为 4,618,760 与 341,824 B，JXZ 对象为 104,840 与 99,608 B，两份回归可执行文件均为 34,112 B。上表测的是两种编译器实现。下方另列同一 Rust 实现的版本前后测量及出现过的回退。自举工具已可承接项目构建、长期服务、命令行统计、张量与量子程序；完整通用数据框、交互绘图、统计工具箱与 notebook 仍需独立建设。

**量子读出。** `quantum_marginal` 和 `quantum_compute_marginal` 按调用者的比特顺序返回计算基分布。两种 Bell 态的单比特边缘分布相同，双比特分布的支持集互不相交；测试直接使用解析概率。新增的 16 比特用例交换两根读出线并逐桶核对。Apple M4 Metal、RTX 3060 Vulkan 和 Adreno 750 Vulkan 均有实机证据，65,536 个振幅的最大误差为 `7.81873961e-8`。M4 记账峰值 2,129,920 B，Adreno 2,097,216 B，运行结束占用归零。这是 Aether 自有模拟器；CUDA-Q 通过另一适配器在 QPP/NVIDIA 上执行。峰值数字覆盖 Aether 管理的缓冲，设备驱动与 SDK 自身的常驻内存另计。

### 最终归档和复验

归档均来自功能提交 `13941f6730247ab7961011bdbd9de9033d1ec0a8`，外置根目录见 [HANDOFF](../HANDOFF.md)。

| 平台 | 实际复验 | 归档 SHA-256 |
| --- | --- | --- |
| macOS arm64 | 完整测试入口退出 0；双种子发布、搬移、3 个编译器别名、包内四组 JXZ 共 216 项；归档内 78 文件逐一校验 | `640f945633c6d435b7070f57ac893063867dbd82837f4f8f30b2d5a79e23f148` |
| Linux x86-64 | 双种子发布；57 项依赖/搬移/别名检查、81 项方法、108 项自举 JXZ、96 项统计、192 项独立 Shell、4 次 CPU/RTX 量子工作负载；79 文件逐一校验 | `a51295c27c954093d0038ea984a2164fb8bda208db8c218e0bc1533007a7cceb` |
| Windows x86-64 | 编译器和 JXZ 各经 C/LLVM、O0/O2 生成并链接 PE，携带 LLVM O2 两个程序、标准库、源码和 PowerShell 原生验收入口。旧 ZIP 165 项的 CRC、清单与 PE 导入表已校验。原生测试随后发现 JXZ 缺陷，替代包与修复见上节 | `e300eb9bd35af09b7df0bd69a61cab8a5e7931eddcb59ad3eec0914628935673` |

终端统计用精确有理数生成样本方差、协方差和回归系数的期望值；状态向量和选定比特分布使用解析概率校验。三块 GPU 已实际运行有序读出。Adreno 在 `e37a4f2` 使用了同一份量子库和 fixture，最终提交的 Linux 与 Mac 测试以最终源码运行。LLVM 与 C 的目标覆盖、Rust 专属内联汇编、协作任务限制、借用与 FFI 边界分别列在 [能力表](FEATURES.md) 和 [自举说明](BOOTSTRAP.md)。

## 较早的双宿主发行包与终端科学工作流

`15aedda` 的 macOS arm64 和 Linux x86-64 包均由提交快照从 Rust 种子构建。常规 Aether 编译器 `aethc-native`、由它产生的两个 JXZ 和两条种子工具链同包交付。Mac manifest SHA-256 为 `dbde4ed396e18376adf2b944a5a1bb8424a2665a0fb9f8d440416b9cf8e35869`。搬移后，包内编译器独立定位标准库，Script 输出 338350 和 25，流式回归读管道得到斜率 2、截距 1。双宿主构建重新从 Rust 开始，修复了可变形参语法差异、LSP 的 size_t 边界和布尔数值转换。整数转 bool 使用非零比较，浮点 NaN 转 bool 为真，均有常量及两后端运行回归。

`18baf08` 的同机比较使用 Rust release 与优化的常规自举编译器。每项预热一次，交替采样七次取中位数。两组使用相同 JXZ 源码、Clang、优化设置及 25 万行 CSV。RSS 取 macOS time 报告的 maximum resident set size，命令耗时包括它所启动的 Clang。原始数据在 `$WORK/t26-default-release/perf/measurements.json`。复现入口为 `scripts/benchmark_host.py`。

| 操作 | Rust 版 | 自举版 | Rust / 自举 RSS 中位数 |
| --- | ---: | ---: | ---: |
| JXZ 检查 | 146.60 ms | 13.25 ms | 69,763,072 / 11,599,872 B |
| JXZ 发射 C | 260.79 ms | 21.24 ms | 75,448,320 / 11,599,872 B |
| JXZ 生成 LLVM O2 对象 | 2,442.41 ms | 1,789.10 ms | 126,828,544 / 96,681,984 B |
| 回归程序处理 250,000 行 | 53.09 ms | 51.52 ms | 1,736,704 / 1,736,704 B |

两个编译器分别为 4,618,760 与 290,416 B，JXZ 对象为 104,328 与 98,384 B，两份回归可执行文件均为 34,112 B。这一对比涉及两种实现，覆盖的系统和交叉目标不同。较早版本同一 Rust 实现的优化前后结果见下文。

`stdlib/csv.aeth` 与 `stdlib/stats.aeth` 支持有界流式字段、样本均值、方差、协方差和一元线性回归。终端程序支持具名列、标准输入和可重定向 JSON。用精确有理数检查 101 对样本以及缺失、引号和退化输入，两种编译器各 96 个命令。当前可承接批量脚本、矩阵运算和数值核验；通用数据框、绘图、交互 notebook 和专用统计工具箱仍需相应生态。

## 从自举编译器构建 JXZ

`b55a11e` 由上一代 Aether 编译器构建，连续两代输出逐字节相同的 LLVM IR。两个后端分别编译仓库内 JXZ 的实际源码。生成的 JXZ 再调用该自举编译器构建有传递依赖的 Aether 项目，依赖修改后程序结果从 36 变为 40。42 条命令还核对了启动失败、子进程退出及信号状态、临时文件和非 ASCII 参数。入口为 `tests/bootstrap_jxz.py`。自举前端共用类型化语义，覆盖 Script、引用、枚举及错误传播、泛型实例及方法。独立 C 测试在 Apple arm64 和 x86_64 Linux 分别核对 19 项聚合调用。`497bd70` 继续补齐外部模块和内联命名空间。`5951201` 修复 match 语句分支均返回时的控制流判定。完整主编译器迁移和最终包仍在主线清单中独立追踪。

## 两种源表示的高级能力

捕获闭包（`451e2f8`）、常量泛型（`66ccafc`）、运行时接口分派（`e08aa2a`）、协作式任务（`0d06551`）各有通过和拒绝用例。联合题让任务先挂起，再恢复并通过动态接口取得偏移量，用捕获闭包计算样本并实例化固定长度数组。C/LLVM 的 O0/O2/O3 都依次得到 Pending、Ready(36)、Finished。独立复核发现 union 字段隐藏动态接口时能误过外部 ABI 检查，修复后连同别名、结构和枚举一并覆盖。证据在 `WORK/{t25-unified/full,t25-dyn/final7,t25-closures/final7}`。第一版采用静态模块图和显式 poll，任务的挂起位置、帧类型和 C ABI 边界见[能力表](FEATURES.md)。

## 基础冻结与自举原生项目构建

`f84e9d4` 的干净快照通过 macOS arm64 全套集成。CachyOS x86_64 的同提交包完成搬移、双后端项目运行和依赖验收。Linux 的严格 C11 JIT 编译原先缺少 POSIX 函数原型，搬移后的 JIT 编译器也找不到附带的 LLVM 库；`419be63` 修复两点，真实 Linux 安装包通过含第二编译器和消毒器的 27 项 JIT 检查。`f84e9d4` 修复 LLVM 指针复合赋值按错误步长写回的问题。相同程序在两个后端 O0/O2/O3 得到相同结果。详见 `WORK/{foundation-c36bd04,linux-mainline-20261118,compound-assignment}`。

Aether 编写的编译器可以自行生成 LLVM IR 或 C，再驱动宿主 Clang 生成对象及可执行程序。`fa4016c` 使 JXZ 能将精确的依赖映射交给该驱动。Mac 和 Linux 四代固定点产生相同 LLVM IR，SHA-256 为 `05ddf5e6ec85587182d6e735bee2a3c3a2f5372ca0f440ed5209b8217cfa5f79`。`ee987fb` 和 `5316467` 在原生 POSIX 错误后立即取线程本地 errno，系统调用连续两次被打断后仍能回收正确的子进程。各宿主分别通过 20 项 errno 和 47 项驱动测试。JXZ 此处作为调用方，现有的 Rust 种子与更完整的语言支持仍在迁移。

## 11 月 18 日类型、所有权和应用验收

固定同一份 JXZ 源码和 release 工具链，对 `cb154d1` 基线与仅应用 `9edda38` 核心改动的版本预热后交替运行九次。表内为中位数，RSS 单位为字节。编译器可执行文件从 4,246,840 B 变为 4,225,240 B。生成 C 均为 5,660,310 B，LLVM O2 对象均为 105,992 B，各组前后 SHA-256 相同。此表衡量编译器阶段；小程序执行时间及先前版本的回退在下方历史数据中。原始数据在 `WORK/typed-ir-performance-20261118`。

| 操作 | 原耗时 → 新耗时 | 原峰值 RSS → 新峰值 RSS |
| --- | --- | --- |
| JXZ 类型检查 | 170.74 → 159.74 ms | 92,209,152 → 72,859,648 |
| 生成 C | 404.37 → 364.67 ms | 300,302,336 → 84,508,672 |
| LLVM O2 对象 | 1,691.00 → 1,677.58 ms | 841,908,224 → 734,740,480 |

`c379133` 修复局部变量被同名导出全局截获。`c13f93f` 在 x86_64 SysV 上根据八字节分类、对齐和剩余寄存器决定结构体传参和返回，独立 C oracle 完成 CachyOS 原生 18 组联验。Darwin arm64 通过 78 项。LLVM 目标不支持的聚合 ABI 在编译前报告 E2407。

`b243b72` 使泛型 Vec、fd、Tensor 和量子 lease 的拥有者在正常返回、提前返回和错误传播时执行相应析构。六个原生组合与统计分配器验证了资源释放。ASan/UBSan 用例分配 1,025 次，退出存活数为零。带 owner 的嵌套聚合仍产生 E2405。`d31ea10` 的目录创建接口能在仅有搜索与写权限的父目录下工作，并保留原始 POSIX 错误。

在 Aether 里生成的算子图可以由 C 或 LLVM 后端编译为本机 CPU 内核，经导出入口校验后缓存。稀疏算子、逐行 int8 量化、低秩近似、梯度、扩散与金融方程、MaxCut 和矩阵链均有独立数值参照。四矩阵实例的最优成本为 162，左结合成本为 265；已知谱 4、2、0.5 的二阶截断残差为 0.5。M4 Metal 与两台 Vulkan 设备使用相同的 Aether 上层程序。RTX 3060 和 Fold6 的 C、LLVM 两版均完成 125 次提交，计费峰值分别为 17,456 B 与 17,600 B，释放后归零；Fold6 两个进程峰值为 33,520 与 33,508 KiB。数据、程序输出与独立参照在 `WORK/compute-{applications,planning,gpus}-20261118`。

关联 Shell 的 `44b3da7` 修复短管道进程组竞态。两套固定语言包各通过 14 个 Shell 套件，并各自在单一进程中执行 1,000 条管道。每 125 条采样，Mac/Linux 均保持 6 fd，RSS 分别从 2,736 增至 2,816 KiB、从 3,048 增至 3,072 KiB。设备图形 Bridge/Broker 的验收另有部署依赖。证据在 `WORK/shell-joint-20261118`。

自举提交 `57ed11c` 由上一代 Core 构建完整的 13 模块 Aether 编译器及直接 LLVM 发射器，实编原有 accumulator 服务和标准库消费者。连续自举产物的 C 为 651,158 B，SHA-256 为 `7a7930926809d40954784ce7a67e588b1486e9b58bc92efe8e8f112a6e6e1ace`；LLVM IR 为 1,013,333 B，SHA-256 为 `7f370b537f537b1ec8601d6589dc6b6ae893d6df651753dac44713bcbcc9521e`。C、LLVM 两个入口各通过 131、104 项，见 `WORK/t26-production`。后续阶段继续迁移 JXZ 需要的前端能力。

## 11 月 18 日双宿主、16 比特量子及自举验收

`423e664` 的 macOS arm64 和 CachyOS x86_64 包均提供 C-only/LLVM+C 两版编译器与相应 JXZ。Linux 携带私有 LLVM 21，清单 SHA-256 为 `940b9094affe63be2feef2af2b5a9297c35b258db4986446febc953e565e4326`。macOS 清单为 `ea387876edc9ef2674a36b3ba2a86211f7a78b93316bae9afe6a7252cb2d5f03`。Linux 缺少 shasum 曾使正常包获取失败，现由 Aether 流式校验受限长度的文件；已用 56 组数据与独立 SHA-256 对照。两套 Linux 安装工具分别通过 40 项 JXZ 项目和 14 项注册表测试。使用这对宿主包的 Shell 分支为 `e1e99bb`，两台机器各完成 51 项原生功能、27 项 PTY 与 65 项脚本主套件，见 `WORK/shell-joint-20261118`。

`10355a4` 的量子态使用复 f32，与独立的 f64 参考模拟器在 65,536 个复振幅上逐项比较。CPU、M4 Metal 及两台 Vulkan 真设备的最大绝对误差约 7.82×10⁻⁸。相同电路、三个泡利轴、参数移位梯度及经典角度更新由 Aether 实现，梯度约 −0.398609264。Metal 计费峰值为 2,129,920 B，NVIDIA 与 Adreno Vulkan 为 2,097,216 B。每轮完成 42 次提交并验证释放后计费归零。新 CUDA-Q SDK 适配的同一程序在 QPP 运行，记录 `on_gpu=0`。RTX 的新 16 比特证据来自 Aether Vulkan，旧 CUDA-Q NVIDIA 验收及其设备选择另存于 `WORK/cudaq/final`。逐后端记录在 `OUT/q16`。

`9839f2f` 和 `06207c6` 让自举编译器从项目入口读取模块，并在 Aether 内生成 C 或直接 LLVM IR。由上代编译全部 13 个模块后三代产生相同 LLVM，SHA-256 为 `691486c1997e60e03c7df6e33d43f7ac32ba2e59edc2a30dc2e7a0297e7d9229`。LLVM 第四代生成与 C 种子相同的 C；宿主 Clang 负责 LLVM 优化、目标码及链接。C、LLVM 两条独立验收的记录在 `WORK/t26-llvm/{c-final,final-entry}`。Rust 主编译器的更广语言支持仍在连续迁移。

## 11 月 18 日冻结包与后续实机增量

`c5ed0b6` 的 macOS arm64 安装包包含 C-only 和 LLVM+C 编译器、两版 JXZ、Core 第四代和匹配的 stdlib；整包 manifest SHA-256 为 `a7ab8bf0f038a1d8ddc356fc2cefcbc911f2c736319d5f89c9be26d6217dc2e7`。两版 release 分别通过 116 个自举检查。安装至新前缀后，在独立工作目录完成 JXZ 创建、构建、执行、测试发现和清理。包目录为 `WORK/releases/aetherlang-foundations-20261118`。

同机、同输入、每组七次。与 `c964ccb` 相比，LLVM+C 编译器由 3,834,616 B 增至 4,108,536 B。C/LLVM 生成耗时中位数分别从 15.70/18.56 ms 变为 30.14/30.96 ms；编译器峰值 RSS 从 22,200,320/29,802,496 B 变为 22,691,840/30,031,872 B。两者 O2 程序均从 33,496 B 变为 33,432 B，执行中位数从 11.18/11.22 ms 变为 12.74/12.69 ms。两版采样分时进行，短程序耗时含进程启动与系统调度。保留回退数据和每次样本于 `WORK/final-review-20261118`。长服务 C 和 LLVM 在 2,000 至 100,000 个更新期间分配驻留稳定，分别为 1,720,320 B 和 1,736,704 B，均为 4 个 fd。

后续 `dc6ae97`、`def6d5c`、`6e91161` 增加最小作用域所有权、JXZ 远端获取以及临时文件故障保护。数值转换 out-pointer 与 Linux 临时目录 C 原型由 `0bdfe56`、`c8456f5` 修复。GPU 快照 `4d36291` 通过 CPU、原生 Metal 和 Vulkan 队列执行同一 Aether 程序。Vulkan adapter 的可复现入口为 `scripts/build_vulkan.py --output libaether_vulkan.so`；交叉构建可指定 NDK 的 `--cc`。额外 SDK 的静态/动态库路径由 `--include` 与 `--library` 传入。

| 设备 | 数值与边界验收 | 存储及观测 |
| --- | --- | --- |
| Apple M4 | 原生 Metal C/LLVM O0/O2 对照及 ASan/UBSan；MoltenVK 的 11×13×9 跨步乘法全部 99 项吻合 | Metal 按页对齐的 backing 和参数缓冲计费 |
| NVIDIA RTX 3060 Laptop | C/LLVM 两个 Vulkan 程序都通过 99 项 oracle、上层张量结果与同步/错误检查 | 映射堆 6 GiB，本次 SDK 报可用 6,049,955,840 B。上层测试显式预算峰值 1 MiB；进程峰值 RSS 各为 187,136/185,620 KiB |
| Fold6 Adreno 750 | Android 29+ NDK 的 C/LLVM 产物通过相同两组程序，数值误差 0，队列完成 4 次提交 | 映射堆 11,654,283,264 B，单缓冲上限 128 MiB；未报告可用余量。上层测试峰值 1 MiB；进程峰值 RSS 各为 34,840/34,884 KiB |

Vulkan 在分配前按驱动返回的 memory requirements 检查预算，命令与描述符的驱动内部占用另计。带 stride 的 99 项测试同时校验未覆盖元素、已经用尽的缓冲预算和越界拒绝。SDK 分配与同步规则依据 [Khronos Vulkan Memory Allocation](https://docs.vulkan.org/spec/latest/chapters/memory.html) 和 [Synchronization](https://docs.vulkan.org/spec/latest/chapters/synchronization.html)。真设备日志位于 `OUT/vulkan-build` 及 `WORK/compute-runtime-20261118`。

## 2026-11-18 本地开发版

本轮沿用 `work/aetherlang-upgrade`，从 `4cb768c` 继续。性能对照为上次冻结的 `c964ccb`。原审计 29 组和 Shell 后补 5 组的复现、提交及验收入口见[主线任务](../MAINLINE_TASKS.md)。

| 领域 | 升级前 | 当前实测能力 |
| --- | --- | --- |
| 数值、模块与调用 | f32 算式先按 f64 计算，自定义 add 被误认成指针操作，Self 与模块私有性在后端失真 | 语义、IR、效果与导航共用解析结果。原 f32 判例在两后端 O0/O2/O3 都得到 0。递归 Vec 可插入、读取和释放 |
| 元组、接口与契约 | 元组仅能解析，缺方法的接口实现也能通过，后置条件未执行 | 非空元组可嵌套、复制、逐字段初始化与返回，并受借用检查约束。函数入口与正常出口都执行契约，包含 `?`；失败触发 trap。返回表达式仅求值一次 |
| 系统开发 | 窄整数 C 互调、32 位指针整数、Darwin 部署版本及 C void 指针出现跨层差异 | 通过独立 C 调用方、实际 wasm32 执行、Mach-O 对象和可执行文件验证。no_std 纯计算可无系统头文件生成对象；受限 x86_64 中断函数有 iretq 对象证据 |
| 标准库和资源 | Vec 对齐不足，文件接口丢失长度/errno，旧模块无法用现有前端导入 | Vec 超对齐/失败回滚，限额二进制流和文件读写，显式释放的 arena/stack，严格的小型 TOML 子集、可增长的标量 DAG、OpenBLAS 和原生 Metal 计算均有消费者 |
| 工具链 | JXZ 在产物和登记之间中断可能损坏状态，测试发现会吞错误，不支持本地依赖 | JXZ 0.3.0-dev.2 持锁恢复、解析本地依赖图、拒绝循环和重名，库修改后重编。安装器按清单校验并原子切换整个版本。CLI、LSP 使用同一包入口解析器 |
| 自举、诊断 | Core helper 抢占外部名称，u64 大数转换失败，过深表达式能使主编译器异常退出 | 七模块 Core 编译器保留外部符号和完整 u64 转换。深度越界产生有源位置的诊断，LSP 在修正文档后可继续工作 |

### 原生工作负载

- JXZ 的配置、依赖图和事务实现均用 Aether 编写。真实项目覆盖构建、测试、运行、清理、发现权限错误、子进程失败、并发排斥及 SIGKILL 恢复。
- accumulator 在同一进程处理 2,000 次更新、EOF、输出及日志失败。堆排序、搜索和归约程序输出 `compute 17218136811 46 1048510 33332.833335`。另有 4,000 节点标量 DAG、300 项含 Unicode 的 TOML 文档。
- Metal 共享缓冲区运行 257 项 f32 仿射变换，Aether 逐项校对并释放资源。输出 `metal registry=4294968333 values=257 first=-255 last=257`。OpenBLAS 验证矩阵乘法、向量运算及双精度点积；LLVM 与 JNI 的消费者链接本机真正的 SDK。
- Metal 通用计算与 CUDA-Q 各有自己的设备入口。此前 RTX 3060 结果属于历史版本。本轮尚无 Metal 量子模拟、Adreno 实机峰值或跨设备自动调度的验收。

### 使用边界

Core 的词法、解析、检查与 C 生成器用 Aether 编写并逐代自编译，机器码由宿主 C 编译器生成。Rust/LLVM 主编译器仍由 Cargo 构建。捕获闭包会明确报 E2403，tuple 跨 C ABI 按值传递报 E2404。资源结构体仍需按 API 的约定显式交接与释放。下一项语言基础优先处理 owner 的隐式复制与作用域退出清理，覆盖普通返回和 `?`。

复现环境见[交接](../HANDOFF.md)，兼容性变化见[迁移说明](MIGRATION.md)。本地包使用示例见[JXZ](../jxz/README.md)，BLAS 与 Metal 见[数学接口](MATH.md)。Shell 自身的产品固定值由独立项目维护。

## 2026-11-14 交付与 11 月 15 日增量（历史版本）

2026-11-14，仓库 `J-x-Z/AetherLang`，本地分支 `work/aetherlang-upgrade`。原始起点为 `aceff1522790df3edfe5c711db5884350ab1b44a`。全部源码、工具链、缓存、产物和证据在 `/Volumes/WINDOWS/work`，没有推送远端。最终功能源码为 `0709b33`，主编译器源码为 `a017d18`；交付提交和产物 SHA 见外置 `release/manifest.json`。

这轮完成了主编译器正确性修复、系统与数学基础库重构、真实 Core 自举，以及 CUDA/cuBLAS 和 CUDA-Q 的实际运行。LLVM 仍是主后端；C 后端提供无 LLVM 的独立编译器、可移植实现和差分参照，并承担 Core 自举的机器码落地。没有把语言限定为 Shell 或插件语言。

## 从起点到交付

| 范围 | 实际改善 |
| --- | --- |
| 工具链 | Rust/Cargo 固定为外置 1.100.0；复用 LLVM 21.1.8。构建、诊断、测试、LLVM IR/对象输出和链接选项有统一入口；未修改全局 Rust 默认 |
| 值与 ABI | 数组/结构体/枚举保留值语义；复制、返回、嵌套布局及 packed 访问正确。LLVM union 使用保留原始字节的存储，修复 Result 错误码被 bool 载体截断；独立 C ABI 检查覆盖 Apple arm64 双向聚合调用 |
| 类型与语义 | 初始化和引用按声明身份与投影跟踪；局部引用逃逸、错误效果、错误方法和错误 `?` 被拒绝。递归引用摘要求解真正收敛；嵌套 enum/bool pattern 覆盖检查、引用数组索引和求值顺序修复 |
| 数值 | 完整 i64/u64 字面量范围，包括负 MIN、十六进制、常量、匹配和泛型上下文；超范围拒绝。整数按位宽 wrap；除零及有符号 MIN/-1 trap；浮点转整数截断后饱和，NaN→0 |
| 泛型与容器 | 函数、结构体、枚举、别名按需实例化；真实 tag+payload、Result 传播、Option、Vec 与零大小值可运行；扩容失败保留原缓冲区 |
| 系统编程 | static/extern static、union、64位布局查询、raw pointer、C FFI、inline asm 保留；typed volatile、原子/CAS/fence、可移植 SIMD 均进入共同的类型/IR规则。目标 errno intrinsic 按 libc 选择符号 |
| POSIX 进程 | fd、pipe、spawn/wait/poll、环境和 cwd、信号及多段流水线；处理 EINTR、短 I/O、exec 失败与 exit127 区别，失败时关闭/终止/回收子进程；1 MiB 真实流水线验证 |
| 数学 | 用一套 CPU f64 rank0–4 张量替换重复/占位实现，支持形状与溢出检查、保留底层存储的切片/转置、矩阵乘法、tanh、反向 AD。matrix/autograd 复用相同实现，删除旧生成 C 副本 |
| 自举 | 删除旧占位编译管线和过期 bootstrap 二进制，改为 Aether 自己实现完整 Core 词法、Pratt 解析、类型检查、C 生成和驱动；完成四代原生构建 |
| 量子与 CUDA | Aether 实现电路、10类门、complex128 参考模拟器、Pauli 期望、参数移位与优化循环；小型原生 SDK 适配层接 CUDA-Q。CUDA runtime/cuBLAS 保留低层接口并新增可失败的 buffer/stream/handle 管理 |
| JXZ 与工具 | 真实构建、argv、配置、产物原子记录和 clean 保留用户文件；不支持的依赖配置明确拒绝。JSON 诊断、UTF-8 原文件范围、`.ath` 映射、stdio LSP 与未保存缓冲区查询共用编译输入 |

`.ath` 与 `.aeth` 共用后续类型检查和 IR。前端语义流、效果、常量/初始化、枚举检查拆成各自负责的模块；类型、常量、SIMD、内存与平台操作通过共同事实传给后端。重构用于消除跨层规则差异，而非仅移动文件。

关键本地提交：`aa61728`（SIMD/errno/union/递归引用）、`975a266`（进程与 fd）、`06c91f3`（张量与 AD）、`f8274c4`（完整 Core 自举替换占位）、`ca9e1c9`（CUDA）、`a017d18`（完整整数/嵌套模式/引用索引/Linux PIC/外部导入）、`654d0aa`（CUDA-Q）、`0709b33`（16比特内存池与lease生命周期）。更早的主编译器、ABI、工具链改动也保留在分支历史中。

## 实际程序与验收

主机 macOS arm64、16 GiB；Cargo 并发3，构建串行，复用缓存。下表有些是构建/运行/拒绝检查记录，不应相加声称独立测试数量。

| 验收入口 | 实际结果 |
| --- | --- |
| Cargo LLVM / C-only | 126 / 122 单元通过；各自 backend regression、driver14、entry1、recursive reference2、tooling3通过；原始起点42单元 |
| `scripts/validate.py` | 22组计算、持续服务与 `.ath` 组合通过，最终 LLVM+C release 再验22组 |
| aggregate / system / semantic | 49 / 71 / 54条记录通过；含独立 C ABI、packed、引用/初始化/NEVER 和 ASan/UBSan |
| runtime / generic / memory | 基础库6组；泛型28运行+4拒绝；memory 8运行+6拒绝+2 IR检查，另有夹具构建与历史x86对象证据 |
| SIMD / platform | 43 / 32条通过；Linux PIC 与裸机静态重定位分别核验；Linux GPU 的实际 PIE 链接也已通过 |
| process / JXZ | process 88条（22场景×2后端×2优化）；最终 release JXZ LLVM/C各15项 |
| math | 23条通过，最终 release 再验；12个参数有限差分、训练、视图存储生命周期、分配失败及 ASan/UBSan |
| 自举 | LLVM seed 与最终 C-only release seed 均完成103项验收；另各执行83个 Core 语义样例的普通/消毒器版本 |
| CUDA | 故障 shim 9步（含4次资源管理运行）；Linux RTX3060 真正 SGEMM/DGEMM，C/LLVM O0/O2四个程序通过 |
| 量子参考/预算 | 参考12条及新增预算19条通过；含16比特GHZ、分配失败回滚和 ASan/UBSan |
| CUDA-Q | Mac CPU 原生 SDK16条（原电路和16比特预算各4构建+4运行）；Linux NVIDIA 原生 SDK，C/LLVM O0/O2四个程序通过；SDK边界12项通过。16比特预算路径另在Mac CPU验收，见下文 |

统一测试入口已接这些套件及自举。此次完整验收分成 `foundations-full-llvm.log` 与 `foundations-full-rest.log`：新增 bitwise 测试一处表达式括号修正后，从该套件续跑剩余检查；未把首次中断标作单次全套成功。之后的 Linux PIC、发布构建与量子预算改动分别执行关联验收。没有重复运行多个全量构建来消耗预算。

计算负载由 Aether 完成32768元素堆排序、二分查找、伪随机数、整数与浮点归约，输出：

```text
compute 17218136811 46 1048510 33332.833335
```

持续运行的 accumulator 在同一进程接受 add/get/reset/quit；验证2000次更新、非法输入、EOF、journal和输出错误。并发内存测试由 C 夹具负责创建/join线程，Aether 实现原子累加、锁保护计数和 release/acquire 发布。进程流水线的生产、变换和消费同样由 Aether 实现。

数学验证的12参数有限差分最大误差为 **5.46057643547e-12**；240步训练损失从 **1.11046027306** 降至 **1.98453930174e-13**，权重 **1.19999881519**。另独立复核了转置/切片、释放父张量后保留视图、共享节点梯度累计、重复 backward 和梯度视图生命周期。

Core 自举的七个模块拼接后 **77,824 B**。Rust种子→stage1→stage2→stage3→stage4；后续三份 C 均 **307,590 B**，SHA256 为 `7e718ccc2b9586620b3058a3bfc0e3621939861d7cac37b671c27fdb3fa31995`。stage2/3/4各 **70,856 B**；没有声称原生二进制哈希相同。自举包含递归堆结构、含 NUL 的文件、求值顺序、trap、输出保护、深 AST 拒绝及 ASan/UBSan 自编译，PATH trap 确认后续没有偷偷调用 Rust 编译器。完整范围见 [BOOTSTRAP](BOOTSTRAP.md)。

多轮审查修复了自举常量一元求值、结构体临时值字段读取、MIN/-1 行为和左深 AST 栈溢出；主编译器整数另经270个 Python 大整数期望值与 C/LLVM O3 差分。CUDA-Q 的 CPU 显示索引与底层振幅顺序不同，适配层已按逻辑 little-endian wire 读取，避免表面“同一电路”却得到错位状态。

## GPU、统一内存与后续 Adreno

Mac ARM 上的 CUDA-Q 使用 **CPU qpp**，没有 Metal 后端。Linux RTX3060 Laptop 6 GiB 上实际使用 **cuStateVec fp64**，程序确认 `on_gpu=1`。运行环境为 CUDA-Q 0.16.0.post1、cuStateVec 1.15.1、CUDA13.4、Clang23；Mac SDK 为0.16.0。SDK安装限任务目录，远端验证证据保存后已删除任务临时目录，没有修改全局工具链。[官方平台说明](https://nvidia.github.io/cuda-quantum/latest/using/quick_start.html)

量子程序对八组混合三比特电路逐项比较复振幅，再由 Aether 执行40步变分优化，CPU/NVIDIA均得到 **-0.999999575958**。cuBLAS 程序使用17×19与19×13矩阵、alpha1.25/beta0.5，Aether 自己计算期望矩阵；两种精度的本次最大误差均0。这些证明正确执行，不是GPU加速比测量。SDK adapter只转换门记录、管理SDK对象并复制结果，不承担算法或优化器。

最新设计约束是 **8 GiB统一内存优先、16 GiB扩展、16比特作为首个跨设备验收规模**。纯态向量只需 `2^n × complex_bytes`；16比特complex128为1 MiB，24比特256 MiB，28比特4 GiB。16比特的完整complex128密度矩阵却需64 GiB；不能把两种模拟模型混为一谈。

新增 QuantumPool/QuantumLease 对主机振幅分配执行字节、qubit与同时存活状态数量限制；释放返还额度，关闭pool后现有lease保持有效。16比特/2状态/2 MiB为小配置。CPU参考和CUDA-Q输出采用同一准入接口。该预算不包括元数据、门列表、SDK/JIT内部工作区或额外设备副本，不能冒充整个进程RSS上限，也没有用16 GiB Mac结果宣称已在8 GiB手机验收。 在本机各一次的16比特O2原生测量中，参考程序RSS为 **3,866,624 B**，CUDA-Q CPU对照为 **60,325,888 B**；记录见 `foundations-release/quantum-16-memory.json`。新增池路径已验Mac CPU，NVIDIA证据对应此前未使用池的接口。

Metal/Adreno 仍是后续真实后端任务，当前未实现通用异构调度器或GPU kernel编译器。后续应按能力查询精度、可用预算、队列和同步，再选择后端；Adreno可评估Vulkan compute。双精度不能成为所有设备的默认假设，`shaderFloat64`是需查询/启用的功能。[Vulkan能力定义](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceFeatures.html) 当前实现继续保持complex128语义，不以偷偷降精度换“兼容”。

## 实测性能与开销

同一两百万步标量计算，必须输出`652496`；原始与最终release同机顺序执行，预热后各5次。耗时为中位数，RSS为生成阶段最大值。测量期间本任务无并行构建/测试。原始版Rust1.99、最终版Rust1.100，因此比较交付版本，不隔离某一处优化。

| 指标 | LLVM 原始→最终 | C 原始→最终 |
| --- | --- | --- |
| O0生成耗时，不含链接 | 14.355→20.065 ms | 5.808→6.389 ms |
| 生成阶段最大RSS | 29,982,720→29,818,880 B | 3,096,576→4,784,128 B |
| 对象/C源码大小 | 1,096→1,176 B | 1,476→4,375 B |
| O0运行耗时 | 15.422→17.085 ms | 25.137→36.260 ms |
| O2运行耗时 | 15.512→12.813 ms | 13.051→12.886 ms |
| O0程序大小 | 33,480→33,496 B | 33,480→33,560 B |
| O2程序大小 | 33,480→33,496 B | 33,480→33,496 B |
| 编译器文件大小 | 94,813,504→3,832,856 B | 2,411,552→3,715,288 B |

该短样本LLVM O2运行约快17.4%，但生成约慢39.8%、O0运行约慢10.8%；C O0运行约慢44.2%，O2差异约1.3%。不据此宣称普遍加速。增加语义检查及定义明确的数值行为有成本；未来优化应保留这些语义。

LLVM文件缩小主要因为改用共享LLVM，不能当作整体安装体积下降。C-only文件和RSS增加。短样本含进程启动，5次采样没有总体性能置信区间；GPU时长含SDK/JIT初始化，未作为加速比。详细采样、编译器SHA与原生链接时间在 `benchmarks/foundations-{llvm,c}/measurements.json`。

## 构建、测试、交付文件

```sh
cd /Volumes/WINDOWS/work/AetherLang
export RUSTUP_HOME=/Volumes/WINDOWS/work/aetherlang-upgrade-work/rustup
export CARGO_HOME=/Volumes/WINDOWS/work/aetherlang-upgrade-work/cargo-home
export CARGO_TARGET_DIR=/Volumes/WINDOWS/work/aetherlang-upgrade-work/target
export TMPDIR=/Volumes/WINDOWS/work/aetherlang-upgrade-work/tmp
export LLVM_SYS_211_PREFIX=/opt/homebrew/opt/llvm@21
export CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3
sh scripts/build.sh llvm
sh scripts/test.sh llvm
```

`build.sh c`/`test.sh c`选择无LLVM构建。这里的CARGO_HOME是外置离线缓存；其他机器可用自己的正常缓存。已有SDK时，设置 `AETHER_CUDAQ_BRIDGE` 增加原生SDK测试；GPU后端另设 `AETHER_CUDAQ_GPU=1`。CUDA GPU实测入口为 `tests/cuda_native.py`，无设备退出77而不是假装通过。命令详见 [QUANTUM](QUANTUM.md)、[MATH](MATH.md)、[BOOTSTRAP](BOOTSTRAP.md)。

外置 `release/` 包含 `aethc`（LLVM+C）、`aethc-c`（C-only）、`aethc-core`（自编译stage4）、`jxz`、`jxz-c`及 `manifest.json`。C-only实查只依赖libSystem，LLVM版依赖本机共享LLVM。核心源码与stdlib保持在仓库中，不把当前主机二进制宣传为可直接复制到任意系统的安装包。

证据根 `/Volumes/WINDOWS/work/aetherlang-upgrade-work`：

- `evidence/foundations-*`：主套件、C-only、release构建及性能日志。
- `target/*/results.json`：完整功能套件；`foundations-release/`：发布二进制、C-only自举、JXZ和量子预算验收。
- `cudaq/final/gpu-validation.json`：八个NVIDIA GPU原生程序的构建/运行、生成物SHA、工具链和设备；`remote-cleanup.json`记录任务目录清理。
- `new-foundations/{integer-literals,relocations,math-review}/`：独立整数oracle、重定位与数学复核。

跨目标状态分开记录：Mac arm64完成编译/链接/运行；Linux x86_64已完成本文GPU程序的编译/链接/运行，但未因此宣称Linux所有POSIX程序已验收；Windows、Android、其他裸机目标的现有证据仅为对象编译/IR或重定位检查，未冒充链接/运行。远端CI未运行。

## 迁移与仍需处理的问题

- 数组采用值语义，浮点转整数采用饱和规则，默认整数为i64；u64上半区需要明确类型上下文，负MIN现在合法，超范围常量拒绝。
- 使用 `Vec<T>`/`vec_*`、独立Option/Result模块。资源handle、张量、Tape、QuantumPool与lease仍有显式释放约定，复制raw owner不等于深拷贝或所有权转移。
- 数学统一到tensor/autodiff；旧占位API不能继续当作已实现能力。`matrix`和`autograd`保留复用入口；具体操作和错误返回见MATH。
- Core自举支持自己的函数/标量/指针/结构体/C FFI/控制流子集；不支持完整语言的模块、泛型、枚举、数组、引用等。主Rust/LLVM编译器继续提供这些能力。
- 未标效果的函数推导效果，pure/显式列表为上界；未知extern保守处理，重复声明不能削弱效果。C可见函数写`#[no_mangle]`，对象输出`--emit-obj`，链接用`--link-object`/`--link-library`。
- LLVM外部聚合C ABI目前实现并验证Apple arm64，其他目标明确拒绝；内部聚合与标量/指针外部接口不受该限制。JXZ依赖图/锁文件/注册表和Windows进程层仍未完成，相关配置明确拒绝。
- 当前没有自动析构、完整所有权/trait/closure/async/const-generics体系；数学不是完整JAX/JIT/vmap，量子不是QPU实机验证；Metal/Adreno与自动异构调度未实现。旧标准库未验证的模块不计入交付能力。

下一项应先建立资源拥有者的不可隐式复制语义与作用域退出清理，覆盖提前return、Result传播和多资源失败路径。异构计算下一里程碑是同一16比特电路在CPU、NVIDIA、Metal及Adreno上有明确精度、峰值内存与同步证据；应先实现一个真实的新后端和能力查询，再扩大调度器。Shell可复用这轮进程/文件/管道基础，但本轮不替用户决定具体项目迁移。

## 2026-11-15增量：跨模块C ABI回调定义（N04）

提交`c964ccb`使同一或不同已加载模块的兼容extern C声明能够指向唯一no_mangle Aether函数体，不再因E2003拒绝合法回调。保留签名/变参/名义类型/重复定义检查，C ABI的Unit返回规范化为void；泛型固定符号需改为具体包装函数。详见LANGUAGE.md和MIGRATION.md。此能力没有扩大Core自举子集。

原4行问题已复现并闭合。160 Cargo测试、98两后端O0/O2/O3定向检查、15独立探针命令、49聚合ABI检查、103完整自举检查、22真实负载及JXZ每后端19项通过。证据`WORK/extern-definitions/receipt.json`；独立开发包`WORK/releases/extern-definitions-20261115/manifest.json`。编译器3834616B，比上一开发包增加1248B；没有新的RSS/运行性能对比。Shell保持旧pin，本次没有产品联验或Linux宿主交付；主线其余问题见MAINLINE_TASKS.md。

## 2026-11-15原固定源码的Linux LLVM宿主交付

Shell不升级编译器：从5f84a3f快照在CachyOS构建Linux x86_64 aethc/JXZ，编译器与a634366相同、44文件stdlib与Mac lock一致。使用既有Rust1.99.1、私有LLVM21.1.8和离线依赖，jobs1。包与校验在`WORK/releases/linux-x86_64-frozen-5f84a3f-20261115/`；归档66954543B。aethc4345688B、JXZ55824B；没有跨架构性能改善声明。

原生126单元+20集成、LLVM JXZ15、process44、foundation10、11真实负载、103完整自举及15搬移检查通过；Core生成C固定点与原Mac版本相同。必须携带lib/并显式设置AETHER_PATH或-I。Shell产品与host lock联验由应用侧完成，本次未改其pin。

C版JXZ在Clang23暴露N05：*void错误输出为Unit结构体指针，与FILE*不兼容。原失败、C11/C17对照和最小C失败/LLVM通过证据均保留；C没有被计入这次LLVM通过声明，也未通过抑制诊断绕过。该问题同样存在当前源码，已归主线T02/T07/T18待修。
