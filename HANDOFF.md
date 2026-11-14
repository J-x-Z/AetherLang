# AetherLang 工程交接

2026-11-14升级快照已交付；2026-11-15追加审计发现新的未修问题，当前不能视为稳定发布。仓库 `/Volumes/WINDOWS/work/AetherLang`，分支 `work/aetherlang-upgrade`，只做本地提交，未推送。原始起点 `aceff1522790df3edfe5c711db5884350ab1b44a`；最终功能源码 `0709b338b2582906f02f12a2558ab16b783ad086`；主编译器源码 `a017d18`。当前交付提交用 `git rev-parse HEAD` 或外置 `release/manifest.json` 查询，后续文档提交不改变构建源码。

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

Rust/Cargo1.100.0、LLVM21.1.8、AppleClang21；`c`选择C-only。一次只做一个Cargo构建。本机CARGO_HOME为外置离线缓存，其他机器可使用正常缓存。最后一次Cargo是C-only，因此当前target/debug与target/release中的aethc是C-only；冻结的WORK/release/aethc为LLVM+C，aethc-c为C-only。

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

最先处理：f32定型丢失、Self方法与add名字特判；LLVM窄整数C ABI；Script/Core内部名称捕获；pure/契约规则；Vec对齐；主编译器8KiB表达式崩溃；JXZ错误吞掉/产物事务。后续还有标准库消费者、安装stdlib、输出保护、LSP身份与引用、Core u64 cast等。完整20项任务、依赖和验收条件见 `docs/PROJECT_ROADMAP.md`，不要继续从旧FEATURES或历史路线图推断支持情况。

下一项具体任务：以B2/F05/B3为一组，让已定型表达式、已解析方法签名和callee成为IR的唯一输入事实，加入两后端O0/O2/O3回归；可独立并行修B1窄整数ABI与I05深表达式崩溃。之后实施资源owner规则与清理、标准库/项目工具，再完成真实程序和自举完整复验。无需重新全面审计；复用现有探针。

范围已收敛：本阶段不再扩展统一异构张量引擎、Metal/Adreno、稀疏/大模型/PDE框架。保留现有系统、CPU数学及CUDA/量子接口，按受影响范围回归。8GiB/16GiB和16q作为后续异构工作的约束保留。最小Core自举要持续成立，不把整个Rust/LLVM编译器重写成Aether作为本阶段条件。未来Shell用小型命令执行负载验证基础能力，不在本轮实现完整产品。
