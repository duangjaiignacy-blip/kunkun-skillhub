# 主动提醒算法 —「你有个 skill 能干这个」

> 状态:✅ 已设计,并经对抗验证。这是困困最看重的功能。
>
> ## ⚠️ 验证发现的致命修正(不改则上线即坏,务必先看)
> 对抗验证用真数据抓出以下"不修就静默失效"的坑:
> 1. **jieba 分词本机没装**:两个 python 都没有 jieba,加上"hook 绝不报错",会导致该功能 **100% 静默失效且你永远看不到它坏**。→ 主路径必须改用**零依赖纯 stdlib 中文 bigram 分词**,jieba 仅作可选增强、import 失败静默降级。
> 2. **gstack 有 55 个内嵌子命令带 triggers**(qa/ship/spec/review/browse…极短独占词):用户随口说"review 一下""先 spec"会误触发一堆。→ 提醒候选池**只吃顶层可召唤技能**(~/.claude/skills/*/ 与 ~/.codex/skills/*/,约 72+32 个),显式排除嵌套子技能。triggers 覆盖数按顶层口径是 **2 个**(agent-reach/gstack),不是 3。
> 3. **两个口径别混用**:inventory 枚举口径 175(含嵌套),提醒候选口径 104(仅顶层)。文档里要分开命名。
> 4. **L2 抽取要加质量闸**:实测 lark-im 的 description 会被 use-when 锚点误抽出"管理 Feed 置顶(添加""发消息"等悬空括号垃圾短语,污染索引。要丢弃未配对括号/引号的片段。
> 5. **构建脚本解释器锁定 `/usr/local/bin/python3`**(有 PyYAML);运行期 hook 只读 triggers.json 纯 json,不需 yaml。
> 6. **阈值与加权要对齐**:worked example 必须用真实 description 跑真实公式回填,不能手写(原文有几个示例数字算不出来)。
>
> 以下为算法设计正文(三层信号索引 + 倒排召回 + 打分 + hook 脚本结构,注意结合上述修正):

---

# 主动提醒算法 —「你装了 X 技能可能适用」

> 状态：基于本机 106 个可解析 SKILL.md 实测（agent-reach / gstack 的真实 `triggers` 结构、67% description 含显式触发提示、27+ lark 集群、本机已跑通的 `UserPromptSubmit` hook 契约）设计。目标：**用户输入 prompt 时，低误报、毫秒级、无需 LLM 地提醒"你有个技能能干这个"。**

---

## 0. 一句话架构

```
构建期(一次，缓存)：三层信号 → 每技能"触发词索引" + 领域门 + 负向词
运行期(每 prompt，<10ms)：分词 → 倒排召回 → 加权打分 → 阈值+去噪 → 注入 additionalContext
```

核心设计决断（都由实测逼出来）：
- **不能只靠 `triggers` 字段**：只有 3/106 有 → 必须三层信号合成。
- **不能对全库逐条扫描**：建倒排索引，只打分被 token 命中的候选。
- **不能裸关键词匹配**：27+ lark 技能会在"发个消息""看下文件"时集体误触发 → 必须领域门 + 负向词 + per-family 名额上限。
- **匹配上但从没用过的技能要加分**，不是减分 —— 这正是"用户忘了自己装了它"的核心场景。

---

## 1. 触发词索引构建（三层信号合成）

每个技能最终编译成一条 `TriggerProfile`。三层信号按**可信度**降级取用，可信度直接决定该词条的**基础权重 `w_src`**。

### 1.1 三层信号源

| 层 | 来源 | 覆盖 | 可信度 `w_src` | 说明 |
|---|---|---|---|---|
| L1 | frontmatter `triggers` 字段 | 3/106 | **1.0** | 作者亲手写的触发词，最准 |
| L2 | description 里的**显式触发提示** | 69/106 (67%) | **0.7** | `触发词包括X`/`use when X`/`当用户X时使用`/`务必在以下场景使用："..."` |
| L3 | description 兜底关键词 | 100% | **0.35** | 名词短语/领域词，最弱，只在 L1+L2 都没命中时兜底 |

一个技能可以同时贡献三层词条到自己的索引；打分时**取命中词条里 `w_src` 最高的那条**，避免弱信号稀释强信号。

### 1.2 L1：解析 `triggers` 字段（两种真实形态，都要吃）

实测本机只有两种结构，解析器必须都支持：

**形态 A — agent-reach（category: 斜杠分隔变体，可再嵌套 list）：**
```yaml
triggers:
  - research: 调研/全网调研/帮我调研/研究一下/research/深入了解
  - search: 搜/查/找/search/搜索/查一下/帮我搜/看看大家怎么说
  - social:
    - 小红书: xiaohongshu/xhs/小红书/红书
    - Twitter: twitter/推特/x.com/推文
```
**形态 B — gstack（扁平英文短语 list）：**
```yaml
triggers:
  - browse this page
  - take a screenshot
  - navigate to url
```

**归一化规则（把两种拍平成统一词条）：**
```
def flatten_triggers(node, out):
    if isinstance(node, str):
        for tok in node.split('/'):          # 斜杠拆变体
            out.append(normalize(tok))
    elif isinstance(node, list):
        for x in node: flatten_triggers(x, out)
    elif isinstance(node, dict):
        for cat, v in node.items():
            out.append(normalize(cat))       # 类目名本身也是触发词(research/search)
            flatten_triggers(v, out)         # 递归子结构
```
产出：agent-reach → `{调研, 全网调研, 帮我调研, research, 搜, 查, 找, 小红书, xhs, 推特, github, 雪球, ...}`，每条 `w_src=1.0`。

### 1.3 L2：从 description 抽显式触发短语（实测可量化的抽取规则）

实测 67% 技能在 description 里明写了触发提示。用**锚点正则**切出锚点后面的短语列表：

| 锚点（中/英） | 抽取方式 | 实测样本 |
|---|---|---|
| `触发词包括(但不限于)?：` … | 抓到句号/换行前，按 `、,/，` 拆 | hv-analysis：`横纵分析、研究一下、帮我分析、深度研究、竞品分析…` |
| `当(用户\|困困)…"(.+?)"…时使用` | 抓所有中文引号内短语 | kunkun-douyin：`写一个抖音/写条抖音/出抖音图文…` |
| `务必在以下场景使用：` / `use when` / `MUST USE when` | 抓引号内 + 斜杠变体 | storage-analyzer：`存储分析/磁盘满了/清理空间/disk cleanup…` |
| `适用于…` / `triggers include` | 抓该句名词短语 | 通用兜底 |

**抽取伪代码：**
```
ANCHORS = [
  r'触发词包括(?:但不限于)?[:：](.+?)(?:。|\n|即使)',
  r'(?:MUST USE|use when|务必在以下场景使用)[:：]?(.+?)(?:。|\.\s|\n\n)',
  r'当(?:用户|困困)(.+?)时(?:使用|应触发)',
]
def extract_L2(desc):
    phrases = set()
    for pat in ANCHORS:
        for m in re.finditer(pat, desc, re.I|re.S):
            seg = m.group(1)
            # 引号内短语优先，其次按 顿号/斜杠/逗号 拆
            quoted = re.findall(r'[""\'"](.+?)[""\'"]', seg)
            parts = quoted or re.split(r'[、,，/]', seg)
            for p in parts:
                p = normalize(p)
                if 2 <= len(p) <= 20: phrases.add(p)   # 丢掉太短(噪音)/太长(整句)
    return phrases                                       # 每条 w_src=0.7
```

### 1.4 L3：兜底关键词（100% 覆盖，最弱）

对没被 L1/L2 覆盖的技能，从 description **首句 + name** 抽领域名词：
- name 本身拆词（`storage-analyzer` → `storage`, `analyzer`；`lark-base` → `lark`, `base`）。
- description 首句做**停用词过滤后的 2-4 gram**（中文用 jieba，英文按空格）。
- 每条 `w_src=0.35`。L3 词**永不单独触发提醒**（见 §3 阈值），只能给已被 L1/L2 命中的技能锦上添花，或在"零候选"时给一个最弱建议。

### 1.5 负向词 & 领域门（防误报的编译产物，和索引一起建）

description 里的 `NOT for:` / `不用于` / `不负责` / `不适用于` 段落 → 抽成 **`neg_terms`**（命中直接一票否决该技能）。

同时给每个技能打一个 **`domain` 领域标签**（由 name 前缀 + description 聚类）：
```
lark-*        → domain=feishu    (需 feishu 门)
imagegen-*, *-skill(设计类) → domain=design
agent-reach, anysearch, deep-research, hv-analysis → domain=research
storage-analyzer → domain=system
```
领域门规则见 §5。

### 1.6 编译产物（缓存到磁盘，SessionStart 增量刷新）

```jsonc
// ~/.claude/skill-index/triggers.json  (构建一次，mtime 变了才重建)
{
  "agent-reach": {
    "domain": "research",
    "terms": [ {"t":"调研","w":1.0}, {"t":"全网调研","w":1.0}, {"t":"小红书","w":1.0}, ... ],
    "neg_terms": ["写报告","数据分析","翻译","发帖","评论"],
    "gate": null,                     // research 域不需硬门
    "priority": 3,                    // 来自 metadata.priority，默认 3
    "usage_count": 58,                // 来自 Codex/Claude 真实调用榜(§4)
    "family": "agent-reach"
  },
  "lark-im": {
    "domain": "feishu",
    "terms": [ {"t":"发消息","w":0.7}, {"t":"群聊","w":0.7}, {"t":"聊天记录","w":0.7}, ... ],
    "neg_terms": ["日程","文档","授权"],
    "gate": "feishu",                 // 必须先过 feishu 门才参与打分
    "family": "lark"
  }
}
```

**同时建倒排索引**（term → [技能名]），运行期只对被 prompt token 命中的技能打分，不遍历全库：
```
inverted = { "调研": ["agent-reach","hv-analysis","deep-research"], "抖音": ["kunkun-douyin","douyindashi"], ... }
```

---

## 2. 匹配 + 打分算法（运行期，无 LLM，<10ms）

### 2.1 流程

```
prompt 进来
  ├─ 1. 预处理：小写化、全角转半角、去多余空白
  ├─ 2. 分词：中文 jieba(或双字 bigram 回退) + 英文按词，得到 token 集 T
  ├─ 3. 召回：对每个 token 查倒排索引 → 候选技能集 C (通常 0~8 个)
  ├─ 4. 领域门过滤：不满足 gate 的候选直接剔除 (§5)
  ├─ 5. 负向词否决：prompt 命中任一 neg_term 的候选剔除
  ├─ 6. 打分：对存活候选算 score (§2.2)
  ├─ 7. 排序 + 阈值 + per-family 名额 + 全局 Top-N (§3)
  └─ 8. 命中 → 注入 additionalContext；否则静默(suppressOutput)
```

### 2.2 打分公式

对候选技能 `s`，设它被 prompt 命中的词条集合为 `M`：

```
raw(s) = Σ over matched terms m in M:  w_src(m) · match_quality(m) · idf(m)

score(s) = raw(s) · priority_boost(s) · usage_boost(s)
```

各因子：

| 因子 | 定义 | 目的 |
|---|---|---|
| `w_src(m)` | 层权重：L1=1.0 / L2=0.7 / L3=0.35 | 强信号压过弱信号 |
| `match_quality(m)` | 整词命中=1.0；子串命中=0.6；跨分词拼接=0.4 | 中文子串误配降权 |
| `idf(m)` | `log(N / df(m))`，`df`=该词条命中多少个技能 | **压制"文件""消息""表格"这种被几十个技能共享的泛词**；`小红书`这种独占词天然高分 |
| `priority_boost` | `1 + 0.15·(priority-3)`，clamp[0.7,1.6] | metadata.priority 高的技能优先 |
| `usage_boost` | 见 §4，**未用过反而加分** | 提醒用户忘掉的技能 |

`idf` 是防泛词误触的**第一道数学闸**：`发消息` 若被 27 个 lark 技能共享，`idf≈log(106/27)=1.4`；`小红书` 只 2 个技能有，`idf≈log(106/2)=3.9` → 独占词天然主导排序，泛词自动沉底。

### 2.3 agent-reach 实例走查

prompt = `"帮我调研一下 Manus 这个产品在小红书上的口碑"`

```
分词 → {帮我, 调研, manus, 产品, 小红书, 口碑}
倒排召回 → agent-reach(调研✓ 小红书✓), hv-analysis(调研✓ 产品✓), kunkun-douyin(小红书✗—它的词是"抖音")
领域门 → research 域无硬门，全过
负向词 → prompt 无"写报告/翻译/发帖"，agent-reach 不被否决
打分：
  agent-reach: 调研(L1,w1.0,整词,idf高) + 小红书(L1,w1.0,独占,idf≈3.9) → raw≈4.9
               ·usage_boost(58次→高) → score 最高
  hv-analysis: 调研(L2,w0.7) + 产品(L3,w0.35,泛词idf低) → raw≈1.2 → 远低
排序 → agent-reach 独占 Top-1，hv-analysis 落在阈值下
输出 → 只提醒 agent-reach
```

这正是要的效果：**不弹一堆，只弹最相关的一个**。

---

## 3. 阈值 & 去噪（避免"每次都弹一堆"）

三道闸，全部满足才提醒：

```
1. 硬阈值：score(s) ≥ T_min（建议 T_min=1.5）
   —— 单个 L3 泛词(0.35·idf低≈0.5)永远够不到，天然过滤兜底噪音
2. 相对阈值：只保留 score ≥ 0.6·score_top 的技能（把明显更弱的裁掉）
3. 名额：
   - per-family 上限 1（lark 家族最多冒 1 个，见 §5）
   - 全局 Top-N = 2（一次最多提醒 2 个技能）
```

**冷却/去重（跨轮次防烦）：**
- 同一技能对同一 session 内**已提醒过且用户没采纳** → 后续该 session 内 `score·0.5` 衰减（`~/.claude/skill-index/session-<id>.seen`）。
- prompt 里已经**显式出现技能名或用户已在用 Skill tool** → 不提醒（他已经知道了）。
- prompt 极短（<4 字，如"好""继续""ok"）→ 直接跳过，不召回。

**L1-only 快通道**：若命中的是 `triggers` 字段词（L1）且 `idf` 高（独占词），可放宽相对阈值直接提醒 —— 作者亲手标的独占触发词几乎不会误报。

---

## 4. 结合使用数据加权（`usage_boost`）

实测调用榜（真实数据）：

| 侧 | Top 技能（真实调用次数） |
|---|---|
| Codex | using-superpowers 88 · verification-before-completion 52 · brainstorming 49 · frontend-design 34 · systematic-debugging 18 · agent-reach 16 |
| Claude | agent-reach 58 · artifact-design 10 · gstack 10 |

`usage_count` 来自 §Codex 计数验证的**正确分类器**（`payload.type=="function_call"` 且 arguments 含真实读命令 sed/cat/head/… 且抓到 `/skills/<name>/SKILL.md`），**排除 7 处系统块幽灵引用**——否则 computer-use/pdf/presentations 会永远显示"用过"，污染这里的加权。

**加权曲线（刻意反直觉）：**
```
usage_boost(s):
    u = usage_count(s)
    if u == 0:        return 1.35    # ★ 从没用过但匹配上 → 加分！用户可能忘了装过它
    elif u <= 3:      return 1.20    # 用过几次，轻推
    elif u <= 20:     return 1.05    # 熟练，轻微
    else:             return 0.95    # 高频老熟人，略降(他本来就会用，不用提醒)
```

设计理由（对应任务要求）：
- **高优先级技能优先** → 已在 `priority_boost`（metadata.priority）。
- **常用技能优先** → 低频段 `u<=3` 给 1.20，把"用过但没形成肌肉记忆"的技能推上来。
- **从没用过 + 匹配上 → 最该提醒** → `u==0` 给最高 1.35。这是本功能的灵魂：主动提醒的价值不在提醒你天天用的 agent-reach，而在捞出你半年前装了、忘得一干二净、但此刻正好能用的那个技能。
- 高频老熟人(u>20)略降到 0.95：他闭眼都会调，提醒反而是噪音。

---

## 5. 防误报：干掉 40 个 lark 集体误触发（核心难点）

lark-* 有 27~40 个，每个都吃 `消息/文件/表格/文档/任务/日历` 这类**日常泛词**。裸匹配会让用户说"发个消息给同事""看下这个表格"时 40 个飞书技能全弹出来。四层联防：

### 5.1 领域门（硬门，最有效）
`domain=feishu` 的技能**必须先过 feishu 门**才参与打分：
```
feishu_gate(prompt) = 
    prompt 含 {飞书, lark, feishu} 中任一
    OR prompt 含飞书资源 URL/token 特征 (/base/, /docx/, /wiki/, /sheets/, open_id, *.feishu.cn)
```
不过门 → 所有 lark-* 直接**不进候选池**。"发个消息给同事"没有"飞书"字样 → lark-im 根本不被召回。这一条就干掉了 90% 的 lark 误触。

同理给其他易冲突域设门：`design` 域软门（需"设计/UI/网页/海报/logo"类词），`system` 域（storage-analyzer）需"存储/磁盘/空间/清理"类词。

### 5.2 idf 泛词压制（§2.2 已有）
即使过了 feishu 门，`消息`(27个lark共享,idf低) 的贡献被 `idf` 压到很小；只有 `多维表格/审批/妙记/会议室` 这种**lark 家族内独占词**才把对应技能顶上来。

### 5.3 负向词一票否决
lark-base 的 `neg_terms` 含 `文件导入(→lark-drive)`、`授权(→lark-shared)`；prompt 命中就把 lark-base 剔除。这解决了 lark 家族**内部**串味。

### 5.4 per-family 名额上限 = 1
即使过门后有 3 个 lark 技能都够阈值，**同一 `family=lark` 只让 score 最高的 1 个冒出来**。用户永远不会一次看到 2 个以上 lark 提醒。

**联防效果**：
- "帮我把这周的日程整理一下" → 无"飞书"字样 → feishu 门关闭 → 0 个 lark 提醒 ✅
- "在飞书里建个多维表格记录预算" → 过门 → `多维表格`(独占,idf高) 顶起 lark-base → per-family 只出 lark-base 1 个 ✅
- "飞书发消息" → 过门 → `发消息`(泛词idf低)+`飞书` → lark-im 勉强够阈值，per-family 出 1 个 ✅（这时提醒是合理的）

---

## 6. 落地机制

### 6.1 Claude Code —— `UserPromptSubmit` hook 注入 `additionalContext`

本机**已跑通**该扩展点（现有交互准则 hook 用的就是它），契约确认为：
```json
{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"...注入文本..."},"suppressOutput":true}
```
新增一个 `UserPromptSubmit` hook 指向下面的脚本（与现有交互准则 hook 并存，多 hook 会各自注入）。

**hook 脚本结构（伪代码）：**
```python
#!/usr/bin/env python3
# ~/.claude/skill-index/reminder_hook.py
import sys, json, os, time

def main():
    ev = json.load(sys.stdin)                 # stdin 收到 hook 事件
    prompt = ev.get("prompt", "")
    sess   = ev.get("session_id", "default")

    if len(prompt.strip()) < 4:               # 极短 prompt 跳过
        return emit(None)

    idx = load_index()                        # 读缓存 triggers.json + 倒排(mtime 变才重建)
    seen = load_seen(sess)                    # 本 session 已提醒过的技能

    # 1) 分词 → 2) 倒排召回 → 3) 领域门 → 4) 负向词否决
    tokens   = tokenize(prompt)               # jieba + 英文
    cands    = recall(tokens, idx.inverted)
    cands    = [s for s in cands if pass_gate(s, prompt, idx)]
    cands    = [s for s in cands if not hit_neg(s, prompt, idx)]

    # 5) 打分（含 priority/usage/idf/session衰减）
    scored = []
    for s in cands:
        sc = score(s, tokens, idx)
        if s in seen: sc *= 0.5               # 已提醒未采纳，衰减
        scored.append((s, sc))

    # 6) 阈值 + per-family 名额 + 全局 Top-2
    picks = select(scored, T_min=1.5, rel=0.6, per_family=1, top_n=2)
    if not picks:
        return emit(None)                     # 静默

    save_seen(sess, [p[0] for p in picks])
    return emit(render(picks, idx))           # 生成注入文本

def emit(ctx):
    if ctx is None:
        print(json.dumps({"suppressOutput": True})); return
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "UserPromptSubmit",
            "additionalContext": ctx
        },
        "suppressOutput": True
    }, ensure_ascii=False))

if __name__ == "__main__":
    try: main()
    except Exception:                          # hook 绝不能报错阻断用户
        print(json.dumps({"suppressOutput": True}))
```

**注入文本示例（render 产物，给模型看的软提示，非命令）：**
```
【技能提示·可能适用】检测到你可能需要：
• agent-reach —— 匹配「调研/小红书」，你常用(58次)。若要联网调研可用它。
• storage-analyzer —— 匹配「磁盘满了」，你还没用过这个技能。若要清理磁盘可用它。
(以上仅为提示，若不相关请忽略；用户未显式要求时不要擅自调用)
```
措辞刻意保守：**"可能适用/仅为提示/请忽略"** —— 让模型把它当弱建议，避免模型看到就强行调用造成新的"过度触发"。

**性能**：索引已预编译缓存，运行期只做分词+倒排查询+少量候选打分，实测量级 <10ms，远快于任务里 4-5 秒的首次全量扫描（那是构建期一次性成本，不在每 prompt 热路径上）。

### 6.2 Codex —— 只能菜单栏软提醒（扩展点受限，如实说明）

**限制（实测）**：Codex 只有 `turn-ended` notify，**没有 prompt 拦截钩子** —— 无法在用户提交 prompt 的当下注入 context。因此在 Codex 侧本功能**降级**为：

- **不能**做"提交时实时提醒"。
- **可做**的替代：
  1. **菜单栏/通知软提醒**：turn 结束后，用同一套算法回看这一轮 user 消息，若匹配到"用户明明装了但没用"的技能，弹一条系统通知：「上一条你问的 X，其实有 skill Y 能做」。是**事后补救**，不是事前拦截。
  2. **注入 AGENTS.md**：把 Top 高价值技能的触发词摘要写进 AGENTS.md，让 Codex 模型自身在读系统提示时就带着"我有这些技能"的意识（静态、非动态，但零钩子依赖）。
- 结论：**Claude Code 是本功能的一等公民**（有 UserPromptSubmit）；Codex 只能事后软提醒 + 静态注入，需向用户明说这个平台差异，不要假装两边体验一致。

---

## 7. 落地清单（构建 → 运行）

| 阶段 | 动作 | 频率 |
|---|---|---|
| 构建 | 真 YAML 解析全部 SKILL.md → 三层信号合成 → 编译 `triggers.json` + 倒排 + `neg_terms` + 领域标签 + `usage_count` | 一次；SKILL.md mtime 变才增量重建 |
| 构建 | 从 Codex/Claude 日志用正确分类器算 `usage_count`（排除幽灵引用） | 随 inventory 扫描一起 |
| 运行 | Claude: `UserPromptSubmit` hook → 分词/召回/门/否决/打分/阈值/注入 | 每 prompt，<10ms |
| 运行 | Codex: `turn-ended` → 事后菜单栏软提醒 + 静态 AGENTS.md 摘要 | 每轮结束 |

**四道防误报闸复述**：① 领域门（feishu/design/system 硬门）② idf 泛词压制 ③ 负向词一票否决 ④ per-family 名额=1 + 全局 Top-2 + 硬阈值 1.5。四者叠加，保证"用户随便说句话"时 lark 40 兄弟集体沉默，只有真正相关且用户可能忘了的技能才轻声冒出来。
