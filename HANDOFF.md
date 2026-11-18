# AetherLang 工程交接

功能源码冻结于 `13941f6730247ab7961011bdbd9de9033d1ec0a8`。工作树在 `/Volumes/WINDOWS/work/AetherLang`，分支为 `work/aetherlang-upgrade`。原始起点是 `aceff15`，本次续做起点是 `4cb768c`。用 `git log -1 --oneline` 查看包含交付文档的当前提交。仓库保持私有，提交保存在本地。[主线任务](MAINLINE_TASKS.md) · [升级报告及测量](docs/UPGRADE.md)

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
| Windows x86-64 | `$WORK/windows-delivery/aether-windows-x64-472e6fd-20261118.zip` | 同目录 `final-receipt-472e6fd.json` 记录交叉编译、链接、PE 和 ZIP 校验 |

常规编译器自举覆盖两种源表示及 C/LLVM；生成的编译器已实际编译 JXZ、独立 Shell、持久服务与科学计算程序。CSV 回归使用精确有理数核验。16 比特程序在 CPU、M4 Metal、RTX 3060 和 Adreno 750 上有实际执行记录，设备取证分别在 Mac 的 compute 日志、Linux 证据包与 `$WORK/adreno-ordered-marginal-94d6348`。最后一项编译器回归及析构验证在 `$WORK/t26-method-receiver/manifest.json`。

## 下一项

按用户指定，在 Windows 上机前停下。下一次在 Windows x64 解压 ZIP、核对随包 SHA-256，使用 Python 3.10+ 和匹配 UCRT x64 的 llvm-mingw Clang 运行包内入口，保留 `native-results` 中的回执。

```powershell
.\Test-Native.ps1 -Clang 'C:\llvm-mingw\bin\clang.exe' -Python python -WorkDir '.\native-results'
```

Windows 原生验收后，再由维护者决定公开和推送时间。后续科学工具最直接的缺口是数据框互操作、绘图和交互式分析；语言本身仍有独立于这些工作流的系统编程入口。
