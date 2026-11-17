# AetherLang 能力与验收入口

两种源文件进入相同的语义检查和 IR。下表对应 `c5ed0b6` 的 macOS arm64 基础开发包。更晚的主线实现和目标见[主线任务](../MAINLINE_TASKS.md)。

| 能力 | 语法与检查 | 生成、链接和执行 | 可复现入口 |
| --- | --- | --- | --- |
| Script 与 Core 源码 | `.ath` 支持缩进、函数、条件、计数循环、基础数值和字符串；`.aeth` 使用显式局部类型 | 同一程序用 C/LLVM 编译并运行。Script 诊断映回原行列 | `scripts/validate.py`、`tests/tooling.rs` |
| 标量、聚合与泛型 | 目标位宽整数、f32/f64、具名结构、字段、数组、非空元组、泛型实例化、枚举有效载荷和 `?` | 两后端 O0/O2；值、数值及 ABI 定向比较 O3 | `tests/backend_regressions.rs`、`tests/{aggregate_abi,generic_runtime}.py` |
| 模块和方法 | 导入、别名、私有成员、静态 interface/impl，检查完整签名和效果界限。泛型函数体按实例检查 | 重名模块、多包依赖和递归 Vec 有实际程序 | `tests/interfaces_methods.py`、`jxz/tests/integration.py` |
| 生命周期与内存 | 跟踪初始化、引用来源和借用。opt-in `own Record` 对绑定的移动、使用和分支归并有检查 | 基础包显式释放 fd、向量、张量和量子 lease。裸指针仍由使用者保证有效性 | `tests/owned_values.rs`、`tests/{vec_alignment,memory_runtime,process_runtime}.py` |
| 效果与契约 | 检查纯谓词、显式效果的上界、`requires`、`ensures` 及 `result` | 入口及正常出口执行检查，包括通过 `?` 返回；失败触发 trap | `tests/contracts.py` |
| C 互调与系统入口 | C 函数/变量、`#[no_mangle]` 导出、union、布局查询、typed volatile、原子操作、受限 interrupt/naked | Apple arm64 双向 C ABI。wasm32 通过实际执行核实目标整数和 Vec。x86-64 裸机处理函数在 QEMU 中处理两种 CPU 帧 | `tests/{global_linkage,aggregate_abi,vec32,freestanding}.py` |
| 数据与宿主库 | 带长度字节、严格 UTF-8、i64 字节键 Map、Option/Result 以及 POSIX 文件/进程接口 | 文件中的 NUL、无效 UTF-8、短读、错误码、子进程和分配失败均有原生用例 | `tests/{text_map,runtime_foundation,process_runtime}.py` |
| 数学与量子 | CPU f64 张量/反向 AD、标量 DAG、电路与状态向量，OpenBLAS、Metal、CUDA-Q 的接口各自独立 | Mac 原生 OpenBLAS、Metal 通用计算和 CUDA-Q CPU 有实测；目标设备及其他版本见升级报告 | `tests/{math_values,quantum_values,ffi_modules,cudaq_runtime}.py` |
| 项目、编辑器与安装 | JXZ 本地路径依赖、项目级 LSP、可校验的整包安装 | 目录移动、子进程退出、并发和中断恢复均有回归 | `jxz/tests/integration.py`、`tests/package_install.py` |
| 自举 | 七模块 Core 的词法、解析、类型检查、C 输出用 Aether 实现 | C-only 与 LLVM 种子分别构建四代，检验生成的 C 固定点并执行消毒器及真实程序。主编译器由 Rust 构建 | `tests/bootstrap_core.py` |

基础包对闭包/函数值报 E2403，对按值穿越 C ABI 的元组报 E2404。动态 trait、const 泛型、async 和主编译器全量迁移保留在 T25–T26。各平台测量与原始结果见[升级报告](UPGRADE.md)。
