# AetherLang 集中工程升级交付记录

日期：2026-11-14。仓库：`J-x-Z/AetherLang`。本地分支：`work/aetherlang-upgrade`。
起点：`aceff1522790df3edfe5c711db5884350ab1b44a`，从远端取得的干净源码。
本轮未推送远端。源码、工具链、构建产物和证据均放在 `/Volumes/WINDOWS/work`。

## 实际能力变化

本轮交付的是能够构建、链接、运行真实程序的 Rust 编译器版本。LLVM 是主后端，C 保留为不依赖 LLVM 的备用实现和语义比较手段。`.ath` 保留面向人的简洁入口，`.aeth` 保留显式类型、指针、FFI 和内联汇编；两者进入同一条检查与生成链。

| 范围 | 起点实际情况 | 本轮完成的结果 |
| --- | --- | --- |
| 语义与模块 | 调用、赋值等检查不完整；导入可能只有声明而无函数体 | 统一模块展开与类型检查；递归导入、公共导出、别名、私有辅助函数和菱形依赖完整进入生成流程 |
| 控制流 | 逻辑操作两边求值，循环退出及 LLVM phi 有缺失/占位 | 短路、break/continue、提前返回、分支合流在两后端及 O0/O2/O3 一致 |
| 内存与数值 | 地址和值混淆，索引宽度、结构体返回及整数行为不一致 | 有类型的 IR 寄存器和可寻址位置；结构值复制、指针返回、规定的整数语义和真实 FFI |
| LLVM 工具链 | 源码已依赖 LLVM 21，但配置、CLI 链接及优化流程不一致 | 复用本机 LLVM 21.1.8，默认 LLVM，正常链接可执行文件，执行对应优化流水线并校验 IR |
| `.ath` | 多处仅有语法入口或替代输出 | 算术、逻辑、带类型函数、分支、可变局部量和范围循环端到端运行，检查未初始化变量 |
| JXZ | 活跃入口自身存在解析失败，构建/配置/进程链不可靠 | 用 AetherLang 实现项目初始化、配置、构建、带参数运行、测试汇总和限定范围清理 |
| 标准库与应用 | 旧标准库与编译器规则脱节，声明不代表可用 | 增加有边界的整数解析，完成计算程序与持续处理输入的服务；没有把旧标准库整体标为可用 |
| 自举 | 完整入口无法生成工作编译器，源码有占位 main | 明确撤下“已自举”结论，保留实验源码；本轮未完成自举 |

## 关键修改与对应验证

1. **统一跨层规则。** `frontend/resolve.rs` 在语义分析和 IR 前完成相对导入、模块搜索路径、别名和私有名称处理；循环导入、缺失/非公开符号明确报错。前向类型、递归指针类型可用，无限大小的值递归与循环别名拒绝。`semantic_constants.rs` 将解析后的类型别名和带类型标量常量交给 IR，FFI 声明按实际类型比较。回归覆盖导入、别名、常量及重复声明。
2. **拒绝错误程序。** 词法错误、溢出字面量、未结束字符串/注释不再静默变成零或成功；UTF-8 使用字节跨度。调用参数、返回路径、赋值目标、可变性、字段、运算和循环控制共用检查规则。负例测试验证诊断，未删除失败用例来取得通过。
3. **修复地址、控制流与 ABI。** 左值和索引只求值一次，真实元素宽度贯穿代码生成；结构值复制/返回和结构指针返回分开处理。LLVM 固定栈槽置于入口块，避免每次循环重新扩张栈。短路、循环跳转、phi 前驱、变参提升、UTF-8/NUL 字符串和内联汇编输入输出均有原生执行回归。
4. **给整数行为明确语义。** 定宽加、减、乘、取负按补码回绕；移位计数按位宽掩码；有符号右移扩展符号。除零及 signed MIN / -1（包括余数）运行时陷阱。编译期常量运算采取溢出/非法操作诊断。浮点比较覆盖 NaN、负零和无穷，不开启 fast-math。测试按这些规则断言，并要求应陷阱的程序确实异常终止。
5. **修复真实驱动链。** LLVM `build` 产生可执行文件；对象输出显式使用 `--emit-obj`。`--emit-ir -o`、优化级别、`CC`/`--cc`、`-I`/`AETHER_PATH` 生效。保留子编译器/链接器诊断。临时文件唯一、权限受限并自动清理；保护根源码和所有导入源码，包含规范路径与硬链接，防止输出覆盖输入。
6. **清除伪实现。** 删除 LLVM 哈希常量/占位函数及不成立的泛型、闭包等降级方式；无法生成的特性明确失败。`check` 通过只表示前端检查通过，不能替代后端构建验收。
7. **补齐 JXZ 的本地工作流。** 配置和进程逻辑在 `.aeth` 中实现，通过 POSIX fork/execvp/waitpid 保留 argv、退出码和信号状态，不拼接 shell 命令。测试失败非零退出，不运行旧产物；clean 只处理记录的输出，不递归删除用户目录或旁边的 `.c`。尚未实现的依赖/注册表操作明确失败。
8. **稳定构建环境。** Rust 升到外置的 1.100.0，提交锁文件与工具链版本；脚本解决 Homebrew cargo/rustc 遮蔽 rustup 的问题，实际打印并验证两者版本。LLVM 目标注册改为全进程一次初始化，修复并发测试挂起，增加八线程回归。相对构建/临时路径规范化为绝对路径，附独立脚本回归。CI 不再用忽略失败的命令掩盖错误。

## 真实程序和测试结果

验收主机为 macOS / Apple Silicon，16 GiB 内存；Cargo 并发 3，未并行进行多个全量构建。完整最终测试的实际工具链为 Rust/Cargo 1.100.0、LLVM 21.1.8、Apple clang 21.0.0。

| 验证入口 | 实际结果 |
| --- | --- |
| Rust 单元测试 | 81 通过；起点为 42 通过 |
| 原生语义矩阵 | 13 程序 × 2 后端 × O0/O2/O3 = 78 项；其中 12 项要求陷阱并通过 |
| 本机内联汇编 | 两后端 × 三优化级别 = 6 项通过；与上项合计 84 项 |
| 驱动与入口集成 | 14 项驱动测试 + 1 项入口测试通过 |
| 实际工作负载 | 22 组构建/运行通过：6 组计算、4 组持续服务、12 组 `.ath` |
| JXZ | LLVM 和 C 各 11/11；调试测试入口与最终 release 产物均验证 |
| README 示例 | `.ath` / `.aeth` × 两后端，共 4 组实际运行通过 |
| 相对构建目录 | build/test × 默认/显式相对 TMPDIR，共 4 组回归通过 |
| 动态检查 | 计算与持续服务生成 C 经 ASan + UBSan 运行无报告；JXZ 的 ASan 集成测试 11/11 |

`examples/upgrade/compute.aeth` 的核心逻辑是 AetherLang：分配数组、生成 32768 个伪随机数、堆排序、二分查找、整数归约和浮点计算。结果为：

```text
compute 17218136811 46 1048510 33332.833335
```

持续服务 `examples/upgrade/accumulator.aeth` 保持一个进程处理 add/get/reset/quit，维护状态、校验整数与输入长度、写 journal，并显式关闭文件与释放内存。每组验证 2000 次连续更新，另含错误命令、数值边界、过长输入、EOF 和文件打开失败；响应与 journal 内容均断言。测试脚本负责提供输入和检查结果，算法与服务状态机不转交给 Python/C。

ASan/UBSan 结果只覆盖被执行的路径，不等同于完整所有权证明或全面泄漏检测。原生执行矩阵同时覆盖 LLVM，动态检查所用的是生成的 C。

跨目标仅完成基准程序对象生成及格式检查：x86_64 Linux、aarch64 Linux 为 ELF；x86_64 Windows 为 COFF。未配置目标 SDK/runtime，三者均未链接或运行。修改后的远端 CI 尚未运行，不能作为其他平台验收结果。

## 实测开销与性能

使用 `examples/upgrade/benchmark.aeth`，执行 2,000,000 步整数计算，原版与新版都必须输出 `652496`。相同主机上顺序运行，预热后采样 5 次，表中耗时为中位数；RSS 为生成阶段采样最大值。生成与 clang 链接分开计时。起点编译器由原提交源码以 Rust 1.99.0 构建，新版以 Rust 1.100.0 构建，所以这是交付版本比较，不是隔离单一代码改动的实验。

| 指标 | LLVM 起点 → 新版 | C 起点 → 新版 |
| --- | --- | --- |
| O0 生成耗时（不含链接） | 14.438 → 17.923 ms | 6.408 → 5.498 ms |
| 编译器生成阶段最大 RSS | 29,966,336 → 28,934,144 B | 3,080,192 → 3,440,640 B |
| O0 生成文件大小 | 对象 1096 → 1176 B | C 源码 1476 → 2078 B |
| O0 程序运行耗时 | 16.027 → 18.261 ms | 24.787 → 29.336 ms |
| O2 程序运行耗时 | 16.077 → 14.537 ms | 13.609 → 13.948 ms |
| 程序可执行文件大小（O0/O2 相同） | 33,480 → 33,496 B | 33,480 → 33,496 B |
| 编译器文件大小 | 94,813,504 → 2,925,864 B | 2,411,552 → 2,868,696 B |

LLVM 编译器文件大幅缩小主要因为从静态链接改成共享 LLVM；运行时仍依赖 `/opt/homebrew/opt/llvm@21/lib/libLLVM.dylib`，**不能视为总安装体积同比例降低**。C-only 编译器无需 LLVM。

LLVM 此样本的 O2 时间约缩短 9.6%，但生成对象变慢、O0 也变慢，C 的 O2 没有明显改善。样本较短，含进程启动成本，五次采样不支持推广为普遍性能提升。原版 LLVM 未按级别接通完整 IR 优化流水线，新版已接通；C 的比较使用相同 clang O0/O2 参数，避免旧驱动漏传参数导致虚假提速。没有用缓存状态不同的整工程构建时间做速度结论。

原始数据：外置工作目录下 `benchmarks/{llvm,c}/measurements.json`。其中保留原始采样、产物大小、编译器 SHA-256 与构建版本来源；当时 shell 中的 rustc 版本单列为 ambient 信息，不冒充编译器构建版本。

## 可复现入口与交付位置

本机完整命令（从任何目录起）：

```sh
cd /Volumes/WINDOWS/work/AetherLang
export RUSTUP_HOME=/Volumes/WINDOWS/work/aetherlang-upgrade-work/rustup
export CARGO_TARGET_DIR=/Volumes/WINDOWS/work/aetherlang-upgrade-work/target
export TMPDIR=/Volumes/WINDOWS/work/aetherlang-upgrade-work/tmp
export LLVM_SYS_211_PREFIX=/opt/homebrew/opt/llvm@21
export CARGO_BUILD_JOBS=3
export RUST_TEST_THREADS=3
sh scripts/build.sh llvm
sh scripts/test.sh llvm
```

`sh scripts/build.sh c` / `sh scripts/test.sh c` 选择 C-only；同一个 Cargo target 的最终 `aethc` 会随所选特性改变，因此已额外保存两份最终 release 编译器。其他主机安装对应工具链后设置自己的路径，不要照抄 Homebrew 路径。

已构建文件位于 `/Volumes/WINDOWS/work/aetherlang-upgrade-work/release/`：

- `aethc`：主 LLVM 编译器，也包含 C 后端。
- `aethc-c`：不依赖 LLVM 的 C-only 编译器。
- `jxz`、`jxz-c`：由上述两种路径生成并各自验证的项目工具。
- `workloads/`：真实程序产物及结果；`manifest.json` 记录构建版本、散列与证据位置。

测量复现示例（`c` 替换为 `llvm` 时，同时把 baseline 改为 `baseline/aethc-llvm`，current 改为 `release/aethc`）：

```sh
python3 scripts/benchmark.py --backend c \
  --baseline /Volumes/WINDOWS/work/aetherlang-upgrade-work/baseline/aethc \
  --current /Volumes/WINDOWS/work/aetherlang-upgrade-work/release/aethc-c \
  --work-dir /Volumes/WINDOWS/work/aetherlang-upgrade-work/benchmarks/repeat-c
```

主要证据相对 `/Volumes/WINDOWS/work/aetherlang-upgrade-work`：

- `evidence/final-rust1100-validation.log`：真实 Rust 1.100.0 完整验收。
- `evidence/release-workloads.log`、`evidence/release-jxz-{llvm,c}.log`：最终 release 程序验收。
- `sanitizers/results.json`、`jxz/integration-asan.log`：动态检查范围与结果。
- `cross/results.json`：跨目标对象、链接和运行状态分别记录。
- `evidence/representative/results.json`：旧标准库/自举代表性源码的阶段性探测，不作为最终版本全部能力清单。

逻辑提交：`47e3ce7`（JXZ），`753f644`（统一检查与原生语义），`8084ad9`（LLVM 并发初始化），`11d1b4a`（工具链和工作负载），`2614ad0`（外置路径与测量入口）；之后的提交只整理交付说明。最终编译器源码对应 `11d1b4a`，后续未改编译器实现。全部本地保存，工作树最终应干净。源码和本轮工作目录合计约 3.0 GiB，复用了现有 LLVM/缓存，未清理其他项目，也未改全局 Rust 默认版本。

## 不兼容变更与迁移

- 分号丢弃表达式值；非 void 函数必须在每条可达路径返回。赋值需可变绑定；有类型整数变量缩窄需显式转换，不能依赖旧代码静默放行。
- 根 `main` 未标返回类型时按语句式入口处理：落到底部/裸 return 返回 0，显式 `return N` 决定状态，尾表达式被丢弃。需要尾表达式作为退出码时写 `fn main() -> i32`。显式入口接受无参数或 `(argc: i32, argv: **u8)`，`no_main` 留给独立链接场景。
- LLVM `build` 默认输出可执行文件；要旧的对象输出行为使用 `--emit-obj`。
- 普通语言函数使用内部链接名，避免模块和 libc 冲突；明确导出 ABI 符号使用 `#[no_mangle]`。
- `.ath` 参数与非 main 的返回值函数需要类型标注。列表、对象成员、`@comptime` 等未实现部分现在明确拒绝。
- 泛型、带载荷枚举、闭包等旧的“能解析”或占位生成不等同于可用。真实实例化尚未完成，调用无法生成的功能会失败。
- JXZ 默认 debug/O0，`--release` 选择 O2；配置为文档所列的 TOML 子集，非空依赖表与未实现操作不再假成功。新示例和 JXZ 活跃实现已随规则更新。

## 已知边界和下一步

当前适合实际编写已验证特性范围内的原生计算程序、有状态输入服务和本地项目工具；可直接使用手动内存管理、C ABI 和本机内联汇编。不是完整成熟的标准库生态或具备完整内存安全保证的系统语言。

- 泛型实例化、带载荷 enum/Option/Result、trait、闭包及高级容器还缺完整实现；旧 stdlib 的许多文件仍与当前规则不兼容。
- 指针和 FFI 生命周期由程序负责；无完整借用/所有权检查或自动析构。本轮修复实际生命周期错误并验证应用，不宣称解决整个内存模型。
- JXZ 的进程层和持续服务当前为 64 位 POSIX；注册表、依赖解析和 Windows 进程层未完成。
- `.ath` 一部分后续诊断仍显示转换后的 Core 上下文，完整源映射仍需补齐。
- 自举编译器缺少完整 AST 到可执行代码链，不能以模块数或旧生成文件声称自举成功。

下一项具体工程任务：基于现有共享类型事实实现真正的泛型实例化与带载荷枚举布局，选择 `Vec<i64>` 和 `Result<i64, i32>` 的创建、访问、返回及错误传播作端到端验收，再统一相应标准库。保持两后端语义矩阵，随后扩展资源管理和诊断源映射。本轮不据此决定终端、Shell 或其他项目是否迁移。
