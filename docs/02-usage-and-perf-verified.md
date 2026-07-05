# Codex 计数验证 + 性能实测 — 已跑通

> 状态:✅ 已用本机真实数据跑通(2026-07-04)。这是方案里"最大的未验证假设"——Codex 调用计数会不会虚高——的实测闭环。

---

## 1. Codex 计数验证:虚高属实,但可精确剔除

### 1.1 单文件实测(真实分类)

拿一个真实 rollout 文件解析,48 处 `/skills/<name>/SKILL.md` 引用清晰分三类:

| 类别 | 判定条件 | 次数 | 是否真调用 |
|---|---|---|---|
| **真读命令** | `payload.type=="function_call"` 且 arguments 含 `sed/cat/head/less/bat/nl/tail` + SKILL.md 路径 | **25** | ✅ 是 |
| 正文提及 | 出现在用户/助手对话文本里 | 16 | ❌ 否 |
| 系统块幽灵 | 出现在 system/developer 消息块 或 session_meta | 7 | ❌ 否 |

### 1.2 核心结论

- **naive 正则(整行抓 SKILL.md)= 48 次,虚高近一倍。**
- **正确分类器 = 25 次真调用**,且能精确定位到技能(test-driven-development 5、figma-use 4…)。
- **幽灵引用集中在 `computer-use`/`pdf`/`presentations` 这类系统级能力**——它们被列在系统块但从没真读。若用正则,它们永远显示"用过",**永不进"可删除"列表 → 毁掉核心功能**。这个机制现在被实测证实。

### 1.3 Codex 全量真实调用榜(正确分类器)

38 个技能有真调用,共 389 次:

| 技能 | 真调用次数 |
|---|---|
| using-superpowers | 88 |
| verification-before-completion | 52 |
| brainstorming | 49 |
| frontend-design | 34 |
| systematic-debugging | 18 |
| agent-reach | 16 |
| control-in-app-browser | 15 |
| lark-shared | 12 |
| image2_UI_skill | 11 |

> 这是你 Codex 侧从未有过的真实使用画像。注意:Codex 计数 confidence 仍标 `inferred`(读取即调用,非显式 Skill 事件),与 Claude 的 `exact` 分开展示,不相加。

### 1.4 正确的计数器(伪代码,已验证可行)

```python
for line in rollout_jsonl:
    if 'SKILL' not in line: continue          # 预筛,性能关键
    obj = json.loads(line)
    payload = obj.get("payload", {})
    if payload.get("type") == "function_call":
        args = payload.get("arguments", "")
        if re.search(r'(sed|cat|head|less|bat|nl|tail)\b', args):
            for m in re.finditer(r'/skills/([\w.-]+)/SKILL\.md', args):
                count[m.group(1)] += 1     # 只有这里才算真调用
```

---

## 2. 性能实测:原方案的悲观假设被推翻

### 2.1 Codex 1.3GB 全量扫描 — 实测数字

| 指标 | 实测值 |
|---|---|
| 文件数 | 88 |
| 总大小 | 1346 MB |
| 扫描行数 | 36,132 (含大文件 147MB) |
| **耗时** | **2.49 秒** |
| 吞吐 | 541 MB/s |

### 2.2 结论

- **原方案(和验证 agent)担心"1.3GB Codex 扫描拖慢 UI"——实测证明担心过度。** Python 都能 2.5 秒扫完,Rust 会更快。首扫两端(Claude 598MB + Codex 1346MB)加起来 **4~5 秒可接受**。
- **关键优化(必须写进实现):`'SKILL' not in line` 预筛**,只对含 SKILL 的行做 `json.loads`,避开对 360 万行逐行 JSON 解析。这是 541 MB/s 的来源。
- 仍建议:首扫放后台线程,UI 先渲染 app.db 里的上次结果,不阻塞冷启动(增量刷新后更是亚秒级)。

---

## 3. 这两块的状态

✅ **Codex 计数:** 虚高机制证实 + 正确分类器验证可行 + 全量真实榜跑出。
✅ **性能:** 1.3GB 实测 2.49 秒,悲观假设推翻,预筛优化确认。
⏳ **待工作流回填:** 修法写回设计正文、主动提醒算法落地(均基于本文档实测数据)。
