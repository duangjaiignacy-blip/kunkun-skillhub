# SkillHub 技能一体化管理台 — 方案文档索引

> Claude Code + Codex 双端技能管理平台。macOS 单机 · 桌面端优先 · 按需刷新 · 本机/局域网 · 只读数据源。
> 所有数字均来自 2026-07-04 对本机的**真实实测**,不是估算。

---

## 文档结构

| 文档 | 内容 | 状态 |
|---|---|---|
| [00-README.md](00-README.md) | 本索引 + 全局结论 | — |
| [01-inventory-verified.md](01-inventory-verified.md) | ① 全量技能扫描:175 个 SKILL.md 逐个解析,格式验证,坏技能定位 | ✅ 实测闭环 |
| [02-usage-and-perf-verified.md](02-usage-and-perf-verified.md) | ① Codex 计数验证 + ④ 性能实测(1.3GB / 2.49s) | ✅ 实测闭环 |
| [03-fix-backfill.md](03-fix-backfill.md) | ② 三个致命/严重坑的修法回填(软链删除/Web 令牌/Codex 计数) | ✅ 设计+验证 |
| [04-reminder-algorithm.md](04-reminder-algorithm.md) | ③ 主动提醒算法(三层信号/倒排/hook) | ✅ 设计+验证 |

可视化方案总览网站:https://claude.ai/code/artifact/da1bad27-a622-4149-bb1b-37a0773a1ccf

---

## 四块任务完成情况(困困要求的验证/校核/算法/性能)

### ① 验证 — Codex 调用计数(方案最大的未验证假设)
- **虚高属实**:naive 正则数 48 次,正确分类器只 25 次真调用。
- **正确分类器**:`payload.type=="function_call"` + arguments 含读命令(sed/cat/head…)+ 抓 `/skills/<name>/SKILL.md`。
- **全量真实榜**:using-superpowers 88、brainstorming 49、frontend-design 34…(Codex 侧首次真实画像)。
- **修正**(验证发现):计数身份必须带路径根 `(name, source_root)`,因为 computer-use 等同名技能横跨插件缓存 + 清单,只用 name 会串味。

### ② 校核填补 — 修法回填设计正文
三个坑从"文档里列修法"升级为"可实现规范":
- **软链删除**:`delete-link`(只删链接节点,永不 realpath 跟随)vs `delete-source`(独立校验)两动词,伪代码可直接用。
- **Web 令牌**:开 LAN 必自签 TLS + 指纹随二维码 pin;token 存 sessionStorage 短 TTL,禁 localStorage;Host 头校验防 DNS 重绑定。
- **Codex 计数**:可删除判定只用 Claude exact 信号,Codex 标"仅供参考"。

### ③ 主动提醒算法
- **三层信号**:L1 triggers 字段(最准但仅 2 个顶层技能有)→ L2 从 description 抽显式触发提示(67% 有)→ L3 关键词兜底。
- **落地**:Claude Code 走 UserPromptSubmit hook 注入 additionalContext(本机已有可用 hook 佐证);Codex 只能菜单栏软提醒。
- **关键修正**(验证发现):分词改零依赖 stdlib bigram(jieba 本机没装);候选池只吃顶层技能(排除 gstack 55 个子命令避免误触发)。

### ④ 性能测试
- **Codex 1346MB / 88 文件全量扫描 = 2.49 秒**,541 MB/s。
- **推翻原方案悲观假设**:"1.3GB 会拖慢 UI"过度担心,首扫两端 4~5 秒可接受。
- **关键优化**:`'SKILL' not in line` 预筛,只对含 SKILL 的行 json.loads。

---

## 诚实的现状评估

**这份方案现在的成色:** 从"方向正确的草案"升级为"关键假设已实测验证、致命坑已给出可实现修法"的设计。四块硬骨头都啃过了,而且每块都经过对抗验证(验证还纠正了我实测里的一处论证错误——见 03 文档)。

**仍未做(诚实标注):**
- 尚未写一行真实产品代码(这些是设计+验证,不是实现)。
- 应用本地 SQLite schema 未用真实全量数据灌压测试。
- 主动提醒的 worked example 数字需在实现时用真实 description 跑真实公式回填(验证指出原文有手写数字对不上)。
- auto-update 的"来源可更新性"仍需 Phase 2 逐技能探测。

**下一步建议:** 进入 Phase 0 — 用 Rust 写 scanner-core CLI,把这些已验证的逻辑(YAML 解析 / Codex 分类器 / 预筛扫描)变成真代码,拿 Claude 侧 118 次、Codex 侧真实榜做回归基线。
