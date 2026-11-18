# AetherLang 工程交接

macOS/Linux 已验收宿主包冻结于 `13941f6`。Windows 11 x64 原生验收包冻结于 `98402df`。裸机栈约定修复于 `d22a1d9`，最新内核用例及构建入口是 `c7ab3ab`。工作树在 `/Volumes/WINDOWS/work/AetherLang`，分支为 `work/aetherlang-upgrade`。原始起点是 `aceff15`，本次续做起点是 `4cb768c`。用 `git log -1 --oneline` 查看包含交付文档的当前提交。仓库保持私有，提交保存在本地。[主线任务](MAINLINE_TASKS.md) · [升级报告及测量](docs/UPGRADE.md)

## 重建

```sh
cd /Volumes/WINDOWS/work/AetherLang
. /Volumes/WINDOWS/work/aetherlang-upgrade-work/typed-facts-20261117/env.sh
sh scripts/build.sh llvm
python3 scripts/build_selfhost.py --seed "$CARGO_TARGET_DIR/release/aethc" --work-dir "$WORK/regular-work" --output "$WORK/aethc-native"
AETHER_TEST_QEMU=1 AETHER_TEST_METAL=1 OPENBLAS_LIBRARY=/opt/homebrew/opt/openblas/lib/libopenblas.dylib AETHER_CUDAQ_BRIDGE="$WORK/cudaq/libaether_cudaq.dylib" sh scripts/test.sh llvm
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

## 裸机入口与下一项

`d22a1d9` 的 macOS Rust 种子已放在 `$WORK/kernel-readback-20261118/aethc-527387d`。从仓库根目录加载前述环境，然后运行：

```sh
python3 tests/freestanding.py --compiler "$WORK/kernel-readback-20261118/aethc-527387d" --backends c llvm --qemu --work-dir "$WORK/kernel-recheck"
```

中断入口复验在 `$WORK/kernel-readback-20261118/final/results.json`。用自举 C 后端构建可启动的页帧池镜像并与 Rust 种子 C/LLVM 对照：

```sh
python3 tests/kernel_boot.py --selfhost "$WORK/releases/aetherlang-macos-472e6fd/aethc-native" \
  --seed "$WORK/kernel-readback-20261118/aethc-527387d" --work-dir "$WORK/kernel-pages-recheck" --qemu
```

仅验证自举 C 路线时可省略 `--seed`。`$WORK/kernel-pages-20261118/verified-8293b4a.json` 记录 O0/O2 六个页帧池镜像和四个原有中断镜像的 QEMU 结果。自举编译器从 Aether 发射 C，Clang 进行 x86-64 交叉编译。直接 LLVM 交叉编译使用 Rust 种子。两路的栈和寄存器设置见[升级报告](docs/UPGRADE.md#aether-写内核的实际边界)。

若继续内核基础，下一项是把独立的周期定时器接到普通 Aether 工作循环，在 QEMU 里检验持续运行和中断返回。真实固件、板上设备或实际中断延迟进入验收时再安排硬件。公开发布和远端推送仍待维护者决定。
