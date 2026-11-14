# AetherLang 集中升级与问题闭合记录

2026-11-14；仓库 `J-x-Z/AetherLang`；分支 `work/aetherlang-upgrade`。起点 `aceff1522790df3edfe5c711db5884350ab1b44a`，最终编译器源码 `234f888`。全部提交保存在本地，未推送。源码、工具链、缓存、产物和证据均在 `/Volumes/WINDOWS/work`。

发布版本已完成真实程序验收，性能与资源数据来自最后冻结的编译器。

## 实际能力变化

LLVM 保持主后端，C 保留为无需 LLVM 的备用实现及独立运行/ABI 对照。复用 LLVM 21.1.8，Rust/Cargo 固定为外置 1.100.0，没有修改全局 Rust 默认。`.ath` 与 `.aeth` 进入共同的解析后处理、类型检查和 IR；本轮没有另造一套 AI 专用运行规则。

| 问题/能力 | 升级后的实际行为与证据 |
| --- | --- |
| 固定数组、结构体、返回值 | 数组保留真实值类型；复制不别名、返回不悬垂，嵌套数组及含数组结构体正确；独立 C ABI 对照与 ASan/UBSan 覆盖原栈返回后访问和 packed 对齐问题 |
| 初始化、引用和控制流 | 按声明身份而非变量名跟踪初始化、借用、字段投影、引用来源和逃逸；合法遮蔽/借用结束被接受，未初始化读取及局部引用通过函数/聚合逃逸被拒绝；NEVER 事实按具体函数与源码位置共享给 IR，return 后不再生成存储或覆盖终结指令 |
| 数值与 C ABI | 浮点转整数定义为截断后饱和：NaN→0、上下越界钳位；两后端/优化等级一致。内部 LLVM 聚合调用与外部 C ABI 分离，Apple arm64 双向聚合 FFI 实测通过；packed C 声明修复 |
| 泛型和错误值 | 按需实例化函数、结构体、枚举及类型别名；真实 tag+payload union 布局、匹配和 `Result ?`，允许成功类型变化、要求错误类型一致；没有哈希占位或统一降成 i64 |
| 容器与资源 | `Vec<T>` 的创建、扩容、访问、修改、弹出、释放；分配失败保持旧缓冲区。`Option<T>`、`Result<T,E>`、`Vec<()>`/空结构体/零长度数组都实际运行。文件成功、打开失败、刷新失败及重新分配失败链路有断言 |
| 系统能力 | static/extern static、初始化与地址重定位、union、size/align/offset 真正进入 IR；非法初始化与不安全 union 读取有诊断。LLVM 数组长度使用64位API，静态数组地址使用目标索引宽度，超过2^32的布局查询不截断 |
| volatile 与原子 | `*volatile T` 保留访问限定，`@volatile` 与显式 intrinsic 使用同一 Memory IR；packed/ZST 访问有正确路径。u8/u16/u32/u64 原子、CAS 和 fence 真正生成对应指令，内存序是受检查的编译期常量 |
| 基础库与 JXZ | 字符串、字节 I/O、文件、参数、路径和 POSIX 进程进入真实应用；普通输出不再把文本当格式串。服务保留首次 I/O 错误并完成关闭；JXZ 统一拒绝不支持的依赖 schema，原子更新全部自有产物记录，clean 保留用户文件 |
| 工具链 | JSON 诊断含稳定分类、原文件 UTF-8 范围；`.ath` 位置映射、跨文件符号查询和真实 stdio LSP；未保存缓冲区/导入的诊断、hover、definition、references、completion 共用编译输入；LLVM IR 可在目标发射前导出 |
| 接受但忽略的功能 | arbitrary extern ABI、错误方法、错误 `?`、错误类型/静态项不再静默通过；重复 extern 合并效果，不能擦掉 pure 限制。自举声明撤下，实验源码保留 |

关键结构决定：语义流、效果推导、常量/初始化、枚举检查分别独立模块；前端与后端消费共同类型/常量/内存指令事实。数组、枚举和 Unit 不借用指针假装值；只有 ABI 边界执行寄存器/间接传递转换。低层原始指针、C FFI、inline asm 与 x86 端口指令保留。

## 真实验收

主机 macOS / Apple Silicon，16 GiB；Cargo 并发3，同一时间只有一个 Cargo 构建。以下记录同时含检查、编译、运行及错误路径，不能简单相加当作独立语言特性数量。

| 入口 | 实际结果 |
| --- | --- |
| `scripts/test.sh llvm` | 完整入口通过：120单元，原有84个原生组合，driver14、entry1、tooling3，相对构建路径4；原始起点为42单元 |
| `scripts/validate.py` | 22组真实负载：计算、持续服务和 `.ath`，LLVM/C及规定优化等级比较 |
| `tests/aggregate_abi.py` | 49条检查/构建/运行记录，含双向 C ABI、数组复制/返回、浮点转换、packed、ASan/UBSan |
| `tests/system_values.py` | 最终54条记录：全局/union/布局、Unit、typed volatile、>2^32布局/静态地址；大类型只查询或导出IR，不分配大对象 |
| `tests/semantic_values.py` | 54条记录：合法借用/初始化/NEVER/Unit match四种后端优化组合运行；15类非法程序检查诊断代码、位置及两后端拒绝 |
| `tests/runtime_foundation.py` | 6组测试；文件/进程/文本和服务 stdout、flush、close 的失败注入，核对首次错误及清理 |
| `tests/generic_runtime.py` | 28项运行＋4项拒绝；容器、payload match、Result、文件错误和分配失败；另有4组生成C的ASan验收 |
| `tests/memory_runtime.py --check-x86` | 8项原生运行＋6类拒绝＋2份优化LLVM IR检查＋2个x86对象；特权端口指令只编译，未执行 |
| C-only独立构建 | 116单元、C原生矩阵、driver14/entry1/tooling3通过；独立release运行计算与零大小泛型，otool确认没有LLVM依赖 |
| JXZ integration | LLVM/C各15项，包括配置、argv、失败退出、嵌套依赖表、改名后的旧产物和元数据写入失败 |

计算程序 `examples/upgrade/compute.aeth` 在 AetherLang 内实现32768元素堆排序、二分查找、伪随机序列、整数与浮点归约，输出：

```text
compute 17218136811 46 1048510 33332.833335
```

持续服务 `examples/upgrade/accumulator.aeth` 在一个进程内处理 add/get/reset/quit，验证2000次连续更新、非法输入、EOF、journal及输出错误；状态机和资源清理由 AetherLang 完成。并发程序由 C 夹具只负责创建/join线程，AetherLang 执行4线程×100000次原子累加、锁保护计数及 release/acquire 数据发布。测试夹具没有接管语言核心算法。

最终release另独立重跑计算/服务22组合、基础库6组、泛型28运行+4拒绝、memory完整18项及JXZ各15，全部通过；记录了发布编译器哈希，未使用旧产物。

最后定向审查补齐 Unit match 的 phi 前驱、64位数组布局和静态GEP索引；分别重跑关联语义/系统套件，未反复执行已闭合的全部测试。ASan/UBSan仅证明所运行路径，没有宣称全面泄漏检测或完整内存安全证明。

跨目标：内部聚合、Unit泛型、x86端口指令已在x86_64 Linux/Darwin完成6个对象编译；之前的标量程序另有Linux aarch64与Windows x86_64对象证据。没有目标运行环境的组合未链接/运行。CI已接统一入口，但远端CI未执行。

## 实测性能与开销

使用同一 `examples/upgrade/benchmark.aeth`，两百万步整数计算，所有运行必须输出`652496`。同机顺序执行，预热后各5次；耗时为中位数、RSS为生成阶段最大值。原版以Rust1.99构建，最终版以Rust1.100构建，因此是交付版本比较，不隔离单一代码改动。测量时本任务没有并行构建或验收。

| 指标 | LLVM 原始起点 → 最终版 | C 原始起点 → 最终版 |
| --- | --- | --- |
| O0生成耗时（不含链接） | 16.504 ms → 23.736 ms | 9.574 ms → 8.452 ms |
| 生成阶段最大RSS | 29,999,104 B → 29,720,576 B | 3,080,192 B → 4,521,984 B |
| 生成文件大小（对象/C源码） | 1,096 B → 1,176 B | 1,476 B → 3,123 B |
| O0运行耗时 | 16.604 ms → 18.691 ms | 25.347 ms → 37.197 ms |
| O2运行耗时 | 15.432 ms → 13.328 ms | 13.819 ms → 13.444 ms |
| O0程序大小 | 33,480 B → 33,496 B | 33,480 B → 33,560 B |
| O2程序大小 | 33,480 B → 33,496 B | 33,480 B → 33,496 B |
| 编译器文件大小 | 94,813,504 B → 3,753,464 B | 2,411,552 B → 3,636,344 B |

此短样本LLVM O2运行约快13.6%，但生成耗时约增加43.8%、O0运行约增加12.6%；C O0运行约增加46.7%，O2变化约2.7%。不能据此宣称普遍提速。当前提升主要是正确性、真实能力和工具链；编译检查更充分也有成本。

LLVM编译器文件大幅缩小主要来自共享LLVM，不能等同总安装体积下降。C-only文件变大且RSS增加。样本包含进程启动开销，5次采样没有给出总体性能置信区间。完整采样、clang版本、编译器哈希及链接时间保存在上述JSON。

复测：`python3 scripts/benchmark.py --backend llvm --baseline "$WORK/baseline/aethc-llvm" --current "$WORK/release/aethc" --work-dir "$WORK/benchmarks/repeat-llvm"`，其中`WORK=/Volumes/WINDOWS/work/aetherlang-upgrade-work`；C使用`--backend c`、`baseline/aethc`与`release/aethc-c`。


## 构建、测试与产物

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

本机 CARGO_HOME 使用已验证的离线依赖缓存，避开其他项目的全局Cargo锁；其他机器可使用自己的常规缓存。`build.sh c`/`test.sh c`选择C-only，工具链脚本处理Homebrew遮蔽并打印实际版本。

最终文件在 `/Volumes/WINDOWS/work/aetherlang-upgrade-work/release/`：`aethc`、`aethc-c`、`jxz`、`jxz-c`及`manifest.json`。LLVM版仍依赖本机共享LLVM；编译器文件体积不代表总安装体积。全部文件的源码提交和SHA-256由manifest记录。

证据根为 `/Volumes/WINDOWS/work/aetherlang-upgrade-work`：完整验收 `evidence/closure-full-llvm.log`；最后修复 `closure-final-corrections.log`、`closure-final-layout.log`；发布验收 `followup-release/`；性能 `benchmarks/final-{llvm,c}/measurements.json`；跨目标 `aggregate-fix/cross-objects/results.json`。中期问题原始证据保留于 `followup-analysis/`，当时编译器冻结为 `baseline/014f0a1-aethc`，不再冒充最终release。

## 迁移与已知边界

- 浮点到整数转换采用上述饱和规则；数组是值，不能再依赖旧复制别名。`()`是size0/align1的值，和ABI void区分。
- 使用 `Vec<T>` 和 `vec_*`、`option_*`、`result_*`自由函数；旧的 `Vec<T,A>`/generic impl接口原本没有完整实现。`Option`来自独立option模块，`Result::Ok`需要足够的错误类型上下文。
- 未标效果的语言函数推导效果；`pure`及显式effect列表为上界。未知extern默认保守效果，可用真实`@reads/@writes/@io/@allocs/@panics/@pure`契约声明；重复声明不削弱已知效果。内联汇编和union读取需要unsafe，asm out允许初始化尚未赋值的绑定。
- 原子默认seq_cst；named-order接口或`_explicit`＋ORDER_*常量选择合法顺序。`compiler_barrier`兼容名称目前提供更强的全fence。端口函数从`mmio`迁到`x86io`；raw atomic指针须指向存活、自然对齐的存储，并采用兼容并发访问方式。
- 普通函数使用内部符号/调用约定，C可见定义写`#[no_mangle]`；对象输出用`--emit-obj`，LLVM文本用`--emit-llvm`，附加对象/库用`--link-object`和`--link-library`。

当前可以编写并实际运行：原生计算和数据结构程序、有状态输入服务、本地项目工具、直接C接口及受测试系统原语。它仍没有自动析构、完整资源所有权/trait/closure/async/const-generic体系；Vec、文件句柄等raw资源由调用者维持单一所有者并显式释放，复制资源容器不会自动深拷贝。未验证的旧标准库模块不算交付能力。

LLVM foreign aggregate C ABI目前实现并验证Apple arm64，其他目标明确拒绝该外部ABI；内部聚合和C后端不受同一限制。JXZ依赖图/锁文件/注册表与Windows进程层未实现，相关配置不假成功。`src_aether`仍存在旧API及占位生成，未完成自举。

下一轮最值得投入的是可解释的资源所有权与析构协议（先防止拥有资源的容器被无意复制，再统一作用域退出清理），然后扩展并验证其他目标的外部聚合ABI、JXZ依赖图。当前复核中的已证实错误及本轮选定的完整能力已逐项闭合；这不等同于声称语言不存在其他缺陷，也不决定终端、Shell或其他项目应迁移到它。
