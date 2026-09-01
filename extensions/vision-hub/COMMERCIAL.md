# vision-hub 商业化与 Pro 仓库拆分方案

> 状态:**方案定稿,未执行**。代码目前仍在 NeoMind-Extensions 工作区(未提交)。
> 执行拆分时按本文件的清单操作即可。

## 1. 结论:双仓库 + 桥接发现(平台零改动)

平台市场的关键事实(`neomind-api/src/handlers/extensions.rs:1735`):

- **发现层硬编码**:`MARKET_BASE_URL = raw.githubusercontent.com/camthink-ai/NeoMind-Extensions`
  ——平台按扩展 id 到**该仓库**拉 `extensions/<id>/metadata.json`,编译期常量,私有仓库不可达(raw.githubusercontent 无 token);
- **下载层自由**:安装时 `.nep` URL 直接取自 metadata.json 的 `builds[].url`,**可指向任何地址**。

因此:

```
NeoMind-Extensions(公开,免费/社区)      NeoMind-Extensions-Pro(私有源码,商业线)
├─ 免费扩展继续维护(懒得动就冻结)         ├─ crates/vision-common(整仓搬走)
├─ 兼任"总目录":Pro 扩展只放一份          ├─ extensions/vision-hub 及后续高价值扩展
│  metadata.json(builds.url → Pro 仓    ├─ CI:6 平台构建 → 发本仓 GitHub Releases
│  的 Release 下载地址)                 │  (Release 公开、源码闭源)
└─ Apache-2.0 继续开源                   └─ 独立版本节奏 / CI 密钥 / 许可服务隔离
```

- 平台**一行不用改**,付费扩展即可在市场被搜到、被安装;
- 源码保密靠"私有仓库 + 公开 Release 二进制"——.nep 本来就是二进制分发格式;
- 若将来要求**下载本身受控**(不只是功能门控),再做核心仓库的市场多源改造(settings 配置源列表 + 鉴权头),那是后话。

### 拆分操作清单(执行时照做)

1. `gh repo create camthink-ai/NeoMind-Extensions-Pro --private`(名字待定,备选:NeoMind-Studio / camthink-vision-pro);
2. 搬移:`crates/vision-common/`、`extensions/vision-hub/` 整目录 → Pro 仓;Pro 仓根建 workspace Cargo.toml(抄现仓库的 members/依赖/patch 段,删掉旧扩展条目);
3. 搬 `build.sh`(删掉 26 个旧扩展的 V2_EXTENSIONS 条目只留 vision-hub)、`rust-toolchain.toml`、`.github/workflows/build-nep-packages.yml`(去掉无关扩展,保留 ORT/FFmpeg 安装与 6 平台矩阵、cargo test 步骤);
4. 在 NeoMind-Extensions(免费仓)保留 `extensions/vision-hub/metadata.json` 一份,**builds.url 手工指向 Pro 仓 Release**;从 build.sh/update-versions.sh/CI 中移除 vision-hub 与 vision-common 条目;
5. Pro 仓首次 Release 后,免费仓 index.json 里 vision-hub 条目即完成上架桥接。

## 2. 收费前必须完成的合规项 ⚠️

**这是硬门槛,不解决不能收钱:**

| 现用资产 | 许可证 | 要求 |
|---|---|---|
| yolo11n.onnx(Ultralytics YOLO)| **AGPL-3.0** | 商用二选一:①购买 Ultralytics 商业许可;②默认捆绑改为 Apache-2.0 模型:**YOLOX-s / RT-DETR / PP-YOLOE**(PaddlePaddle 系,与 OCR 线同源)。vision-common 的 YoloLayout 已支持换型(加 layout 即可),商业版建议默认 YOLOX,yolo11n 改为"用户自备" |
| det_10g.onnx / w600k_r50.onnx(InsightFace)| 代码 MIT,**预训练模型明确非商业** | face 批次商用前必须换模型源(候选:商汤/OpenCV Haar 级联 + 自训 embedding,或购买商用检测模型) |
| PP-OCR 系 | Apache-2.0 | ✅ 无障碍 |
| usls 移植代码 | MIT(© Jamjamjon)| ✅ 已在源码保留版权声明,闭源分发允许 |
| NotoSans 字体 | SIL OFL 1.1 | ✅ 可捆绑,不可单独卖字体 |

现有免费仓的 5 个 yolo 扩展同样存在 AGPL 暴露——**收费行为会大幅提升被追究概率**,建议商业版上线前一并评估(哪怕只是给免费版加 "YOLO weights © Ultralytics, AGPL" 声明)。

## 3. License key 机制(已实现,默认开放)

`vision-common::license` 已落地,当前策略**默认全开放**(无 license.key = 所有任务可用),发布后可随时切换收费而不需要重新发版架构:

- **格式**:`NP1.<b64url(payload_json)>.<b64url(ed25519_sig)>`,单行 ASCII;
- **payload**:`{to, exp(unix秒|null), features:["detect","ocr","face","ground","vlm","*"], fp(设备指纹|null)}`;
- **校验**:Ed25519 离线验签(`verify_strict`)+ 过期检查 + 设备指纹绑定(SHA-256(machine-id) 前 16 hex,不外泄原始 ID);无网络、无遥测;
- **失败语义(fail-closed)**:license **不存在** ⇒ 默认开放;**存在但**篡改/过期/指纹不符 ⇒ 全部拒绝,绝不降级为开放;
- **门控点**:vision-hub 的 `TaskRegistry::get_or_load`(任务实例唯一创建点),`get_status` 暴露 license 状态,`reload_license` 命令支持热更新;
- **测试**:vision-common 9 个 license 单测(签发/验签/篡改/过期/指纹/畸形输入),vision-hub 1 个门控测试(default-open 放行、Rejected 封死、部分授权只放行列表内任务)。

### 启用收费前还要做( vendor 侧)

1. 生成正式 Ed25519 keypair,替换 `license.rs` 的 `PRO_PUBLIC_KEY` 占位(当前全零 → 任何签名验证失败 → fail-closed,所以**现在放占位是安全的**);
2. 写 `license-keygen` 小工具(签发端,放 Pro 仓私有):读 payload → 出 license 行;含指纹读取命令(`get_status` 的 license 段可直接给用户抄);
3. 决定收费粒度(全功能一把钥匙 vs 按任务位零售)——payload 的 features 数组两种都支持。

## 4. 免费层 / 付费层切分建议(产品决策,待定)

| 方案 | 免费(NeoMind-Extensions)| 付费(Pro)|
|---|---|---|
| A. 整体收费 | 旧 yolo 扩展继续免费(不动) | vision-hub 全部 |
| B. Freemium(常见)| vision-hub 社区版(detect + 默认小模型,license 机制闲置) | Pro 版:ocr/face/vlm 任务位 + 大模型 + jetson/cuda 变体优先支持 |

B 与现有 license 机制天然匹配(同一二进制,靠 key 开任务位),且社区版持续导流。建议 B。

## 5. 当前状态快照(2026-08-29)

- vision-common:10 模块(含 license),65 单测;vision-hub:detect 批次,20 单测;
- 端到端已验证:.nep 打包(26.9MB 含 ORT 1.22 + yolo11n)→ 隔离实例自动安装 → `analyze`(bus.jpg:5 检测,**coreml**,28ms)→ `get_status` 硬件画像完整;
- 代码位置:NeoMind-Extensions 工作区**未提交**(crates/vision-common、extensions/vision-hub、以及 build.sh/Cargo.toml/CI/update-versions.sh 的集成修改);
- 待用户拍板:①是否执行双仓拆分(本文 §1);② Pro 仓库名;③收费模式(§4)。
