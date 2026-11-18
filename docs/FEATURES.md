# AetherLang 能力与验收入口

两种源文件进入相同的语义检查和 IR。下表记录本地主线基础冻结至 `f84e9d4` 的实测能力。双宿主基础包也固定于 `f84e9d4`，Shell 联验包固定于 `423e664`。任务状态见[主线任务](../MAINLINE_TASKS.md)。

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
| 自举 | Core 读取真实模块图，在 Aether 中执行解析、检查、C 和直接 LLVM IR 输出 | C-only 与 LLVM 种子分别构建四代，分别核对 C 和 LLVM 固定点，原生构建、对象输出及 JXZ 精确依赖映射；运行未改的 accumulator 服务和标准库消费者。主编译器由 Rust 构建 | `tests/{bootstrap_core,bootstrap_llvm}.py` |

### 稳定基础语义

整数运行时加、减、乘依目标位宽回绕，移位量按位宽取模；整除以零和有符号最小值除以 −1 产生 trap。浮点值按其声明精度计算，显式转换为整数时截断并饱和，NaN 转为零。短路运算仅在需要时求右值。函数实参从左至右求值，复合赋值先确定目标和旧值，再求一次右值。未初始化读取、借用局部值越过有效期和隐式复制 owner 均有检查。裸指针程序自行维护地址的有效性。Result 的 `?` 在 Err 路径提前返回，并清理已经获得的拥有资源。模块名按作用域解析，重名局部优先于导出的全局名。相关正反例见 `tests/{backend_regressions.rs,semantic_values.py,owned_cleanup.rs,aggregate_abi.py,interfaces_methods.py}`。

目标相关符号在选定目标下检查。例如元组按值穿越 C ABI 报 E2404，嵌套聚合承载 owner 报 E2405，不支持的目标聚合 ABI 报 E2407。`a18bc69` 补上 errno 的目标检查，wasm32 使用无宿主 libc 的 errno 入口会报 E2407，原生调用约定或符号种类不符报 E2202。覆盖 Windows 与 Android 等交叉目标的检查或对象生成各有独立记录，见 `tests/platform_intrinsics.py`。

### 高级语义增量

`451e2f8` 的捕获闭包支持静态调用与组合，`66ccafc` 支持常量泛型。闭世界借用接口 `&dyn` / `&mut dyn` 在 `e08aa2a` 运行时分派，不透明标签与有效载荷留在 Aether 内。`0d06551` 的具体 async 任务持有帧，显式 poll 能得到 Pending、Ready、Finished，取消会释放活跃子任务及资源。当前只支持函数顶层的挂起点，以及非递归、非泛型、无跨挂起借用的帧。带 owner 的局部移动跨越后续挂起点受当前检查限制。运行例与拒绝用例见 `tests/{closure_values,dynamic_values,async_runtime,array_repeat}.py`。

公开基础库按实际消费者分组。字节、文本、持有键的 Map 使用 `tests/text_map.py`，Vec、Option 和 Result 使用 `tests/{vec_alignment,vec32,generic_runtime}.py`；作用域包装见 `tests/owned_allocations.py`。文件、io、path、fd、process 与二进制接口由 `tests/{runtime_foundation,process_runtime}.py` 和 JXZ 验证；分配器、TOML 子集、标量图见 `tests/legacy_runtime.py`。LLVM/JNI/OpenBLAS 与硬件 SDK 各有宿主或设备依赖，见 `tests/ffi_modules.py` 及升级报告。仓库里的早期生成 `.c`、smoke/test 文件不作为公开标准库接口。

各平台测量和版本变更见[升级报告](UPGRADE.md)。
