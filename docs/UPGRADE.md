# AetherLang 集中升级交付

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
