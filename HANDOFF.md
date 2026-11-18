# AetherLang 工程交接

工作树 `/Volumes/WINDOWS/work/AetherLang`，分支 `work/aetherlang-upgrade`，原始起点 `aceff15`。本轮内核和量子从 `78f5ac5` 续做，实现及测试入口至 `4226137`。已验收的宿主安装包仍分别冻结在 macOS/Linux `13941f6`、Windows `98402df`。本地新增的实现、回执与剩余任务见下方；用 `git log -1 --oneline` 查看包含交接的当前提交。[主线任务](MAINLINE_TASKS.md) · [升级报告及测量](docs/UPGRADE.md)

## 重建

```sh
cd /Volumes/WINDOWS/work/AetherLang
. /Volumes/WINDOWS/work/aetherlang-upgrade-work/typed-facts-20261117/env.sh
sh scripts/build.sh llvm
python3 scripts/build_selfhost.py --seed "$CARGO_TARGET_DIR/release/aethc" --work-dir "$WORK/regular-work" --output "$WORK/aethc-native"
AETHER_TEST_QEMU=1 AETHER_TEST_METAL=1 OPENBLAS_LIBRARY=/opt/homebrew/opt/openblas/lib/libopenblas.dylib AETHER_CUDAQ_BRIDGE="$WORK/cudaq/quake-export-20261118/libaether_cudaq.dylib" sh scripts/test.sh llvm
```

本机使用外置 Rust 1.100.0、LLVM 21、Clang 和三路 Cargo 并发。`sh scripts/build.sh c` 构建 C-only 种子。`scripts/test.sh llvm seed` 与 `scripts/test.sh llvm selfhost` 可单独复现两组验收。从干净 Git 提交构建发行目录使用 `python3 scripts/package.py --work-dir "$WORK/rebuild-evidence" --output "$WORK/rebuild-release"`，输出应为新目录。Linux 附带 LLVM 共享库时加 `--bundle-linux-llvm`。

## 交付与证据

外置根为 `WORK=/Volumes/WINDOWS/work/aetherlang-upgrade-work`。

| 平台 | 包或入口 | 验收 |
| --- | --- | --- |
| macOS arm64 | `$WORK/releases/aetherlang-macos-472e6fd.tar.gz` | `$WORK/accept-472e6fd/{full.log,run.json,archive.json}` |
| Linux x86-64 | `$WORK/linux-mainline-20261118/delivered/472e6fd/linux-release-472e6fd.tgz` | 同目录 `final-verification.json` 和证据归档 |
| Windows x86-64 | `$WORK/windows-native-20261118/aether-windows-x64-117efa2.zip` | 同目录 `native-acceptance.json` 与 `evidence-117efa2.zip` 含真机自举、JXZ、CSV 与 LSP 记录 |

常规编译器自举覆盖两种源表示及 C/LLVM；生成的编译器已实际编译 JXZ、独立 Shell、持久服务与科学计算程序。CSV 回归使用精确有理数核验。16 比特程序在 CPU、M4 Metal、RTX 3060 和 Adreno 750 上有实际执行记录，设备取证分别在 Mac 的 compute 日志、Linux 证据包与 `$WORK/adreno-ordered-marginal-94d6348`。最后一项编译器回归及析构验证在 `$WORK/t26-method-receiver/manifest.json`。

## Windows 复验

在 NTFS 工作目录解压包，使用 x64 Python 3.10+ 和 llvm-mingw UCRT Clang。Windows 11 build 26200、Python 3.10.11 与 Clang 21.1.8 已跑通该入口。

```powershell
.\Test-Native.ps1 -Clang 'C:\Tools\llvm-mingw\bin\clang.exe' -Python python -WorkDir '.\native-results'
```

独立 GPU 包在 `$WORK/windows-gpu-20261118/AetherLang-Windows-x64-Vulkan.zip`。解压后 `python windows_vulkan.py run --dir . --vendor 10de` 重跑 RTX 4060 的 CPU/Vulkan 对照。CUDA-Q 的 SDK 设备探针在 `examples/quantum/cudaq_device.aeth`，设备结果见 [量子运行时](docs/QUANTUM.md)。

## 本轮源码和验收

`26378d9` 的周期时钟在持续运行的 Aether 工作函数内实际触发。常规自举编译器在 `7dcd8d8` 加入 x86-64 裸机 LLVM，并在 `8253c44` 修复优化 IR 的目标核对。修正版固定点及直接可用的工具位于 `$WORK/selfhost-metal-target/corrected`。QEMU 用例无需板卡。

```sh
trial="$WORK/llvm23-kernel-quantum-20261118"
native="$WORK/selfhost-metal-target/corrected/aethc-native"
python3 tests/kernel_boot.py --selfhost "$native" --seed "$CARGO_TARGET_DIR/release/aethc" --work-dir "$WORK/recheck-pages" --qemu
python3 tests/bootstrap_kernel_target.py --compiler "$native" --cc "$LLVM_SYS_211_PREFIX/bin/clang" --work-dir "$WORK/recheck-abi" --qemu
python3 tests/kernel_timer.py --seed "$CARGO_TARGET_DIR/release/aethc" --work-dir "$WORK/recheck-timer"
```

`25665bc` 提供显式 LLVM 23 选项。`a98f550` 修复了配套 Clang 检出的宿主 C 后端 FILE 句柄错误。稳定默认仍为 LLVM 21.1。对照使用的 23.1.2 已安装在 `/opt/homebrew/opt/llvm`；浮点文本 IR 由配套的 Clang 读取。两套环境使用独立产物目录。

```sh
LLVM_SYS_231_PREFIX=/opt/homebrew/opt/llvm CARGO_TARGET_DIR="$trial/opt-in-target" sh scripts/build.sh llvm23
LLVM_SYS_231_PREFIX=/opt/homebrew/opt/llvm CARGO_TARGET_DIR="$trial/opt-in-target" sh scripts/test.sh llvm23 seed
python3 scripts/benchmark_host.py --reference "$CARGO_TARGET_DIR/release/aethc" --candidate "$trial/opt-in-target/release/aethc" --work-dir "$WORK/recheck-host-perf" --samples 8
```

`acf62af` 增加通过 SDK 导出 Quake，`beb76e7` 和 `46ac232` 增加参数模板及 M4 GPU 验收。模板和绑定均在 Aether 中实现。Mac 上 CUDA-Q qpp 和 Metal 可以在同一验收中分别选用。

```sh
bridge="$WORK/cudaq/quake-export-20261118/libaether_cudaq.dylib"
python3 tests/quantum_templates.py --compiler "$native" --backends c llvm --bridge "$bridge" --metal --work-dir "$WORK/recheck-templates"
```

- 21/23 编译耗时、RSS、产物和运行对照在 `$trial/compare-host/{results.json,run.py,provenance.md}`，最终 release 对象的逐字节核对在 `$trial/final-benchmark-objects/results.json`。页帧池在 `$trial/{final21-pages,final23-pages}`，时钟在 `$WORK/kernel-timer-20261118` 与 `$trial/timer`。
- macOS 完整 LLVM 23 种子入口退出 0，日志为 `$trial/opt-in-final.log`。LLVM 21 构建与单元测试在 `$trial/final-default-{build,unit}.log`。
- 交叉 ABI 与错误输出保存在 `$WORK/selfhost-metal-target/corrected/{llvm21,llvm23}`。量子记录在 `$WORK/quantum-templates-20261118` 和 `$WORK/cudaq/quake-export-20261118`。
- 下一项内核工作是让自举编译器支持现有定时器的中断入口。量子下一项是导出保留参数的 Quake 函数并用同一模块执行多组参数。宿主大版本迁移下一步是使用确切拟发布的 LLVM 补丁版本，在 Linux/Windows 的现有原生入口重跑。公开发布和推送由维护者决定。
