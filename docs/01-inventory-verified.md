# Inventory 层 — 已验证的真实盘点与修正设计

> 状态:✅ 已用真实全量数据验证(2026-07-04)。这是方案里第一块从"纸面假设"升级为"实测闭环"的部分。
> 验证方法:用真 YAML 解析器逐个解析全部 SKILL.md,不是抽样,不是正则。

---

## 0. 一句话结论

**inventory 的核心假设成立(name+description 100% 覆盖),但原方案有三处与真实数据对不上,已在下方修正。**

---

## 1. 实测数字 vs 原方案(纠错表)

| 项 | 原方案说 | **实测真相** | 影响 |
|---|---|---|---|
| 技能 SKILL.md 总数 | ~106 (73 Claude + 33 Codex) | **175 个**(含插件 + 共享源全部 SKILL.md) | 数据模型要能容纳这个量级 |
| name+description 覆盖 | "统一,可能没全验" | **173/173 = 100%**(能解析的全有) | ✅ 核心字段假设牢固 |
| 真正坏的技能 | 只提 1 个(frontend-design) | **2 个**:douyindashi(无 frontmatter)、frontend-design(断链) | inventory 要能标记并展示坏技能 |
| frontmatter 字段 | 只取 name+description | 实际有 **14 种字段**,metadata 45%、version 35%、triggers/chainTo 等 | 数据模型太窄,要扩(见 §3) |
| 解析方式 | "serde_yaml 解析" | **必须真 YAML 库,不能正则** | 见 §2,这是个真坑 |

**各根目录实测条目:**
- `~/.claude/skills/` = 73(44 真目录 + 29 软链)
- `~/.codex/skills/` = 33(32 用户 + `.system` 5 个 meta 另计)
- `~/.agents/skills/` = 30(共享源,`~/.agents` → `/Desktop/codex/核心数据/.agents`)
- 插件:superpowers 14 + vercel 26

---

## 2. 修正一:解析器必须用真 YAML 库,严禁正则(实测踩坑)

**踩坑经过(真实发生):** 用简易正则 `^---\n(.*?)\n---` 解析时,把 vercel 的 4 个技能(ai-sdk / nextjs / vercel-storage / workflow)**误判为"无 frontmatter"**。

**根因:** 这些技能的 frontmatter 里有**多层嵌套 YAML**——
```yaml
---
name: nextjs
description: Next.js App Router expert guidance...
metadata:
  priority: 5
  docs:
    - "https://nextjs.org/docs"
  pathPatterns:
    - 'next.config.*'
---
```
正则遇到嵌套内容里的字符会提前截断/错配,把好技能杀成坏的。

**修正规则:**
1. `serde_yaml`(Rust)/ 真 YAML 库解析,**不做正则抽字段**。
2. frontmatter 边界:去 BOM → 必须以 `---` 开头 → 按 `---` 三段切分 → 中间段喂给 YAML 解析器。
3. 解析结果必须是 dict,否则标 `frontmatter_not_dict`。
4. 误判代价量化:正则会把真实的 **2 个坏技能虚报成 6 个(3.5% vs 1.1%)**,并误杀 4 个好技能。

---

## 3. 修正二:数据模型要接住 triggers / chainTo / metadata(不止 name+description)

原方案的 `Skill` 模型只取 name+description,**丢掉了对功能有用的字段**。实测字段分布:

| 字段 | 出现率 | 用途 | 原方案是否接 |
|---|---|---|---|
| name | 100% | 主键/展示 | ✅ |
| description | 100% | 一句话说明 + 匹配 | ✅ |
| metadata | 45% | 优先级/文档链接/路径模式 | ❌ 漏 |
| version | 35% | auto-update 判新 | ❌ 漏 |
| retrieval | 12% | 检索策略 | ❌ 漏 |
| **chainTo** | 11% | 技能链(A 之后接 B) | ❌ 漏,对提醒有用 |
| **triggers** | 2% | **作者写好的触发词** | ❌ 漏,对提醒是金矿 |
| allowed-tools | 1% | 工具白名单 | ❌ 漏 |

**关键洞察:** `triggers` 字段是技能作者**亲手写好的触发词**(agent-reach、gstack 都有)。原方案的"主动提醒"打算自己从 description 猜关键词——**其实优先读 triggers 字段更准**。数据模型必须把这些原样存进 `raw_frontmatter`(JSON blob),不能只挑两个字段。

**修正后的 Skill 记录追加字段:**
```
raw_frontmatter   TEXT   -- 完整 frontmatter 序列化成 JSON,保留全部字段
triggers          TEXT   -- 若 frontmatter 有 triggers,提取出来(主动提醒优先用)
chain_to          TEXT   -- 技能链目标
declared_version  TEXT   -- frontmatter.version(auto-update 用)
```

---

## 4. 修正三:坏技能要能识别并区分类型(不能当正常技能列)

实测 2 类坏技能,inventory 必须分别处理:

1. **`douyindashi` — 无 frontmatter 但仍是有效技能。** 它的 SKILL.md 直接以 `# 抖音运营大师` 开头,没有 YAML 头。但它确实能被调用(靠别的注册机制)。
   → 处理:标 `parse_ok=0, reason=no_frontmatter`,但**仍入库、仍可展示**,description 回退用首个 `#` 标题或首段文字。**绝不能因为没 frontmatter 就当它不存在。**

2. **`frontend-design` — 断链(`../../` 相对深度错)。** SKILL.md 不可达。
   → 处理:标 `is_broken=1`,UI 标红,作为"建议清理"的**最安全删除对象**(删了只是清一个无效链接)。

**原方案的漏洞:** 网站原型里把 douyindashi 当正常技能列出来了,没标它坏。修正后 inventory 输出必须带 `health` 字段:`ok | no_frontmatter | broken_symlink | unreadable`。

---

## 5. 修正后的发现算法(取代原方案 §1.2)

```
对每个 <root>/<entry>:
  1. lstat:普通文件(llms.txt 等)→ 跳过
  2. 软链且 realpath 目标不存在 → health=broken_symlink,入库,标"建议清理"
  3. 定位 realpath/SKILL.md:
       不存在 → health=unreadable
  4. 读文件,去 BOM:
       不以 --- 开头 → health=no_frontmatter,description 回退首个 # 标题
       以 --- 开头 → 真 YAML 解析中间段:
           解析失败 → health=yaml_error
           非 dict  → health=frontmatter_not_dict
           成功    → health=ok,存 raw_frontmatter(全字段)+ 提取 triggers/chainTo/version
  5. 产出 Skill 记录(含 health、raw_frontmatter)
```

**来源分类(实测确认三类,格式略有差异但都能被真 YAML 解析器统一处理):**
- 用户技能(claude_user / codex_user):标准 frontmatter
- 共享源软链(→ ~/.agents/skills,实际在 Codex 根下):同格式,但**删除时只删链接节点,永不跟随**(见删除安全章节)
- 插件技能(plugin:*):**有嵌套 metadata**,只能用真 YAML 解析器,不能正则

---

## 6. 这块地基现在的状态

✅ **已验证成立:** name+description 100% 覆盖;175 技能可枚举;2 个坏技能已定位。
✅ **已修正:** 解析器改真 YAML;数据模型扩字段接 triggers/chainTo;坏技能分类处理。
⏳ **仍待做(其余方案模块):** Codex 调用计数亲手验证、修法回填 allowlist/删除流程、主动提醒算法落地、性能实测。

> 下一块建议补:**亲手验证 Codex 调用计数**——这是整个方案剩下的最大未验证假设。
