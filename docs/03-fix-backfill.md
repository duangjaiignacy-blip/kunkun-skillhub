# 修法回填 — 三个致命/严重坑的可实现设计规范

> 状态:✅ 已回填,并经对抗验证。本文档取代原方案正文里带 bug 的三段(删除逻辑/Web 鉴权/Codex 计数)。
>
> ## ⚠️ 验证发现的关键修正(实现时务必注意)
> 对抗验证用真数据抓出以下几处,已知需在实现时修正:
> 1. **Codex 计数身份必须带路径根**:`computer-use` 等技能同名存在于多个位置(插件缓存 + 175 清单),计数器只抓 name 会串味。主键改 `(skill_name, source_root)`,不能只用 name。原"幽灵引用从没被读"的论证错误——它们其实被读过,虚高真因是系统块成批罗列 + 同名串味。
> 2. **`~/.claude` 本身是软链**:`LINK_MANAGE_ROOT` 不能写死字符串,必须运行时 `realpath(expanduser('~/.claude/skills'))` 动态求值,并在启动自检里断言一致。
> 3. **macOS 大小写不敏感**:`_under` 前缀判定要 casefold 归一,或改用 `os.path.samefile`/inode 比对,否则 deny 前缀可被大小写绕过。
> 4. **delete_source 事前扫描要含 files**:指向文件的软链和断链落在 `files` 不在 `dirs`,原扫描漏检一半(实测 Python 3.14 的 rmtree 本身不跟随软链,是真正兜底)。
> 5. token_ok 的"防 compare_digest 早退泄漏"注释是错的——`hmac.compare_digest` 本就常数时间且处理不等长,长度门非安全关键。
>
> 以下为设计正文(软链删除的两动词方案 + 伪代码可直接用,注意结合上述修正):

---

# 修正后设计规范 — 回填三个致命/严重坑

> 本文档取代原方案正文中对应的三段（删除逻辑、Web 服务鉴权、Codex 计数）。
> 所有路径/inode/软链结构均已在本机实测核对（见每坑「实测锚点」）。凡与原方案冲突处，以本文档为准。

---

## 坑 #1（致命）：删软链连累两端共享源

### 实测锚点（本机核对，非推断）
- `~/.agents` 本身是软链：`~/.agents -> /Users/mac/Desktop/codex/核心数据/.agents`（**第一层间接**）。
- `~/.claude/skills/` 下有 **29 个软链节点**：
  - 28 个 `lark-*` 用**相对**目标 `../../../../.agents/skills/lark-X`，经 `~/.agents` 再跳，最终 realpath 落到共享源 `/Users/mac/Desktop/codex/核心数据/.agents/skills/lark-X`（**双重间接**）。
  - `storage-analyzer` 用**绝对外链** `/Users/mac/Desktop/Claude code/安装/storage-analyzer`。
- `frontend-design` 是**断链**：`os.path.lexists()=True` 但 `os.path.exists()=False`，realpath 落到不存在的 `/Users/mac/Desktop/Claude code/.agents/skills/frontend-design`（`../../` 深度算错）。
- **共享 inode 实测**：链接侧 `lark-doc/SKILL.md` 与源侧 SKILL.md 是同一 inode（`97987053`）。即：对链接侧做「跟随删除」= 直接删源文件，Codex 侧同时爆炸。

### 原设计错在哪
1. 用 `rm -rf <skillDir>` 或 `shutil.rmtree(path)`。当 `path` 是软链目录，`rmtree` 会**跟随进入**并递归删除**共享源里的真实文件**，Codex 全量数据被连累删除。
2. 用 `os.path.exists()` 做「删前存在性校验」——它对断链（frontend-design）返回 `False`，导致「明明有个链接节点要清理，却被判定为不存在而跳过」，坏链永远清不掉。
3. allowlist / writeDeny 用 `~/.agents` 或 `~/.claude/skills` 这类**别名/未解析路径**做前缀匹配。攻击/误删可用相对软链绕过前缀（realpath 落在共享源里，但字符串前缀是 `~/.claude/skills/...`，看起来「在允许区内」）。
4. 只有一个 `delete` 动词，无法区分「我要断开这个链接」和「我要删掉真实内容」。

### 修正规则

**R1. 两个不同的删除动词，语义互斥、不可混用**

| 动词 | 语义 | 允许作用对象 | 底层调用 | 绝对禁止 |
|---|---|---|---|---|
| `delete-link` | 只删「链接节点」本身 | `os.path.islink(path)==True` 的节点 | `os.remove(path)`（对软链 = unlink 该链接项，**不触碰目标**） | 禁止 `realpath` 后再删；禁止 `rmtree`；禁止对非软链调用 |
| `delete-source` | 删真实内容（真目录/真文件） | `os.path.islink(path)==False` 且通过独立源校验 | `shutil.rmtree(path, ...)` 但先断言无软链穿越 | 禁止作用于任何 `islink()==True` 的路径；禁止跟随软链进入 |

- **判定用 `os.path.islink()`（lstat 语义），绝不用 `os.path.exists()`。** `islink()` 对断链（frontend-design）仍返回 `True`，因此坏链可被 `delete-link` 正常清理。
- `delete-link` 处理路径时**全程用非解析路径**（传入什么删什么），**永不 `os.path.realpath()` / `os.readlink()` 后再对目标动手**。readlink 只允许用于「展示这个链接指向哪」，不允许把结果喂给删除函数。

**R2. writeDeny / allowlist 用「完全解析的绝对 realpath」判定，不用别名**

- 删除前把**候选路径**和**每一条 deny 前缀**都先 `os.path.realpath()` 归一化，再做前缀比较。
- deny 列表里写死共享源的**真实绝对路径**（不是 `~/.agents`）：
  - `/Users/mac/Desktop/codex/核心数据/.agents`（Codex 共享源，双重软链的最终落点）
  - `/Users/mac/Desktop/Claude code/安装/storage-analyzer`（storage-analyzer 绝对外链的真实内容）
- 关键不对称规则：
  - **`delete-source` 的候选 realpath 命中任一 deny 前缀 → 硬拒绝**（这是共享/外部真实内容，本工具无权删）。
  - **`delete-link` 不看 realpath 落点**（因为它本来就允许指向共享源），只校验「候选路径的**父目录** realpath 必须落在本侧管理区 `/Users/mac/.claude/skills` 内」——即「我只在自己家门口删软链节点，不去别人家删」。

**R3. 删后断言（post-delete assertion）**

- `delete-link` 删完后断言：
  1. `os.path.lexists(linkPath) == False`（链接节点确已消失）。
  2. 若该链接曾指向共享源，取删前记下的 `sourceRealpath`，断言 `os.path.exists(sourceRealpath) == True` **且** inode 未变（`st_ino` 等于删前快照）。**共享源 inode 仍在 = 没连累到源。** 不成立则立即报「灾难性误删」并中止后续批处理。
- `delete-source` 删完后断言：目标真实路径不存在，且**遍历一遍所有已知软链**，确认没有任何链接节点变成断链（可选，用于删真目录前提示「有 N 个链接会因此断链」）。

### 伪代码

```python
import os, shutil, json

# 共享/外部真实内容：写死「完全解析的绝对 realpath」，绝不用 ~ 别名
WRITE_DENY_REALPATHS = [
    os.path.realpath("/Users/mac/Desktop/codex/核心数据/.agents"),
    os.path.realpath("/Users/mac/Desktop/Claude code/安装/storage-analyzer"),
]
# 本侧唯一允许「摘链接节点」的管理区
LINK_MANAGE_ROOT = os.path.realpath("/Users/mac/.claude/skills")

def _under(child_real: str, parent_real: str) -> bool:
    # 归一化前缀判定，避免 /a/bc 命中 /a/b
    return child_real == parent_real or child_real.startswith(parent_real + os.sep)

def _snapshot_link(path: str) -> dict:
    """删前对软链节点及其目标做快照，供删后断言。仅 readlink，用于展示/断言，不喂给删除。"""
    assert os.path.islink(path), f"not a symlink: {path}"
    target_raw = os.readlink(path)                    # 原始（可能相对）目标
    src_real   = os.path.realpath(path)               # 目标最终落点
    src_exists = os.path.exists(src_real)             # 断链时为 False
    src_ino    = os.stat(src_real).st_ino if src_exists else None
    return {"link": path, "targetRaw": target_raw,
            "srcReal": src_real, "srcExists": src_exists, "srcIno": src_ino}

def delete_link(path: str) -> dict:
    """只删链接节点本身；永不 realpath 跟随；断链也能删。"""
    if not os.path.islink(path):                       # 用 islink(lstat)，不是 exists
        raise ValueError(f"delete-link 只作用于软链节点，但这不是软链: {path}")
    parent_real = os.path.realpath(os.path.dirname(path))
    if not _under(parent_real, LINK_MANAGE_ROOT):
        raise PermissionError(f"delete-link 只能在本侧管理区摘链接: {path}")
    snap = _snapshot_link(path)
    os.remove(path)                                    # unlink 链接项，目标毫发无伤
    # —— 删后断言 ——
    assert not os.path.lexists(path), f"链接节点未消失: {path}"
    if snap["srcExists"]:
        assert os.path.exists(snap["srcReal"]), f"灾难：共享源被连累删除 {snap['srcReal']}"
        assert os.stat(snap["srcReal"]).st_ino == snap["srcIno"], \
            f"灾难：共享源 inode 变了 {snap['srcReal']}"
    return {"action": "delete-link", "ok": True, "snapshot": snap}

def delete_source(path: str) -> dict:
    """删真实内容；拒绝一切软链；拒绝共享/外部真实路径。"""
    if os.path.islink(path):
        raise ValueError(f"delete-source 禁止作用于软链，请改用 delete-link: {path}")
    cand_real = os.path.realpath(path)
    for deny in WRITE_DENY_REALPATHS:
        if _under(cand_real, deny):
            raise PermissionError(f"拒绝：{cand_real} 属于共享/外部真实内容 {deny}")
    # rmtree 遇到内部软链子目录时也不能跟随（onerror 兜底 + 事前扫描）
    for root, dirs, files in os.walk(path):
        for d in dirs:
            sub = os.path.join(root, d)
            if os.path.islink(sub):
                raise RuntimeError(f"目录树内含软链，拒绝整树删除，请人工处理: {sub}")
    shutil.rmtree(path)
    assert not os.path.exists(cand_real)
    return {"action": "delete-source", "ok": True, "path": cand_real}
```

**UI 层规则**：库存扫描时对每个技能节点标注 `nodeType ∈ {realdir, symlink→shared, symlink→external, symlink→broken}`。
- `symlink→shared` / `symlink→external`：删除按钮**只提供 `delete-link`**，文案「断开链接（不删共享源）」。
- `symlink→broken`（frontend-design）：显示「坏链」徽标，一键 `delete-link` 清理。
- `realdir`：才提供 `delete-source`，且弹二次确认列出「删后会断链的 N 个引用」。

---

## 坑 #2（致命）：Web 明文令牌裸奔

### 实测锚点
- 本机防火墙关闭 → 局域网内任意主机可直连本地 Web 服务端口；明文 HTTP 上的 token 会在同网段被嗅探。
- 已确认 `~/.claude/settings.json` 有可用 `UserPromptSubmit` hook（注入交互准则），证明 hook 扩展点可用——鉴权失败告警可复用该机制，无需另造。

### 原设计错在哪
1. 起明文 `http://0.0.0.0:port`，token 走 URL query 或明文 header，同网段可嗅探/重放。
2. token 存 `localStorage` → 持久化、任何同源脚本可读、XSS 直接盗走且长期有效。
3. token 比较用 `a == b` 或 `hmac.compare_digest` 但**未先做长度归一** → 可被 timing/长度侧信道试探。
4. 不校验 `Host` 头 → 易受 DNS 重绑定（DNS rebinding）：外部页面把域名解析到 `127.0.0.1`，用浏览器绕过同源直接打本地服务。
5. 默认绑 `0.0.0.0` 且无网络环境判定，公共 Wi-Fi 下等于裸奔。

### 修正规则

**R1. 传输层：分环境三档，明文仅限 loopback**

| 场景 | 允许 | 强制措施 |
|---|---|---|
| 仅本机 | `http` 但**只绑 `127.0.0.1`**（绝不 `0.0.0.0`） | 默认档 |
| 开 LAN 访问（手机/另一台机） | **必须自签 TLS**，否则拒绝启动 | 证书指纹随二维码一起 pin（见 R2） |
| 公共 Wi-Fi / 未知网络 | **禁止开 LAN**，直接拒绝 `--host` 非 loopback | 建议改走 Tailscale / SSH 隧道 |

- 首选**隧道而非开 LAN**：`ssh -L 8765:127.0.0.1:8765 user@host` 或 Tailscale（`tailscale serve`），此时服务仍只绑 `127.0.0.1`，最安全。
- 开 LAN 走自签 TLS：启动时若 `host != 127.0.0.1` 且未提供证书 → **拒绝启动**并提示生成命令。

**R2. 证书指纹随二维码 pin（防中间人 + 首连信任）**

- 生成自签证书，算 SHA-256 指纹 `fp`。
- 二维码内容 = `https://<lan-ip>:<port>/#t=<sessionToken>&fp=<sha256>`。手机扫码后前端在建连时校验服务端证书指纹 == `fp`，不匹配则拒连并告警（首连即 pin，杜绝同网段中间人换证书）。
- token 放 URL fragment（`#`）而非 query，fragment 不进服务端日志、不随 Referer 外泄。

**R3. token 存储与生命周期：短 TTL session token，禁 localStorage**

- 服务端启动时生成**一次性 session token**（`secrets.token_urlsafe(32)`），带 TTL（默认 15 min，可配），过期即失效、重启即轮换。
- 前端拿到后存 **`sessionStorage`**（标签页关闭即清）或内存变量，**严禁 `localStorage`**。
- Cookie 版另设 `HttpOnly; Secure; SameSite=Strict`，但 API 仍以 `Authorization: Bearer` 为准。

**R4. 常数时间比较：先长度检查，再 `compare_digest`**

```python
import hmac
def token_ok(supplied: str, expected: str) -> bool:
    if supplied is None:
        return False
    sb = supplied.encode(); eb = expected.encode()
    if len(sb) != len(eb):          # 先长度门（长度本身不敏感，但避免 compare_digest 早退泄漏）
        return False
    return hmac.compare_digest(sb, eb)   # 常数时间比较
```

**R5. Host 头校验防 DNS 重绑定**

```python
ALLOWED_HOSTS = {"127.0.0.1", "localhost"}   # 开 LAN 时再追加显式 lan-ip:port

def host_ok(host_header: str) -> bool:
    if not host_header:
        return False
    hostname = host_header.split(":")[0].strip().lower()
    return hostname in ALLOWED_HOSTS
```

- 每个请求先过 `host_ok`，不通过直接 `403`，与 token 校验**同时成立**才放行。
- 再叠加 CSRF：状态改变请求（删除）要求自定义头 `X-Skill-Token`（浏览器跨源不会自动带），双重防 rebinding + CSRF。

**R6. 鉴权失败告警复用 hook**

- 连续 N 次（默认 5）token/Host 校验失败 → 通过既有 hook 通道（`Notification` hook 已在 settings.json 注册）弹本机告警「有人在试探 Skill 管理服务」，并临时封该来源 IP 60s。

### 请求准入流程（伪代码）

```python
def authorize(request) -> "200|403":
    if not host_ok(request.headers.get("Host")):
        return 403                       # 先挡 DNS rebinding
    tok = request.headers.get("Authorization", "").removeprefix("Bearer ").strip()
    if not session_alive():              # TTL 过期
        return 403
    if not token_ok(tok, CURRENT_SESSION_TOKEN):
        bump_fail_counter(request.remote_ip)   # 达阈值触发 hook 告警 + 临时封禁
        return 403
    if request.method in ("POST", "DELETE") and \
       not token_ok(request.headers.get("X-Skill-Token",""), CURRENT_SESSION_TOKEN):
        return 403                       # CSRF 双提交
    return 200
```

---

## 坑 #3（严重）：Codex 计数虚高

### 实测锚点
- 单文件实测：48 处 `SKILL.md` 字面出现 = 真读命令 25 + 正文提及 16 + 系统块幽灵 7。**naive 正则数 48，虚高近一倍。**
- 幽灵引用集中在 `computer-use / pdf / presentations` 这类**系统级能力**（被列在系统块从没真读）；正则会让它们永远「显示用过、永不进可删除列表」，**毁掉核心功能**。
- 性能实测：Codex 1346MB/88 文件全量扫 **2.49s（541MB/s，Python）**；关键优化 `'SKILL' not in line` 预筛，只对含 SKILL 的行 `json.loads`，避开 360 万行逐行解析。→ 原方案「1.3GB 拖慢 UI」担心过度，首扫两端 4–5s 可接受。
- Claude 侧对比信号（此前实测）：显式 `Skill` tool_use，`confidence=exact`（agent-reach 58 / artifact-design 10 / gstack 10）。

### 原设计错在哪
1. 用 naive 正则 `grep -c 'SKILL\.md'` 或 `re.findall(r'skills/(\w+)/SKILL\.md')` 统计 Codex 用量，把「正文提及 + 系统块幽灵」都算成调用，数值虚高近一倍。
2. 系统级能力（computer-use/pdf/presentations）因常驻系统块而恒显「用过」，永远进不了「可删除」候选，删除功能名存实亡。
3. 把 Codex 虚高计数与 Claude exact 信号**混在同一列 confidence**，让用户以为 Codex 计数同样可信。

### 修正规则

**R1. Codex 真调用分类器 = 三条件同时成立**

一条 Codex 日志行计入「真读」当且仅当：
1. 该行 `json.loads` 成功且 `payload.type == "function_call"`（是一次工具调用，不是助手正文、不是系统块）。
2. `payload.arguments`（或其 `command`/`cmd` 字段）里含**读命令**：`sed|cat|head|less|bat|nl|tail`（词边界匹配，避免 `concatenate` 之类误命中）。
3. 同一 `arguments` 里能抓到 `skills/<name>/SKILL.md` 路径，提取 `<name>` 作为被读技能。

- **正文提及**（`type` 是 message/assistant 文本里出现 `SKILL.md`）→ 不计。
- **系统块幽灵**（在 system/instructions 块里被列出但无对应 function_call 读命令）→ 不计。这正是 computer-use/pdf/presentations 能进入「可删除」候选的关键。

**R2. 可删除判定只信 Claude exact，Codex 仅供参考**

- **`deletable` 判定输入只用 Claude `confidence=exact` 的调用信号**（显式 Skill tool_use）。
- Codex 分类器数出的 25 次真读 → 存为 `codexRefCount`，标签 **`仅供参考 (reference-only)`**，**不参与** `deletable` 布尔计算。
- 理由：Codex 侧即便用了正确分类器，也缺 Claude 那种「exact tool_use」的确定性，用它做删除闸门风险仍高；降级为辅助展示。

**R3. UI 上 confidence 分级可视化**

| 列 | 来源 | confidence | 是否进删除闸门 |
|---|---|---|---|
| `claudeExactUses` | Claude Skill tool_use | `exact` | ✅ 唯一闸门 |
| `codexRealReads` | 分类器（R1 三条件） | `heuristic / reference-only` | ❌ 仅展示 |
| ~~`codexNaive`~~ | 已废弃，不展示 | — | — |

- 若某技能 `claudeExactUses==0` 但 `codexRealReads>0` → UI 显示「Claude 侧未见调用，Codex 参考 N 次」，**仍可进可删除候选**（由用户决定），不因 Codex 参考数而屏蔽。

### 分类器伪代码（含性能预筛）

```python
import json, re
READ_CMD = re.compile(r'\b(sed|cat|head|less|bat|nl|tail)\b')
SKILL_PATH = re.compile(r'skills/([A-Za-z0-9_\-]+)/SKILL\.md')

def count_codex_real_reads(log_path: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    with open(log_path, "r", errors="ignore") as f:
        for line in f:
            if 'SKILL' not in line:          # 预筛：跳过 360 万无关行，541MB/s 的关键
                continue
            try:
                obj = json.loads(line)
            except Exception:
                continue
            payload = obj.get("payload", obj)
            if payload.get("type") != "function_call":   # 条件①：必须是工具调用
                continue
            args = payload.get("arguments", "")
            if isinstance(args, dict):
                args = json.dumps(args, ensure_ascii=False)
            if not READ_CMD.search(args):                # 条件②：含读命令
                continue
            for name in SKILL_PATH.findall(args):        # 条件③：抓 skills/<name>/SKILL.md
                counts[name] = counts.get(name, 0) + 1
    return counts   # 单文件实测应得 25，而非 naive 的 48
```

### 数据库 DDL 片段

```sql
CREATE TABLE skill_usage (
    skill_name         TEXT PRIMARY KEY,
    node_type          TEXT NOT NULL,          -- realdir | symlink_shared | symlink_external | symlink_broken
    claude_exact_uses  INTEGER NOT NULL DEFAULT 0,   -- 唯一删除闸门信号
    codex_real_reads   INTEGER NOT NULL DEFAULT 0,   -- R1 分类器，reference-only
    codex_naive_count  INTEGER,                       -- 仅留存审计对比，UI 不展示
    confidence         TEXT NOT NULL                  -- 'exact' | 'reference-only'
        CHECK (confidence IN ('exact','reference-only')),
    -- 可删除只看 claude_exact_uses==0，Codex 参考数不参与
    deletable          INTEGER GENERATED ALWAYS AS (claude_exact_uses = 0) VIRTUAL,
    last_scanned_at    TEXT NOT NULL
);

-- 可删除候选视图：Codex 参考数不屏蔽候选，仅作展示注释
CREATE VIEW deletable_candidates AS
SELECT skill_name, node_type, codex_real_reads,
       CASE WHEN codex_real_reads > 0
            THEN 'Claude 未见调用；Codex 参考 ' || codex_real_reads || ' 次'
            ELSE '两端均未见调用' END AS note
FROM skill_usage
WHERE deletable = 1;
```

---

## 三坑联动的一条硬约束（实现时务必贯穿）

删除动作（坑#1 的 `delete-link`/`delete-source`）必须**同时满足坑#2 的鉴权**（Host+token+CSRF 三重）与**坑#3 的删除闸门**（`deletable = claude_exact_uses==0`）才执行：

```
执行删除  ⇔  authorize()==200  ∧  skill.deletable==1  ∧  按 node_type 选对动词(delete-link|delete-source)  ∧  删后断言全过
```

任一不成立即中止并回滚该项，不影响批处理其余项。
