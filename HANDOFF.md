# AetherLang 工程交接

本轮交付已完成。仓库 `/Volumes/WINDOWS/work/AetherLang`，分支 `work/aetherlang-upgrade`，只做本地提交，未推送。原始起点 `aceff1522790df3edfe5c711db5884350ab1b44a`；最终功能源码 `0709b338b2582906f02f12a2558ab16b783ad086`；主编译器源码 `a017d18`。当前交付提交用 `git rev-parse HEAD` 或外置 `release/manifest.json` 查询，后续文档提交不改变构建源码。

所有资产在 `/Volumes/WINDOWS/work/aetherlang-upgrade-work`（下称 WORK）。已有源码与历史证据保留；没有清理其他项目或修改全局Rust配置。远端NVIDIA验收任务目录 `/tmp/aetherlang-cudaq-20261114` 已取证并删除，未关闭设备或修改全局SDK。

## 有效构建与测试

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

Rust/Cargo1.100.0、LLVM21.1.8、AppleClang21；`c`选择C-only。一次只做一个Cargo构建。本机CARGO_HOME为外置离线缓存，其他机器可使用正常缓存。最后一次Cargo是C-only，因此当前target/debug与target/release中的aethc是C-only；冻结的WORK/release/aethc为LLVM+C，aethc-c为C-only。

自举：`python3 tests/bootstrap_core.py --compiler "$WORK/release/aethc-c" --work-dir "$WORK/bootstrap-repeat" --backend c`。完整链会真实执行自编译、工作负载与sanitizer；不是统计可编译模块。

可选CUDA-Q CPU：设置 `AETHER_CUDAQ_BRIDGE=$WORK/cudaq/libaether_cudaq.dylib` 后运行统一测试。GPU库另外设置 `AETHER_CUDAQ_GPU=1`。SDK构建与CUDA原生验收见docs/QUANTUM.md，仓库不自动安装大型工具链。

## 已完成与有效证据

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

下一项：先设计资源owner不可隐式复制与作用域退出清理，覆盖Vec/fd/Tensor/QuantumLease、提前return和Result?；用现有失败注入测试作为验收。异构路线随后为同一16q电路实现一个真实Metal或Vulkan/Adreno后端，查询精度/内存/队列能力，测量误差/峰值内存/同步，再扩展调度器。不要重新全面审计已闭合模块，不要把“支持CUDA-Q”说成“支持Metal”。不要替用户决定Shell或其他项目必须迁移。
