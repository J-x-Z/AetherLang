# AetherLang 工程交接

2026-11-14升级快照已交付；2026-11-15追加审计发现新的未修问题，当前不能视为稳定发布。仓库 `/Volumes/WINDOWS/work/AetherLang`，分支 `work/aetherlang-upgrade`，只做本地提交，未推送。原始起点 `aceff1522790df3edfe5c711db5884350ab1b44a`；最终功能源码 `0709b338b2582906f02f12a2558ab16b783ad086`；主编译器源码 `a017d18`。2026-11-14交付由原外置release/manifest.json标识；最新开发版源码为`cd2569b`，新版本包见末尾，当前Git HEAD可用`git rev-parse HEAD`查询。

所有资产在 `/Volumes/WINDOWS/work/aetherlang-upgrade-work`（下称 WORK）。已有源码与历史证据保留；没有清理其他项目或修改全局Rust配置。远端NVIDIA验收任务目录 `/tmp/aetherlang-cudaq-20261114` 已取证并删除，未关闭设备或修改全局SDK。

## 有效构建与测试

```sh
cd /Volumes/WINDOWS/work/AetherLang
export RUSTUP_HOME=/Volumes/WINDOWS/work/aetherlang-upgrade-work/rustup
export CARGO_HOME=/Volumes/WINDOWS/work/aetherlang-upgrade-work/cargo-home
export CARGO_TARGET_DIR=/Volumes/WINDOWS/work/aetherlang-upgrade-work/target
export TMPDIR=/Volumes/WINDOWS/work/aetherlang-upgrade-work/tmp
export WORK=/Volumes/WINDOWS/work/aetherlang-upgrade-work
export LLVM_SYS_211_PREFIX=/opt/homebrew/opt/llvm@21
export CARGO_BUILD_JOBS=3 RUST_TEST_THREADS=3
sh scripts/build.sh llvm
sh scripts/test.sh llvm
```

Rust/Cargo1.100.0、LLVM21.1.8、AppleClang21；`c`选择C-only。一次只做一个Cargo构建。本机CARGO_HOME为外置离线缓存，其他机器可使用正常缓存。最后一次Cargo为LLVM+C，当前target/debug与target/release均支持LLVM/C。原冻结WORK/release保持不变；最新版本在WORK/releases/literal-escapes-20261115，带独立manifest与stdlib快照。

自举：`python3 tests/bootstrap_core.py --compiler "$WORK/release/aethc-c" --work-dir "$WORK/bootstrap-repeat" --backend c`。完整链会真实执行自编译、工作负载与sanitizer；不是统计可编译模块。

可选CUDA-Q CPU：设置 `AETHER_CUDAQ_BRIDGE=$WORK/cudaq/libaether_cudaq.dylib` 后运行统一测试。GPU库另外设置 `AETHER_CUDAQ_GPU=1`。SDK构建与CUDA原生验收见docs/QUANTUM.md，仓库不自动安装大型工具链。

## 2026-11-14已完成与历史有效证据

- 语义/IR/ABI：数组和聚合值、Result/union原始字节、借用初始化、NEVER、递归引用固定点、嵌套模式、完整i64/u64、LTR索引、foreign imports、Linux PIC；低层volatile/atomics/SIMD/errno保留并贯通。
- 系统：fd/进程/多段pipe错误与清理闭合；process88，JXZ最终两后端各15；计算/持续服务最终release22组。
- 数学：统一tensor/matrix/autodiff，视图存储引用计数、矩阵与反向AD；23条，12参数有限差分最大5.46e-12，240步损失1.98e-13。
- 自举：七模块77824B；stage0→1→2→3→4，生成C三代同SHA `7e718ccc2b9586620b3058a3bfc0e3621939861d7cac37b671c27fdb3fa31995`；最终aethc-core70856B。LLVM种子及C-only release种子均103check+83semantic×2。旧占位源码/旧bootstrap binaries已从当前树删除，Git历史保留。
- CUDA：原始CABI+可失败buffer/stream/cuBLAShandle；NVIDIA真矩阵程序C/LLVM O0/O2通过，不以shim冒充GPU。
- 量子：Aether电路/CPU模拟/参数移位；native CUDA-Q在MacCPU及LinuxRTX3060真GPU均通过八组混合电路+40步优化，energy -0.999999575958。Metal/Adreno未实现。
- 最新用户约束：8GiB统一内存优先、16GiB扩展，16比特首个跨设备规模。已落实QuantumPool/Lease，字节/状态数准入、失败回滚和关闭后保活；16qGHZ C/LLVM O0/O2、sanitizer、SDK CPU通过。预算只计主机振幅（示例2MiB/2state），不含SDK/元数据/总RSS。实测16q参考RSS3866624B、SDKCPU60325888B；未声称8GiB手机或GPU池路径已验。
- LLVM单元126、C-only122；完整入口分项证据在evidence/foundations-full-{llvm,rest}.log；最后PIC与release定向复验，不把首次fixture括号错误导致的中断当作单次全套成功。
- 发布目录WORK/release：aethc、aethc-c、aethc-core、jxz、jxz-c、manifest.json。详细报告docs/UPGRADE.md。性能5samples在benchmarks/foundations-{llvm,c}/measurements.json。
- 关键证据：WORK/foundations-release/、WORK/target/*/results.json、WORK/quantum-budget-validation/results.json、WORK/cudaq/final/gpu-validation.json、WORK/cudaq/final/remote-cleanup.json。早期证据仍保留，不能误当最终构建结果。

## 边界与下一项具体任务

Core自举是明确子集，不是全Rust/LLVM编译器自举。当前资源owner仍可被raw复制，需要调用者维持单一所有权并显式释放；QuantumPool为串行准入机制，不是异构调度器。LLVM外部聚合CABI只验证Applearm64；其他目标明确拒绝。JXZ依赖图/注册表和Windows进程层未实现。数学不是完整JAX/JIT/vmap；CUDA绑定不是AetherGPU kernel编译器。

## 2026-11-15多代理审计与当前任务

本次用户要求收敛“成为正常语言”的全部任务，并增加子代理深审。三名子代理覆盖前端/自举、IR/后端、标准库/JXZ，主代理覆盖安装、输出失败、LSP和健壮性。审计冻结提交 `a6cc268977129a5e69c57de501e04586fabc6060`；使用原release二进制，不修改编译器或运行库、不重建Cargo、不重跑全量套件。当前HEAD可用 `git rev-parse HEAD` 查询；后续文档提交不是新的编译器发布，release/manifest.json继续标识旧快照。

**审计已完成；新发现尚未修复。** `WORK/audit-normal-language/index.json`归并29组复现缺陷/能力不一致；frontend、backend、runtime、bootstrap、integration子目录保留命令和结果，前置探针在WORK/task-planning。源码风险与实际故障分开；其他部分未发现新故障不代表穷尽验证。

最先处理：f32定型丢失、Self方法与add名字特判；LLVM窄整数C ABI；Script/Core内部名称捕获；pure/契约规则；Vec对齐；主编译器8KiB表达式崩溃；JXZ错误吞掉/产物事务。后续还有标准库消费者、安装stdlib、输出保护、LSP身份与引用、Core u64 cast等。完整主线任务、依赖和验收条件见根目录 `MAINLINE_TASKS.md`，不要继续从旧FEATURES或历史路线图推断支持情况。

下一项具体任务：以B2/F05/B3为一组，让已定型表达式、已解析方法签名和callee成为IR的唯一输入事实，加入两后端O0/O2/O3回归；可独立并行修B1窄整数ABI与I05深表达式崩溃。之后实施资源owner规则与清理、标准库/项目工具，再完成真实程序和自举完整复验。无需重新全面审计；复用现有探针。

基础阶段范围已收敛：先修语言，不立即扩展统一异构张量引擎、Metal/Adreno、稀疏/大模型/PDE框架。用户随后要求把这些方向作为最终收束前的正式后段任务，已纳入MAINLINE_TASKS.md的T21–T28，不能视为取消或整条主线已完成。保留现有系统、CPU数学及CUDA/量子接口，按受影响范围回归。8GiB/16GiB和16q作为后续异构工作的约束保留。最小Core自举要持续成立，不把整个Rust/LLVM编译器重写成Aether作为本阶段条件。未来Shell用小型命令执行负载验证基础能力，不在本轮实现完整产品。

## 2026-11-15 JXZ项目原生输入接口

Shell提出的独立项目接口缺口已实现：`[build] module_paths` / `link_objects` 支持有序quoted路径数组、跨行/注释/尾逗号，build/run/test共用argv；相对路径按当前Jxz.toml项目目录解释，不做shell/env/glob展开，`-`/`@`开头规范化为`./`。build/run保留编译器退出码，test保留聚合失败1。现有native对象由项目提供，JXZ不编译C桥接源码。

版本为 `jxz 0.3.0-dev.1`。冻结aethc与stdlib均未修改。LLVM/C构建各19项集成通过，含真实跨模块C对象执行42、精确argv和32个错误配置子例；3项ASan/UBSan定向通过。首轮新增fixture漏写`test_`前缀导致4个test子例失败，修正后复验通过；初始失败证据保留，未削弱测试。证据 `WORK/jxz-native-inputs/results.json` 与各integration日志；旧版新增能力基线失败在 `WORK/jxz-build-arrays/baseline/new-test-baseline.json`。

独立版本目录 `WORK/releases/jxz-0.3.0-dev.1/` 保存jxz、jxz-c与manifest（源码提交、二进制SHA、冻结编译器及stdlib身份、验证命令）。原WORK/release不覆盖。此接口只完成主线项目工具任务的一部分，旧审计的JXZ测试目录权限/产物事务问题仍未闭合；没有修改Shell源码或更新其pin。

## 主线任务唯一入口与追加阻塞

用户要求的主线文档是根目录 `MAINLINE_TASKS.md`。原20项保持T01–T20及详细验收，T21–T28为此前暂缓扩张的后段正式任务，T29是整体最终收束；旧ROADMAP仅保留导航。JXZ数组接口记为T13/T18的已完成子项，整项尚未完成。

Shell追加的N01已由本线复现：`Node { children: Vec<Node> }` 的data元素仍是空fields占位Node，合法引用转*Node报E2100。最小源码和双方receipt保存在 `WORK/language-blockers/recursive-value/`，挂T02/T04，未修复。Shell保持其已验pin和应用绕过；本线不修改Shell源码。下一项实现仍为统一类型/方法/callee事实，并把此递归泛型占位回填加入回归。

## Shell平台反馈：N02与Linux宿主包

N02已独立复现：本机macOS27.2、SDK27.0，冻结aethc LLVM native对象LC_BUILD_VERSION minos28.0，最终binary为27.0；显式aarch64-apple-macosx27.0对象与binary均为27.0。直接Clang链接返回0但警告object更新；aethc native_compile成功路径丢弃stderr。源码native codegen读取LLVMGetDefaultTargetTriple，native link让C驱动自行选目标，两者没有共享effective target。证据 `WORK/language-blockers/darwin-native-target/results.json`；本次只编译/链接/查对象元数据，没有运行程序、没有修复。T07/T18已补统一目标与deployment/SDK政策、Mach-O回归及链接诊断验收。

Shell报告当前冻结LLVM-O2主执行链已运行；这属于Shell线程报告，本线未独立重跑其产品。Shell将显式部署目标作为临时adapter策略，继续保持pin。语言侧稳定后需提供Linux x86_64可运行aethc/JXZ宿主包、匹配stdlib和manifest/SHA256，并在真实Linux验收后供Shell联验；已列T18/T28。Linux对象或已有GPU应用运行不能替代冻结宿主工具链交付。本次没有下载工具链、创建Linux发布包或改动Shell。

## 2026-11-15 N03字面量链路已修复

源码提交 `cd2569b698eea33710a55c2a0701923e4007c049`：Core/Script共享Rust转义解码规则，自举Core同步；未知/畸形escape拒绝，`\xHH`为ASCII00..7F，`\u{1..6 hex}`为Unicode scalar且字符串UTF-8编码。修复EOF扫描越界、Script词法错误列位置、C输出NUL后吞0..7数字；字符可显式转整数，常量/运行窄化一致，逆向未校验转换仍拒绝。规范和迁移见docs/LANGUAGE.md 1.4及docs/MIGRATION.md；Core旧高位hex原始字节需迁移为numeric u8或Unicode。

验证：134单元、14driver、3tooling；C/LLVM O0/O2/O3与Core共220条字面量检查；最终新seed完整stage0→1→2→3→4、103检查、98语义用例加原生字节执行分别在normal/sanitizer模式通过；生成C三代SHA一致为 `eaefb458043767b5d3559e13feb8a160bedb9f4f91d5ce990e31789ba776b813`。22组真实负载、JXZ两后端各19项通过。Core没有新增char语法，仍是明确子集。本次没有重跑全工程所有套件/GPU，也没有Shell GUI联验。

主证据 `WORK/literal-escapes/verification-final.json`、matrix-final/results.json、bootstrap-final/results.json。旧版错误接受unknown escape及C字节错误在baseline/results.json。首次运行矩阵发现char as u32被拒绝后进行了实际语义修复；初次214条中6个构建失败与一次Rust类型修正前日志均保留，不冒充首轮全通过。

新独立冻结包 `WORK/releases/literal-escapes-20261115/`：aethc（LLVM+C）、aethc-core、jxz、jxz-c、原版本匹配stdlib与manifest。主编译器SHA256 `8e67c244d52a6f04536504f4a8df7888d9ed02570c25329061c3479c630c008b`。使用其stdlib时显式-I/AETHER_PATH；可搬移查找I01仍未修。原release及JXZ独立包未覆盖；Shell pin未改。当前仅Mac arm64宿主包，LLVM21动态库依赖记录在manifest；Linux宿主包任务仍在T18。

主线T01/T02/T16/T18增加已完成子项，没有将整项勾完；N01递归泛型与N02deployment偏差、29组基线问题仍待处理。下一项仍是B2/F05/B3/N01的已定型事实和类型身份传递，不重启全面审计。


## Shell首阶段交付回执已归档

Shell源码提交 `bfe01906da145b68ac434c3286b5480f9a68866e`，Glass验收索引指向 `a076daccd3f0b21d8dd9da6135812fa1ca72fdcb`；Shell可执行文件SHA256 `0585df506f1a1d0b77c832baf5374c3833698f987217f9cbceec43491204cc8e`。当前Shell继续pin原aethc b4d0d036、JXZ d8504047和stdlib5f84a3f，并使用既有adapter/绕过，没有采用24ceae3的新编译器。

本线只读核对Shell HEAD、实际binary hash、lock中的编译器/JXZ/44个stdlib文件及原release manifest hash；9份receipt逐项与验收索引SHA匹配，快照归档至 `WORK/shell-acceptance/20261115-bfe0190/`。原结果包括51 CLI、27编辑PTY、12真实vi/job-control、两套Terminal PTY和GUI；完整单元及部分IME/Unicode结果按Shell交接复用前一候选，不能都说成最终hash重跑。本线没有重新启动GUI/PTY测试。

T17/T28记为部分完成：已有旧pin的macOS产品验收；新工具链聚焦联验/显式更新lock、Linux x86_64宿主aethc/JXZ包及真实Linux验收仍未完成。原固定发布继续保留。当前没有新增阻塞macOS核心的语言接口需求，N01/N02等已知语言问题仍按主线修复，不把应用绕过当作闭合。
