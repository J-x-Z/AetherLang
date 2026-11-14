# 升级后实际问题复核 — 2026-11-14

检查基点 `90f4769`，使用已交付的 release/aethc，未重编整个编译器。针对性小程序覆盖 LLVM/C O0/O2，并对两个内存错误做 ASan 验证。本次未修改编译器或应用实现。结论：上轮建议直接推进泛型过早，应先补聚合值表示及控制流/存储分析。

所有最小复现、脚本与原始输出保存在 `/Volumes/WINDOWS/work/aetherlang-upgrade-work/followup-analysis/{backend,frontend,runtime,core}`。以下“已证实”仅指列出的具体案例；没有把未测路径当作正常。

## 第一优先：已证实的正确性缺陷

### 1. 固定数组被当成指针，是目前最值得深入修复的根问题

- `array_copy.aeth`：`b = a; b[0] = 99` 后两后端 O0/O2 均输出 `a=99 b=99`。固定数组的值复制语义没有成立。
- `array_return.aeth`：函数返回 `[i64;3]` 后，O0 读到后续函数使用的 90/91/92，O2 读到无关数值；生成 C 经 ASan 明确报告 stack-use-after-return。
- `struct_array.aeth`：`@repr(C)` 的 `{ data:[u8;16], tail:i64 }` 实际 tail 偏移 8，而 C 布局要求 16；数组内容也读错。
- `nested_array.aeth`：`[[i32;2];2]` 应读到 1/2/3/4，实际为 1/0/3/0。
- 根因：`src/middle/ir_gen.rs:1771`、`:1852` 将 Array 降成 Ptr；聚合复制路径 `:1424` 没有保留内联数组身份。两个后端得到相同的错误表示，因此差分测试会共同通过错误结果。
- 应补：真实 `Array(element, length)` IR 类型；区分值、存储位置和借用指针；数组/结构体统一复制与返回协议；按目标布局计算 size/align/offset。
- 验收：固定/嵌套数组、含数组结构体的复制/参数/返回；返回后覆盖栈；ASan；与独立 C `sizeof/offsetof` 及双向 FFI 调用对照。

### 2. Core 的初始化和引用生命周期检查没有贯通

- `fn main()->i32 { let value:i32; return value; }` 的 check 和两后端 O0/O2 构建均成功，实际读取未初始化存储。
- `fn dangling()->&i64 { let value:i64=42; return &value; }` 也全部通过；无 raw pointer/unsafe，调用后解引用在 C O0/O2 出现差异，ASan 明确报 stack-use-after-return。应归因于引用逃逸未检查，不能指责优化器改变合法程序。
- 内层借用结束后再次借用，或在不同作用域借用同名变量，反而报重复可变借用。
- `let unused:i32={return 42;};`、`loop{return 42;break;}` 被错误判为缺返回。
- 根因：`semantic.rs:1365/:1418` 缺确定赋值状态；`:1387/:1774` 只看引用类型而无来源/生命周期；`:150` 借用按字符串变量名记录，释放方法没有调用点；类型与可达控制流混在一起。
- 应补：每个声明有独立 BindingId；控制流图上的确定赋值/可达性分析；引用来源、保守词法借用范围及逃逸检查。保留原始指针的低层能力，明确其责任边界，不用禁止全部指针规避问题。
- 验收：分支部分赋值、循环零次执行、遮蔽、嵌套借用、返回局部引用、聚合内引用与所有提前退出路径。

### 3. 数值转换与 C ABI 还存在硬缺口

- `1e20 as i32`：两后端 O0 得到 2147483647，O2 本次得到 -69910144；`NaN as i64` 为 0 与本次 8520024448。当前直接 C cast/LLVM fptosi 没有处理越界与 NaN（`llvm_codegen.rs:954`、`c_codegen.rs:710`）。先定义转换规则，例如检查后陷阱或明确的饱和转换，再写共同测试，不能把本次任意输出定成语言语义。
- `extern "C" { fn make_pair()->Pair; }` 检查通过，但两后端 emit-obj 都失败，尚未链接：C 解引用结构值，LLVM 报 Load operand must be a pointer（`ir_gen.rs:912/:643`）。需要独立的目标 ABI lowering，区分内部聚合返回协议与 C ABI。
- `@repr(packed)` 在 LLVM 本机运行正确，但 C 输出的属性位置非法，构建失败（`c_codegen.rs:945`）。需修正并将布局/对齐纳入共同测试。

## 第二优先：封闭“接受但忽略”与应用错误路径

- `static VALUE:i32=true`、含 `MissingType` 字段的 union，check 与两后端 build 均成功；真正读取合法 static 又报未定义。`semantic.rs:1220`、`ir_gen.rs:347` 直接跳过了相关项。未实现时应明确拒绝，同时将 globals/union 作为系统能力逐步实现，不能静默丢弃。
- `extern "NOT_AN_ABI"` 也能编译并调用 puts。应验证 ABI 名称与目标支持，不把任意 ABI 标签当作 C。
- `S::nonexistent()` 以及 i64 函数内 `1?` 通过 check，随后生成失败。`semantic.rs:1475` 返回 Unknown，`:1511` 对 ? 返回原类型。前端不能用 Unknown 兜底掩盖不存在的方法或错误的 ? 操作数。
- pure 函数本来拒绝 puts；添加同样的 extern 声明后却通过并打印。`semantic.rs:964` 的空效果集覆盖已有元数据。应保留已知外部函数效果，定义其他 extern 的效果契约。
- `stdlib/core.aeth:22` 使用 `printf(s)`，普通文本 `literal %% text` 实际变成 `literal % text`。应使用固定格式或明确的字节写入接口。
- 持续服务向 stdout 写入失败后发送 quit，仍 exit 0；首次 flush 和 quit 路径未检查（`accumulator.aeth:35/:52`）。journal 写失败已能非零退出，但关闭错误会覆盖首次失败原因。需要统一输出/关闭检查并保存首次错误。
- JXZ 拒绝 `[dependencies]`，却接受 `[dependencies.missing]` 和不存在的 path 并成功构建（`config.aeth:147`）。应按配置结构一致识别不支持的依赖。包名变化后只记录最后一个产物，clean 留下旧自有产物（`build.aeth:95`），属于较低优先级的构建记录问题。

## 最值得补的能力及实施顺序

1. **先完成聚合值/布局/ABI和绑定/控制流两条修复。** 不需重写解析器或整个后端。引入带已解析类型和绑定身份的中间表示，让后端消费检查结果。当前 SemanticFacts 只传 aliases/constants，其余类型仍从 AST 重建，造成规则分叉。逐步替换这种重复推导；无效类型/未支持功能不允许落到后端的默认值。
2. **建立小而完整的标准库可用集合。** 优先文本/字节缓冲、字符串、文件、路径、参数、进程及一致错误返回，补齐错误注入与清理测试。parse_int/helper 已实际运行；string/io/fs/args 的当前第一阻断点是旧源码缺类型/可变标注，后续生成与运行仍需逐项验证，不能因此断言整个语言路线不可用，也不能只改标注便宣称库完成。
3. **在值表示稳定后做泛型、带载荷 enum、Option/Result。** 首批端到端目标是 Vec<i64>/Vec<u8>、Result<文件句柄,错误码>、分支处理与错误传播。避免同时扩展 trait、闭包、async 等所有方向。资源管理先完成可解释的显式关闭与作用域清理，再建立自动析构，保留直接 FFI/原始指针。
4. **补系统编程承诺。** globals、union、size/align/offset、volatile/atomic 必须进入真实 IR，而非名字猜测。当前 `*volatile T`、`@volatile` 均在解析处失败，IR Load/Store 也无访问限定信息，C 还按类型名包含 volatile 判断。需设计显式访问指令与内存顺序，不能把 MMIO/并发能力算作已实现。
5. **工具链优先机器可读诊断，而非新增 AI 专用语法。** `.ath` 源映射、稳定诊断编号、JSON 输出、跨文件符号/类型查询直接服务人和 AI。现有 LSP 的 diagnostics 永远空、hover/definition 为 None（`lsp/mod.rs:148`），无 CLI 服务入口；AI-IR 也未接入主编译路径。应复用同一检查结果，之后再接通 LSP。JXZ 先严谨处理本地 manifest/产物/退出码，再做依赖图、锁文件和增量构建。

下一批的完成条件应是：上述数组/引用/转换复现有明确语义且回归闭合，真实 C ABI 对照通过，基础 I/O 在失败路径不假成功。此后再把泛型/Result作为新增能力交付。

不建议当前投入：更换 LLVM、删除 C 后端、再次扩张语法、自举大重写、微基准提速。问题主要在传给后端的表示与规则；LLVM 保持主后端，C 继续承担备用与交叉验证。但要增加独立语义断言/ABI 对照，不能把“两者结果相同”当作正确性证据。
