//! 调用统计 —— Claude 精确(显式 Skill tool_use)+ Codex 推断(function_call 读命令)
//! 关键性能优化(实测 541MB/s):每行先做 `contains("SKILL"/"Skill")` 预筛,再 json 解析。

use crate::model::UsageStat;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// 内部累加器
#[derive(Default, Clone)]
struct Acc {
    count: u32,
    first: Option<String>,
    last: Option<String>,
}

impl Acc {
    fn bump(&mut self, ts: Option<&str>) {
        self.count += 1;
        if let Some(day) = ts.map(|t| &t[..t.len().min(10)]) {
            let day = day.to_string();
            if self.first.as_ref().map_or(true, |f| &day < f) {
                self.first = Some(day.clone());
            }
            if self.last.as_ref().map_or(true, |l| &day > l) {
                self.last = Some(day);
            }
        }
    }
}

/// Claude 侧:扫 ~/.claude/projects/**/*.jsonl,数显式 Skill tool_use(confidence=exact)
pub fn scan_claude_usage(projects_dir: &Path) -> (Vec<UsageStat>, ScanMeta) {
    let mut acc: HashMap<String, Acc> = HashMap::new();
    let mut meta = ScanMeta::default();

    for entry in WalkDir::new(projects_dir).into_iter().flatten() {
        if entry.file_type().is_file()
            && entry.path().extension().map_or(false, |e| e == "jsonl")
        {
            meta.files += 1;
            if let Ok(content) = fs::read_to_string(entry.path()) {
                meta.bytes += content.len() as u64;
                for line in content.lines() {
                    meta.lines += 1;
                    // 预筛:没有 Skill 字样直接跳过,避免无谓 json 解析
                    if !line.contains("\"name\":\"Skill\"") {
                        continue;
                    }
                    parse_claude_line(line, &mut acc);
                }
            }
        }
    }

    let stats = acc
        .into_iter()
        .map(|(slug, a)| UsageStat {
            slug,
            engine: "claude".into(),
            call_count: a.count,
            confidence: "exact".into(),
            first_used: a.first,
            last_used: a.last,
        })
        .collect();
    (stats, meta)
}

fn parse_claude_line(line: &str, acc: &mut HashMap<String, Acc>) {
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return,
    };
    let ts = v.get("timestamp").and_then(|t| t.as_str());
    let content = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array());
    if let Some(blocks) = content {
        for b in blocks {
            if b.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                && b.get("name").and_then(|n| n.as_str()) == Some("Skill")
            {
                if let Some(skill) = b
                    .get("input")
                    .and_then(|i| i.get("skill"))
                    .and_then(|s| s.as_str())
                {
                    if !skill.is_empty() {
                        acc.entry(skill.to_string()).or_default().bump(ts);
                    }
                }
            }
        }
    }
}

/// Codex 侧:扫 ~/.codex/sessions/**/*.jsonl,数 function_call 里的读命令(confidence=inferred)
/// 验证过的分类器:payload.type==function_call && arguments 含 sed/cat/head... && /skills/<name>/SKILL.md
pub fn scan_codex_usage(sessions_dir: &Path) -> (Vec<UsageStat>, ScanMeta) {
    let mut acc: HashMap<String, Acc> = HashMap::new();
    let mut meta = ScanMeta::default();

    for entry in WalkDir::new(sessions_dir).into_iter().flatten() {
        if entry.file_type().is_file()
            && entry.path().extension().map_or(false, |e| e == "jsonl")
        {
            meta.files += 1;
            if let Ok(content) = fs::read_to_string(entry.path()) {
                meta.bytes += content.len() as u64;
                for line in content.lines() {
                    meta.lines += 1;
                    // 预筛:关键性能点
                    if !line.contains("SKILL") {
                        continue;
                    }
                    parse_codex_line(line, &mut acc);
                }
            }
        }
    }

    let stats = acc
        .into_iter()
        .map(|(slug, a)| UsageStat {
            slug,
            engine: "codex".into(),
            call_count: a.count,
            confidence: "inferred".into(),
            first_used: a.first,
            last_used: a.last,
        })
        .collect();
    (stats, meta)
}

const READ_CMDS: [&str; 7] = ["sed", "cat", "head", "less", "bat", "nl", "tail"];

fn parse_codex_line(line: &str, acc: &mut HashMap<String, Acc>) {
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return,
    };
    let payload = match v.get("payload") {
        Some(p) => p,
        None => return,
    };
    // 只认 function_call(验证过的真调用形态)
    if payload.get("type").and_then(|t| t.as_str()) != Some("function_call") {
        return;
    }
    let args = payload.get("arguments");
    let args_str = match args {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => return,
    };
    // 必须是读命令
    if !READ_CMDS.iter().any(|c| word_present(&args_str, c)) {
        return;
    }
    let ts = v.get("timestamp").and_then(|t| t.as_str());
    // 抓 /skills/<name>/SKILL.md
    for slug in extract_skill_slugs(&args_str) {
        acc.entry(slug).or_default().bump(ts);
    }
}

/// 词边界匹配 "sed" 等命令,避免 "based" 误命中
fn word_present(haystack: &str, word: &str) -> bool {
    let bytes = haystack.as_bytes();
    let wb = word.as_bytes();
    let mut i = 0;
    while let Some(pos) = haystack[i..].find(word) {
        let start = i + pos;
        let end = start + wb.len();
        let before_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let after_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        i = start + 1;
    }
    false
}

/// 从字符串里抽出所有 /skills/<slug>/SKILL.md 的 slug(不用正则库,手写扫描)
fn extract_skill_slugs(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let pat = "/skills/";
    let mut i = 0;
    while let Some(pos) = s[i..].find(pat) {
        let start = i + pos + pat.len();
        // slug 到下一个 /
        if let Some(slash) = s[start..].find('/') {
            let slug = &s[start..start + slash];
            let rest = &s[start + slash..];
            if rest.starts_with("/SKILL.md")
                && !slug.is_empty()
                && slug.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
            {
                out.push(slug.to_string());
            }
            i = start + slash;
        } else {
            break;
        }
    }
    out
}

/// 扫描元信息(性能/统计)
#[derive(Default)]
pub struct ScanMeta {
    pub files: u32,
    pub lines: u64,
    pub bytes: u64,
}
