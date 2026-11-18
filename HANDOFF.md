# AetherLang 工程交接

外置源码位于 `/Volumes/WINDOWS/work/AetherLang`，分支 `work/aetherlang-upgrade`。升级起点为 `aceff15`，三个宿主包的源码以经核验的 `v0.1.0` 标签和各包的 `source_commit` 为准。当前分支提交用 `git rev-parse HEAD` 查询。[主线任务](MAINLINE_TASKS.md) · [升级报告与实测指标](docs/UPGRADE.md)

本机日常终端可直接运行 `aethc`、`aethc-native`、`jxz` 和 `jxz-native`。工具链安装在外置卷 `/Volumes/WINDOWS/work/aetherlang-toolchain`。

## 当前本地开发版

MCP 与编译修复的源码提交为 `84c270b`，在公开 `v0.1.0` 之后。MCP 的本地入口为 `aether-mcp --root /Volumes/WINDOWS/work/AetherLang/jxz`。包内和项目均可直接启动 `tools/mcp/aether_mcp.py`。它支持检查、定义、引用和分页符号搜索。[配置示例](docs/MCP.md) · [审计及测量](docs/UPGRADE.md#010-之后的本地-mcp-与编译审计)

工具链已按清单 SHA-256 `4afd1f42c3e98ececa0c971b8b7019c18ad944fd49f774abd8566efd3180d0d7` 原子安装。命令中的版本号仍为 0.1.0，安装清单的 `release_id` 为 `aetherlang-mcp-dev-84c270b`。验收记录位于 `/Volumes/WINDOWS/work/aetherlang-upgrade-work/mcp-cycle`。载入下方“本机重建”的环境后，可运行这组检查。

```sh
cycle="$WORK/mcp-cycle"
prefix=/Volumes/WINDOWS/work/aetherlang-toolchain/bin
python3 -B tests/mcp_protocol.py
python3 -B tests/mcp_queries.py --compiler "$prefix/aethc" --jxz "$prefix/jxz" --work-dir "$cycle/recheck-seed"
python3 -B tests/mcp_queries.py --compiler "$prefix/aethc-native" --jxz "$prefix/jxz" --work-dir "$cycle/recheck-native"
```

Windows x64 开发包 `d7f119c` 已在 RTX 4060 笔记本上通过完整原生验收，覆盖双后端自举、LSP、JXZ、MCP 与 CSV。安装目录为 `D:\AetherLang`，NTFS 分配 2,630,896 字节。新终端可直接运行 `aethc`、`jxz` 和 `aether-mcp`。需要重跑时，在 PowerShell 中执行以下命令。详细记录保存在 `WORK/mcp-cycle/win4060/d7f119c0e7dbd09ed4593a0b05b81f4e8b04a835/native-receipts.zip`。下文多平台验收表对应已发布的 0.1.0。

```powershell
$python = Join-Path $env:LOCALAPPDATA 'Programs\Python\Python312\python.exe'
& D:\AetherLang\Test-Native.ps1 -Clang (Get-Command clang.exe).Source -Python $python -WorkDir D:\AetherLang-work\recheck
```

## 本机重建

```sh
cd /Volumes/WINDOWS/work/AetherLang
. /Volumes/WINDOWS/work/aetherlang-upgrade-work/typed-facts-20261117/env.sh
sh scripts/build.sh llvm
python3 scripts/build_selfhost.py --seed "$CARGO_TARGET_DIR/release/aethc" \
  --work-dir "$WORK/regular-work" --output "$WORK/aethc-native"
```

完整验收同时启用 QEMU、Metal、OpenBLAS 和已安装的 CUDA-Q SDK。可选 LLVM 23 使用独立的 Cargo 产物目录。发布包默认用 LLVM 21.1。普通宿主可从 [README](README.md) 的无可选设备命令开始。

```sh
cycle="$WORK/closure-20261118"
export AETHER_TEST_QEMU=1 AETHER_TEST_METAL=1
export OPENBLAS_LIBRARY=/opt/homebrew/opt/openblas/lib/libopenblas.dylib
export AETHER_CUDAQ_BRIDGE="$cycle/quake/libaether_cudaq.dylib"
export AETHER_CUDAQ_QUAKE_PYTHON="$cycle/sdk-python"
sh scripts/test.sh llvm
LLVM_SYS_231_PREFIX=/opt/homebrew/opt/llvm CARGO_TARGET_DIR="$WORK/llvm23-kernel-quantum-20261118/opt-in-target" sh scripts/test.sh llvm23 selfhost
python3 scripts/package.py --work-dir "$WORK/rebuild-evidence" --output "$WORK/rebuild-release"
```

包输出应为新目录。Linux 打包可加 `--bundle-linux-llvm`。Windows 从解包根运行 `Test-Native.ps1`，传入 x64 Python 和 llvm-mingw UCRT Clang 的路径。

## 当前验收凭据

原始执行记录使用当时的目录名。提交身份重写的逐提交对照保存在外置卷 `closure-20261118/identity/commit-map.json`。下列路径相对 `/Volumes/WINDOWS/work/aetherlang-upgrade-work`。原始日志留在外置卷，程序、输入和测试入口随仓库交付。

| 平台或能力 | 已执行的工作 | 本地凭据 |
| --- | --- | --- |
| macOS arm64 | 稳定版完整入口、可选版本自举、安装和 SDK 模板的分步状态 | `closure-20261118/accept/6dc584cc1e/acceptance.json` |
| Linux x86-64 | 双种子、安装树、JXZ、独立 Shell、QEMU、CPU/Vulkan 实机及 236 项自举异步检查 | `closure-20261118/identity/verified/6dc584c-linux-evidence/{final-release,async-verified}.json` |
| Windows x86-64 | 发行 ZIP 在原生 Windows 上完成自举、JXZ、LSP、CSV 与 64 KiB 数组；维护者 RTX 3060 实机另有早一轮 GPU 验收 | `closure-20261118/identity/verified/6dc584c-windows/hosted-summary.json`、`closure-20261118/platforms/a24134e/windows/final-verification.json` |
| Arch Linux ARM | Xiaomi Warsaw 实机，板上自举固定点、JXZ、张量微分与 Adreno 830 Vulkan | `closure-20261118/arm64-linux/receipt.json` |
| x86-64 裸机 | 种子和自举生成的周期中断程序，LLVM 21/23 的 O0/O2 共八次 QEMU 启动 | `closure-20261118/quake/kernel-independent-review.json` |
| 参数化量子程序 | CUDA-Q SDK 通过同一 Quake 模块执行五组角度 | `closure-20261118/quake/parameterized-final.json` |

16 比特状态向量结果及 SDK 和原生计算路径见[量子运行时](docs/QUANTUM.md)。维护者已暂缓物理 8 GiB 整机余量测试。

## 发行与下一项

同版发行源码为 `6dc584cc1ef88ed37a1d60d3c5722f65be8d2b0f`。三份归档、`SHA256SUMS` 和 FileVeil 签名 TOML 位于 `closure-20261118/identity/verified/6dc584c-public`。各平台清单与五份文件的匿名下载校验位于 `closure-20261118/identity/verified/6dc584cc1ef88ed37a1d60d3c5722f65be8d2b0f/release-verification`。[0.1.0 发布页](https://github.com/J-x-Z/AetherLang/releases/tag/v0.1.0) · [报告](docs/UPGRADE.md)

原发行线的下一项工作是在另一个配有常规 Clang 的 ARM64 GNU/Linux 发行版上构建和搬移安装包，核验 glibc 与 libLLVM 的最低分发要求。当前 ARM64 真机已经完成原生自举和 Adreno 计算验收。
