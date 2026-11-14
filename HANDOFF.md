# AetherLang 当前交接 — 2026-11-14

- 源码：`/Volumes/WINDOWS/work/AetherLang`；分支 `work/aetherlang-upgrade`；没有推送远端。
- 起点：`aceff1522790df3edfe5c711db5884350ab1b44a`（干净 checkout）。不要重新克隆、重做审计或覆盖当前工作。
- 已验证编译器源码：`11d1b4a`；之后 `2614ad0` 仅修复脚本相对路径并完善基准入口，附四组回归。最新 HEAD 用 `git log -1 --oneline` 查看；后续文档提交不改变编译器产物。
- 提交顺序：`47e3ce7` JXZ → `753f644` 编译器语义/IR/后端 → `8084ad9` LLVM 并发初始化 → `11d1b4a` 工具链/负载 → `2614ad0` 脚本修复。
- 全部构建、工具链和证据：`/Volumes/WINDOWS/work/aetherlang-upgrade-work`，约 3.0 GiB。保持本任务 8 GiB 内，Cargo 并发 3，一次一个全量构建；复用 LLVM，不清理别的项目。

## 有效构建/测试命令

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

C-only 使用参数 `c`。脚本会选外置 Rust 1.100.0 并打印 cargo/rustc 版本。**不要只依赖 `rustup run ... cargo`**：本机 Homebrew cargo/rustc 1.99.0 曾遮蔽实际工具链；已通过 PATH 固定解决。全局默认 Rust 未改。

## 已完成及验收

- 统一模块/类型/常量事实；词法、返回、赋值、可变性和错误检查；有类型的 IR 地址/值、短路、循环、结构体 ABI、变参 FFI、整数规则、LLVM 优化和并发初始化。
- LLVM 为主，正常链接 executable；C 备用。`.ath` 实际进入相同检查/生成流程。手动指针、FFI 和 inline asm 保留。
- JXZ 初始化/配置/构建/argv/测试/清理实际可用；不支持的依赖功能报错。
- 81 单元、84 原生组合、14 驱动 + 1 入口、22 真实工作负载组合通过；JXZ LLVM/C 各 11/11。最终 release 重跑负载及两套 JXZ 均通过。
- README 四组原生示例与构建目录四组回归通过。C 生成的计算/服务经 ASan+UBSan、JXZ 经 ASan 无报告；不等于完整泄漏/所有权证明。
- 跨 Linux x86_64/aarch64 和 Windows x86_64 对象生成成功，未链接/运行。远端 CI 尚未运行。
- 完整最终日志 `evidence/final-rust1100-validation.log`；发布验收 `evidence/release-*.log`；测量 `benchmarks/{llvm,c}/measurements.json`；动态检查 `sanitizers/results.json`；跨目标 `cross/results.json`。
- 发布文件 `release/aethc`、`aethc-c`、`jxz`、`jxz-c`；`release/manifest.json` 记录 SHA-256。LLVM 编译器运行需要本机共享 LLVM 21.1.8。
- 基线源码和原编译器保存在 `baseline/`。用户提到的 4.7 GB 归档仅做目录清单检查，未发现 AetherLang/.aeth，不曾批量解压。
- 性能与完整边界见 `docs/UPGRADE.md`。单样本 LLVM O2 较快，但生成对象和 O0 较慢；不能宣布普遍提速。

## 原定下一项具体任务（被下方复核调整）

实现真正的泛型实例化及带载荷 enum，端到端运行 `Vec<i64>` 和 `Result<i64, i32>`，再整理相应标准库。不要把语法解析成功当作生成/运行成功。

仍未完成：高级泛型/trait/闭包、完整所有权/析构、完整标准库、自举、JXZ 依赖/注册表/Windows 进程层、`.ath` 完整源映射。`src_aether/main.aeth` 仍有占位生成，不可宣称自举成功。不要在下轮重新全面审计已闭合的项目。

## 升级后针对性复核 — 最新优先级

基于 `90f4769` 的 release 复核发现：数组在 IR 中退化为指针，造成复制别名、返回悬垂及 repr(C) 布局错误；Core 未初始化读取/局部引用逃逸被接受，ASan 已确认栈返回后访问；另有浮点转整数未定义边界、extern 聚合返回失败、静态/union 静默忽略及应用错误路径问题。

**下一项应先修聚合值/布局/ABI，以及 BindingId + 控制流上的确定赋值/引用生命周期，再推进泛型/Result。** 细节、根因、验收条件见 `docs/FOLLOWUP_ANALYSIS.md`；所有复现与原始日志在外置工作目录 `followup-analysis/`。本次只修改分析与交接文档，未声称这些新增问题已经修复。
