# AetherLang 能力与验收入口

两种源文件进入相同的语义检查和 IR。下表记录本地主线至 `d31ea10` 的实测能力。可安装的双宿主对照包固定于 `423e664`。任务状态见[主线任务](../MAINLINE_TASKS.md)。

| 能力 | 语法与检查 | 生成、链接和执行 | 可复现入口 |
| --- | --- | --- | --- |
| Script 与 Core 源码 | `.ath` 支持缩进、函数、条件、计数循环、基础数值和字符串；`.aeth` 使用显式局部类型 | 同一程序用 C/LLVM 编译并运行。Script 诊断映回原行列 | `scripts/validate.py`、`tests/tooling.rs` |
| 标量、聚合与泛型 | 目标位宽整数、f32/f64、具名结构、字段、数组、非空元组、类型与整数/bool 常量泛型、重复数组初值单次求值、枚举有效载荷和 `?` | 两后端 O0/O2；值、数值及 ABI 定向比较 O3 | `tests/backend_regressions.rs`、`tests/{aggregate_abi,generic_runtime}.py` |
| 模块和方法 | 导入、别名、私有成员、静态 interface/impl，检查完整签名和效果界限。泛型函数体按实例检查 | 重名模块、多包依赖和递归 Vec 有实际程序 | `tests/interfaces_methods.py`、`jxz/tests/integration.py` |
| 生命周期与内存 | 跟踪初始化、引用来源和借用。opt-in `own Record` 对绑定的移动、使用和分支归并有检查 | 泛型析构在返回、循环及 `?` 早退时释放拥有的 fd、Vec、TensorHandle 和 LeaseHandle。裸指针仍由使用者保证有效性 | `tests/owned_values.rs`、`tests/{vec_alignment,memory_runtime,process_runtime}.py` |
| 效果与契约 | 检查纯谓词、显式效果的上界、`requires`、`ensures` 及 `result` | 入口及正常出口执行检查，包括通过 `?` 返回；失败触发 trap | `tests/contracts.py` |
| C 互调与系统入口 | C 函数/变量、`#[no_mangle]` 导出、union、布局查询、typed volatile、原子操作、受限 interrupt/naked | Apple arm64 和 Linux x86_64 SysV 双向 C ABI，含混合寄存器与栈传参。wasm32 通过实际执行核实目标整数和 Vec。x86-64 裸机处理函数在 QEMU 中处理两种 CPU 帧 | `tests/{global_linkage,aggregate_abi,sysv_abi,vec32,freestanding}.py` |
| 数据与宿主库 | 带长度字节、严格 UTF-8、i64 字节键 Map、Option/Result 以及带 errno 的 POSIX 文件/目录/进程接口 | 文件中的 NUL、无效 UTF-8、短读、错误码、子进程和分配失败均有原生用例 | `tests/{text_map,runtime_foundation,process_runtime}.py` |
| 数学与量子 | CPU f64 张量/反向 AD、标量 DAG、电路与状态向量，OpenBLAS、Metal、CUDA-Q 的接口各自独立 | 真实 CPU native JIT/vmap、低秩、稀疏、PDE、MaxCut；M4 Metal、RTX 3060 和 Adreno 750 Vulkan 运行上层程序与 16q，CUDA-Q QPP 有独立 SDK 验收 | `tests/{math_values,compute_jit,compute_planning,compute_applications,compute_runtime,cudaq_runtime}.py` |
| 项目、编辑器与安装 | JXZ 本地依赖、精确版本注册表、锁定/离线缓存、项目级 LSP、可校验的整包安装 | 目录移动、子进程退出、并发和中断恢复均有回归 | `jxz/tests/integration.py`、`tests/package_install.py` |
| 自举 | 13 模块 Core 读取真实模块图，在 Aether 中执行解析、检查、C 和直接 LLVM IR 输出 | C-only 与 LLVM 种子分别构建四代，分别核对 C 和 LLVM 固定点，编译未改的 accumulator 服务和标准库消费者。主编译器由 Rust 构建 | `tests/{bootstrap_core,bootstrap_llvm}.py` |

在此提交上，动态 trait、捕获闭包和 async 仍在 T25 实施。源级常量泛型已经可以在两种语法下运行。元组不能按值穿越 C ABI（E2404），嵌套聚合不接收 owner（E2405）；不支持的目标聚合 ABI 在检查阶段报告 E2407。

公开基础库按实际消费者分组。字节、文本、持有键的 Map 使用 `tests/text_map.py`，Vec、Option 和 Result 使用 `tests/{vec_alignment,vec32,generic_runtime}.py`；作用域包装见 `tests/owned_allocations.py`。文件、io、path、fd、process 与二进制接口由 `tests/{runtime_foundation,process_runtime}.py` 和 JXZ 验证；分配器、TOML 子集、标量图见 `tests/legacy_runtime.py`。LLVM/JNI/OpenBLAS 与硬件 SDK 各有宿主或设备依赖，见 `tests/ffi_modules.py` 及升级报告。仓库里的早期生成 `.c`、smoke/test 文件不作为公开标准库接口。

各平台测量和版本变更见[升级报告](UPGRADE.md)。
