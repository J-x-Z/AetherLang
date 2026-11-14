# AetherLang 集中升级交付

> 状态更新（2026-11-15）：本文记录2026-11-14快照及当时测试结果。追加多代理审计发现了数值/ABI、名称捕获、效果、分配对齐、JXZ错误处理等未修缺陷，不能将本文的通过记录解释为完整正确性保证。当前执行任务及证据入口见 [MAINLINE_TASKS.md](../MAINLINE_TASKS.md)，交接见 [HANDOFF.md](../HANDOFF.md)。本次仅更新审计与任务状态，没有生成新的编译器发布。

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
