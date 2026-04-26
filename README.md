# RAID-reassemble

中文 | [English](#english)

RAID-reassemble 是一个命令行 RAID 重组与取证辅助工具。项目基于论文 **Generic RAID reassembly using block-level entropy** 的思路，使用块级熵、XOR 冗余关系和启发式评分，从多个磁盘镜像中推断 RAID 参数，并在可行时生成重组后的逻辑卷镜像。

当前版本是 Rust 原型，目标是先跑通 RAID-0、RAID-1、RAID-5 的基础检测与重组链路，再逐步补齐 offset 检测、RAID-5 layout 自动推断、文件系统验证和完整审计报告。

## 功能状态

已实现：

- raw/dd 磁盘镜像读取
- 512 字节或自定义块大小扫描
- Shannon entropy 块熵计算
- zero block 检测
- XOR parity 检测
- RAID 类型启发式检测：RAID-0、RAID-1、RAID-5、degraded RAID-5 候选
- 基于低熵/高熵边界的 stripe size 投票
- RAID-0 逻辑卷组装
- RAID-1 镜像复制
- RAID-5 基础组装，支持四类 layout 参数
- RAID-5 单盘缺失 XOR 恢复
- JSON 检测报告
- Markdown 检测报告
- 基础单元测试和文件级集成测试

尚未完成：

- 数据 offset 自动检测
- RAID-5 layout 自动高置信度推断
- GPT/MBR/NTFS/Ext 文件系统结构评分
- top-k 候选的完整验证链
- 坏块、不等长镜像、短读区域的细粒度标记
- rfrb-master 对照测试集
- 大镜像扫描缓存和断点续跑

## 适用场景

RAID-reassemble 适合用于：

- 原 RAID 控制器不可用，只剩多块磁盘镜像
- RAID 参数未知，需要先获得候选配置
- NAS、小型服务器、旧系统中的 RAID-0、RAID-1、RAID-5 初步分析
- 取证流程中需要可解释的 RAID 参数推断报告
- RAID-5 缺失一块盘时尝试 XOR 恢复

它不是一个“自动恢复所有 RAID”的黑箱工具。全盘加密、随机填充空闲区、高熵容器文件、复杂嵌套 RAID、RAID-6、ZFS/Btrfs/Storage Spaces 等场景仍需要后续扩展或其他专业工具。

## 构建

需要 Rust 1.95 或更新版本。

```powershell
cargo build
```

如果当前 shell 没有加载 Cargo 到 `PATH`，可以在 Windows 上使用绝对路径：

```powershell
C:\Users\caochensheng\.cargo\bin\cargo.exe build
```

运行测试：

```powershell
cargo test
```

当前测试状态：

```text
6 passed
```

## 使用方法

查看帮助：

```powershell
target\debug\raid-reassemble.exe --help
```

输出：

```text
RAID-reassemble

Usage:
  raid-reassemble scan <images...> [--block-size 512] [--max-blocks N]
  raid-reassemble detect <images...> [--raid auto|raid0|raid1|raid5] [--stripe 256K] [--output result.json] [--markdown report.md]
  raid-reassemble assemble <images...> --raid raid0|raid1|raid5 --stripe SIZE --output logical.img [--order 0,1,2,3] [--layout left-symmetric]
  raid-reassemble recover <images...> --stripe SIZE --missing INDEX --output logical.img [--order 0,1,2,3] [--layout left-symmetric]
```

### scan

扫描磁盘镜像并输出基础统计信息。

```powershell
target\debug\raid-reassemble.exe scan disk0.img disk1.img disk2.img --block-size 512 --max-blocks 100000
```

输出包括：

- 镜像数量
- 块大小
- 每个镜像大小
- zero block 数量
- 平均熵

### detect

自动推断 RAID 类型和 stripe size 候选。

```powershell
target\debug\raid-reassemble.exe detect disk0.img disk1.img disk2.img disk3.img `
  --raid auto `
  --top 10 `
  --output result.json `
  --markdown report.md
```

如果已知部分参数，可以手工指定：

```powershell
target\debug\raid-reassemble.exe detect disk0.img disk1.img disk2.img disk3.img `
  --raid raid5 `
  --stripe 256K `
  --output result.json
```

### assemble

按已知参数生成逻辑卷镜像。

RAID-0：

```powershell
target\debug\raid-reassemble.exe assemble disk0.img disk1.img disk2.img `
  --raid raid0 `
  --stripe 256K `
  --order 0,1,2 `
  --output logical.img
```

RAID-1：

```powershell
target\debug\raid-reassemble.exe assemble disk0.img disk1.img `
  --raid raid1 `
  --stripe 512 `
  --output logical.img
```

RAID-5：

```powershell
target\debug\raid-reassemble.exe assemble disk0.img disk1.img disk2.img disk3.img `
  --raid raid5 `
  --stripe 256K `
  --order 0,1,2,3 `
  --layout left-symmetric `
  --output logical.img
```

支持的 RAID-5 layout：

- `left-symmetric`
- `left-asymmetric`
- `right-symmetric`
- `right-asymmetric`

### recover

RAID-5 缺失一块盘时，通过 XOR 生成逻辑卷输出。

```powershell
target\debug\raid-reassemble.exe recover disk0.img disk1.img disk3.img `
  --stripe 256K `
  --missing 2 `
  --order 0,1,2,3 `
  --layout left-symmetric `
  --output recovered-logical.img
```

`--missing` 是完整 RAID 中缺失磁盘的零基索引。比如完整顺序是 `0,1,2,3`，当前只有 `disk0.img disk1.img disk3.img`，则缺失索引为 `2`。

## 检测方法概览

当前检测逻辑吸收了 rfrb-master 和论文中的核心启发式：

1. 读取各镜像同一偏移的块。
2. 若所有块相同，计为 mirrored block。
3. 若所有块 XOR 后为零，计为 parity block。
4. 否则计为 unassigned block。
5. 根据 mirrored/parity/unassigned 比例生成 RAID 类型候选。
6. 计算块级熵，寻找低熵到高熵或高熵到低熵的稳定边界。
7. 对边界间距做常见 stripe size 投票。
8. 综合 RAID 类型分数和 stripe size 票数生成候选排名。

这些结论是统计推断，不是绝对证明。正式取证流程中应结合文件系统验证、哈希验证和人工审查。

## 项目结构

```text
src/
  main.rs       CLI 入口
  cli.rs        命令解析与调度
  image.rs      raw 镜像读取与扫描摘要
  features.rs   entropy、zero block、XOR 基础函数
  detect.rs     RAID 类型检测和 stripe size 投票
  raid.rs       RAID-0/1/5 组装与 RAID-5 缺盘恢复
  report.rs     JSON 和 Markdown 报告输出
```

## 设计文档

完整设计见：

- [RAID-reassemble-design-document.md](RAID-reassemble-design-document.md)

该文档包含工具定位、架构、CLI、算法、评分模型、rfrb-master 评估、开发路线和测试策略。

## 取证注意事项

- 输入镜像应只读保存，处理前后应计算哈希。
- 当前工具不会修改输入镜像。
- 输出镜像是新生成文件，应单独计算哈希。
- 低置信度候选不应直接作为最终结论。
- 能挂载文件系统不等于全卷重组完全正确。
- 全盘加密、随机填充和高熵数据会显著削弱熵边界检测。

## License

尚未确定最终许可证。

注意：`rfrb-master` 声明为 GPLv3 或更高版本。本项目当前采用重新实现算法思想的方式，没有直接复制 rfrb 源码。如果未来直接复用 rfrb 代码，需要选择 GPLv3 兼容许可证。

---

## English

RAID-reassemble is a command-line RAID reassembly and forensic assistance tool. It is inspired by the paper **Generic RAID reassembly using block-level entropy** and uses block-level entropy, XOR redundancy, and heuristic scoring to infer RAID parameters from disk images.

The current version is a Rust prototype. Its immediate goal is to make the basic RAID-0, RAID-1, and RAID-5 detection and reassembly workflow usable, then gradually add offset detection, RAID-5 layout inference, filesystem validation, and a stronger audit trail.

## Feature Status

Implemented:

- raw/dd disk image reading
- 512-byte or custom block-size scanning
- Shannon entropy calculation
- zero block detection
- XOR parity detection
- heuristic RAID type detection for RAID-0, RAID-1, RAID-5, and degraded RAID-5 candidates
- stripe size voting from low/high entropy boundaries
- RAID-0 logical image assembly
- RAID-1 mirror copy
- basic RAID-5 assembly with explicit layout
- RAID-5 single-missing-disk XOR recovery
- JSON detection report
- Markdown detection report
- basic unit and file-level integration tests

Not implemented yet:

- automatic data offset detection
- high-confidence automatic RAID-5 layout inference
- GPT/MBR/NTFS/Ext structural scoring
- full top-k candidate validation chain
- detailed bad-block and short-read marking
- rfrb-master comparison dataset
- large-image scan cache and resumable jobs

## Use Cases

RAID-reassemble is intended for:

- cases where the original RAID controller is unavailable
- unknown RAID parameters that need candidate analysis
- RAID-0, RAID-1, and RAID-5 images from NAS devices, small servers, and legacy systems
- forensic workflows that need explainable RAID inference
- RAID-5 arrays with one missing disk

It is not a black-box tool that can automatically recover every RAID. Full-disk encryption, random-filled free space, high-entropy container files, nested RAID, RAID-6, ZFS, Btrfs, and Storage Spaces require further work or specialized tools.

## Build

Rust 1.95 or newer is recommended.

```powershell
cargo build
```

If Cargo is not available in the current shell `PATH` on Windows:

```powershell
C:\Users\caochensheng\.cargo\bin\cargo.exe build
```

Run tests:

```powershell
cargo test
```

Current test status:

```text
6 passed
```

## Usage

Show help:

```powershell
target\debug\raid-reassemble.exe --help
```

### scan

Scan images and print basic entropy statistics:

```powershell
target\debug\raid-reassemble.exe scan disk0.img disk1.img disk2.img --block-size 512 --max-blocks 100000
```

### detect

Infer RAID type and stripe size candidates:

```powershell
target\debug\raid-reassemble.exe detect disk0.img disk1.img disk2.img disk3.img `
  --raid auto `
  --top 10 `
  --output result.json `
  --markdown report.md
```

Force known parameters:

```powershell
target\debug\raid-reassemble.exe detect disk0.img disk1.img disk2.img disk3.img `
  --raid raid5 `
  --stripe 256K `
  --output result.json
```

### assemble

Assemble a logical image from known parameters:

```powershell
target\debug\raid-reassemble.exe assemble disk0.img disk1.img disk2.img `
  --raid raid0 `
  --stripe 256K `
  --order 0,1,2 `
  --output logical.img
```

```powershell
target\debug\raid-reassemble.exe assemble disk0.img disk1.img disk2.img disk3.img `
  --raid raid5 `
  --stripe 256K `
  --order 0,1,2,3 `
  --layout left-symmetric `
  --output logical.img
```

Supported RAID-5 layouts:

- `left-symmetric`
- `left-asymmetric`
- `right-symmetric`
- `right-asymmetric`

### recover

Recover a RAID-5 logical image with one missing disk:

```powershell
target\debug\raid-reassemble.exe recover disk0.img disk1.img disk3.img `
  --stripe 256K `
  --missing 2 `
  --order 0,1,2,3 `
  --layout left-symmetric `
  --output recovered-logical.img
```

`--missing` is the zero-based disk index in the full RAID order.

## Method Overview

The current detection workflow follows the paper and the rfrb-master prototype:

1. Read blocks at the same offset from all images.
2. Count identical rows as mirrored blocks.
3. Count XOR-zero rows as parity blocks.
4. Count the rest as unassigned blocks.
5. Generate RAID type candidates from mirrored/parity/unassigned ratios.
6. Compute block entropy.
7. Detect stable low-to-high or high-to-low entropy edges.
8. Vote for likely stripe sizes from distances between entropy edges.
9. Rank candidates using RAID type evidence and stripe votes.

The result is heuristic evidence, not absolute proof. For forensic use, combine it with filesystem validation, hash validation, and manual review.

## Project Layout

```text
src/
  main.rs       CLI entry point
  cli.rs        command parsing and dispatch
  image.rs      raw image reading and scan summaries
  features.rs   entropy, zero block, and XOR primitives
  detect.rs     RAID type detection and stripe size voting
  raid.rs       RAID-0/1/5 assembly and RAID-5 recovery
  report.rs     JSON and Markdown report output
```

## Design Document

See:

- [RAID-reassemble-design-document.md](RAID-reassemble-design-document.md)

## Forensic Notes

- Keep input images read-only and hash them before and after processing.
- RAID-reassemble does not modify input images.
- Output images should be hashed separately.
- Low-confidence candidates should not be treated as final conclusions.
- A mountable filesystem does not prove the whole logical volume is correct.
- Encryption, random filling, and uniformly high-entropy data can defeat entropy-boundary detection.

## License

The final license has not been selected yet.

Note: `rfrb-master` is GPLv3-or-later. This project currently reimplements the algorithmic ideas without copying rfrb source code. Directly reusing rfrb code would require a GPLv3-compatible license.
