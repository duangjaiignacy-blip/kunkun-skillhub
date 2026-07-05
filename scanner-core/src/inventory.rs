//! 技能发现(Discovery)—— 用真 YAML 解析器,不用正则(实测:正则会误杀嵌套 YAML)

use crate::model::{Health, NodeType, Skill};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 扫描一个根目录下的所有技能条目
pub fn scan_root(root: &Path, engine: &str, source: &str) -> Vec<Skill> {
    let mut out = Vec::new();
    let root_real = fs::canonicalize(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| root.to_string_lossy().into_owned());

    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return out,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        // 用 symlink_metadata(lstat 语义)判类型,不跟随
        let lmeta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let is_link = lmeta.file_type().is_symlink();
        // 普通文件(llms.txt 等)跳过;只处理目录或软链
        if !is_link && !lmeta.is_dir() {
            continue;
        }

        let skill = build_skill(&path, &name, engine, source, &root_real, is_link);
        out.push(skill);
    }
    out
}

fn build_skill(
    path: &Path,
    slug: &str,
    engine: &str,
    source: &str,
    source_root: &str,
    is_link: bool,
) -> Skill {
    let install_path = path.to_string_lossy().into_owned();
    let link_target = if is_link {
        fs::read_link(path).ok().map(|p| p.to_string_lossy().into_owned())
    } else {
        None
    };

    // 断链检测:软链但 canonicalize 失败/目标不存在
    let resolved = fs::canonicalize(path);
    let (node_type, resolved_path, broken) = classify_node(path, is_link, &link_target, &resolved);
    let installed_at_ms = install_time_ms(path);
    let updated_at_ms = update_time_ms(path, &resolved_path);
    let (scope_level, scope_level_cn) = scope_level(source, source_root, &resolved_path);

    let mut skill = Skill {
        slug: slug.to_string(),
        engine: engine.to_string(),
        source: source.to_string(),
        source_root: source_root.to_string(),
        display_name: None,
        description: None,
        function_cn: String::new(),
        summary_cn: String::new(),
        suited_software: suited_software(engine, source),
        source_kind_cn: source_kind(source),
        scope_level,
        scope_level_cn,
        source_url: None,
        update_url: None,
        installed_at_ms,
        updated_at_ms,
        install_path,
        resolved_path: resolved_path.clone(),
        node_type,
        health: Health::Ok,
        health_detail: None,
        link_target,
        triggers_raw: None,
        declared_version: None,
        has_chain_to: false,
    };

    if broken {
        skill.health = Health::BrokenSymlink;
        skill.health_detail = Some("断链:软链目标不可达(相对深度可能算错)".into());
        fill_chinese_summary(&mut skill);
        return skill;
    }

    // 解析 SKILL.md
    let skill_md = PathBuf::from(&resolved_path).join("SKILL.md");
    parse_frontmatter(&skill_md, &mut skill);
    fill_chinese_summary(&mut skill);
    skill
}

fn classify_node(
    path: &Path,
    is_link: bool,
    link_target: &Option<String>,
    resolved: &std::io::Result<PathBuf>,
) -> (NodeType, String, bool) {
    if !is_link {
        let rp = resolved
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string_lossy().into_owned());
        return (NodeType::RealDir, rp, false);
    }
    // 软链:目标不可达 = 断链
    match resolved {
        Err(_) => {
            let rp = path.to_string_lossy().into_owned();
            (NodeType::SymlinkBroken, rp, true)
        }
        Ok(rp) => {
            let rp_str = rp.to_string_lossy().into_owned();
            // 绝对外链 vs 指向共享源
            let is_absolute = link_target
                .as_ref()
                .map(|t| t.starts_with('/'))
                .unwrap_or(false);
            let nt = if is_absolute {
                NodeType::SymlinkExternal
            } else {
                NodeType::SymlinkShared
            };
            (nt, rp_str, false)
        }
    }
}

fn install_time_ms(path: &Path) -> Option<u128> {
    let meta = fs::symlink_metadata(path).ok()?;
    meta.created()
        .or_else(|_| meta.modified())
        .ok()
        .and_then(system_time_ms)
}

fn update_time_ms(path: &Path, resolved_path: &str) -> Option<u128> {
    let skill_md = PathBuf::from(resolved_path).join("SKILL.md");
    modified_time_ms(&skill_md)
        .or_else(|| modified_time_ms(Path::new(resolved_path)))
        .or_else(|| modified_time_ms(path))
}

fn modified_time_ms(path: &Path) -> Option<u128> {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(system_time_ms)
}

fn system_time_ms(t: SystemTime) -> Option<u128> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis())
}

/// 用真 YAML 解析器读 frontmatter。关键:不能用正则(会误杀嵌套 metadata)。
fn parse_frontmatter(skill_md: &Path, skill: &mut Skill) {
    let txt = match fs::read_to_string(skill_md) {
        Ok(t) => t,
        Err(_) => {
            skill.health = Health::Unreadable;
            skill.health_detail = Some("SKILL.md 缺失或不可读".into());
            return;
        }
    };
    // 去 BOM
    let txt = txt.strip_prefix('\u{feff}').unwrap_or(&txt);
    if !txt.starts_with("---") {
        // douyindashi 情况:无 frontmatter 但仍有效,description 回退首个标题
        skill.health = Health::NoFrontmatter;
        skill.description = fallback_desc(txt);
        skill.display_name = Some(skill.slug.clone());
        return;
    }
    // 按 --- 切三段
    let parts: Vec<&str> = txt.splitn(3, "---").collect();
    if parts.len() < 3 {
        skill.health = Health::YamlError;
        skill.health_detail = Some("frontmatter 分隔符不完整".into());
        return;
    }
    let yaml_body = parts[1];
    match serde_yaml::from_str::<serde_yaml::Value>(yaml_body) {
        Ok(val) => {
            skill.display_name = val.get("name").and_then(|v| v.as_str()).map(String::from);
            skill.description = val
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string());
            skill.source_url = first_url_for_keys(
                &val,
                &[
                    "source",
                    "source_url",
                    "sourceUrl",
                    "repository",
                    "repo",
                    "homepage",
                    "download",
                ],
            )
            .or_else(|| first_url_for_keys(&val, &["docs"]));
            skill.update_url = first_url_for_keys(
                &val,
                &["update", "update_url", "updateUrl", "releases", "changelog"],
            );
            skill.declared_version = val
                .get("version")
                .and_then(|v| v.as_str())
                .map(String::from);
            skill.has_chain_to = val.get("chainTo").is_some();
            // triggers 原样存 JSON(主动提醒的金矿)
            if let Some(trig) = val.get("triggers") {
                skill.triggers_raw = serde_json::to_string(trig).ok();
            }
            skill.health = Health::Ok;
        }
        Err(e) => {
            skill.health = Health::YamlError;
            skill.health_detail = Some(format!("YAML 解析失败: {}", e));
        }
    }
}

/// 无 frontmatter 时,用首个 # 标题或首段做 description 回退
fn fallback_desc(txt: &str) -> Option<String> {
    for line in txt.lines() {
        let l = line.trim();
        if let Some(h) = l.strip_prefix("# ") {
            return Some(h.trim().to_string());
        }
        if !l.is_empty() && !l.starts_with('#') {
            return Some(l.chars().take(80).collect());
        }
    }
    None
}

fn suited_software(engine: &str, source: &str) -> String {
    if source.starts_with("plugin:") {
        "Claude Code / Desktop".into()
    } else {
        match engine {
            "codex" => "Codex".into(),
            "claude" => "Claude Code".into(),
            _ => "未知".into(),
        }
    }
}

fn source_kind(source: &str) -> String {
    if source.starts_with("plugin:") {
        "插件缓存".into()
    } else {
        match source {
            "claude_user" | "codex_user" => "本机用户技能".into(),
            "agents_shared" => "共享技能库".into(),
            _ => "本机来源".into(),
        }
    }
}

fn scope_level(source: &str, source_root: &str, resolved_path: &str) -> (String, String) {
    if source.starts_with("plugin:") {
        return ("plugin".into(), "插件级".into());
    }

    let source_root_path = Path::new(source_root);
    let resolved = Path::new(resolved_path);
    if under(resolved, source_root_path) {
        return ("global".into(), "全局".into());
    }

    if looks_like_project_skill_path(resolved_path) {
        ("project".into(), "项目级".into())
    } else {
        ("external".into(), "外部".into())
    }
}

fn looks_like_project_skill_path(path: &str) -> bool {
    ["/.codex/skills/", "/.claude/skills/", "/.agents/skills/"]
        .iter()
        .any(|needle| path.contains(needle))
}

fn under(child: &Path, parent: &Path) -> bool {
    child == parent || child.starts_with(parent)
}

fn fill_chinese_summary(skill: &mut Skill) {
    let desc = skill.description.as_deref().unwrap_or("");
    skill.function_cn = function_summary_cn(&skill.slug, desc, &skill.health);
    skill.summary_cn = format!(
        "功能：{}；适配：{}",
        skill.function_cn, skill.suited_software
    );
}

fn function_summary_cn(slug: &str, desc: &str, health: &Health) -> String {
    if *health == Health::BrokenSymlink {
        return "技能链接已失效，可查看来源位置并清理断链".into();
    }

    let slug_l = slug.to_ascii_lowercase();
    let desc_l = desc.to_ascii_lowercase();

    let known = [
        ("using-superpowers", "会话开始时检查并加载适用技能"),
        ("verification-before-completion", "完成前执行验证，避免误报已完成"),
        ("brainstorming", "澄清需求、比较方案并形成设计"),
        ("systematic-debugging", "按步骤定位问题根因并验证修复"),
        ("frontend-design", "生成高质量前端界面与交互"),
        ("agent-reach", "联网搜索、调研与资料提取"),
        ("anysearch", "实时搜索、垂直检索与网页内容提取"),
        ("playwright", "浏览器自动化、截图与页面验证"),
        ("imagegen", "生成或编辑图片资产"),
    ];
    for (needle, summary) in known {
        if slug_l.contains(needle) {
            return summary.into();
        }
    }

    let keyword_rules = [
        ("lark-", "处理飞书消息、文档、日程或审批等工作流"),
        ("gmail", "处理 Gmail 邮件搜索、总结与回复草稿"),
        ("google-", "处理 Google Drive、Docs、Sheets 或 Slides"),
        ("github", "处理 GitHub 仓库、PR、Issue 或 CI 工作"),
        ("netlify", "处理 Netlify 配置、部署与函数能力"),
        ("pdf", "读取、生成、渲染或校验 PDF 文件"),
        ("presentation", "创建、编辑或导出演示文稿"),
        ("slides", "创建、编辑或导出演示文稿"),
        ("spreadsheet", "创建、编辑或分析电子表格"),
        ("document", "创建、编辑或整理文档内容"),
        ("browser", "控制浏览器进行页面操作和检查"),
        ("chrome", "控制 Chrome 浏览器完成页面任务"),
        ("image", "生成、编辑或整理视觉图片资产"),
        ("skill", "创建、安装或管理 Codex/Claude 技能"),
        ("debug", "定位问题、分析错误并验证修复"),
        ("test", "编写、运行或验证测试流程"),
        ("deploy", "部署项目并检查发布结果"),
        ("search", "搜索资料并提取关键信息"),
        ("research", "调研主题并整理结论"),
    ];
    for (needle, summary) in keyword_rules {
        if slug_l.contains(needle) || desc_l.contains(needle) {
            return summary.into();
        }
    }

    if contains_cjk(desc) {
        return shorten_cn(desc);
    }

    format!("提供 {} 相关自动化能力", slug.replace(['-', '_'], " "))
}

fn contains_cjk(s: &str) -> bool {
    s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

fn shorten_cn(s: &str) -> String {
    let compact = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for ch in compact.chars().take(42) {
        out.push(ch);
    }
    out
}

fn first_url_for_keys(val: &serde_yaml::Value, wanted: &[&str]) -> Option<String> {
    match val {
        serde_yaml::Value::Mapping(map) => {
            for (k, v) in map {
                if let Some(key) = k.as_str() {
                    if wanted.iter().any(|w| key.eq_ignore_ascii_case(w)) {
                        if let Some(url) = first_url(v) {
                            return Some(url);
                        }
                    }
                }
            }
            for (_, v) in map {
                if let Some(url) = first_url_for_keys(v, wanted) {
                    return Some(url);
                }
            }
            None
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                if let Some(url) = first_url_for_keys(item, wanted) {
                    return Some(url);
                }
            }
            None
        }
        _ => None,
    }
}

fn first_url(val: &serde_yaml::Value) -> Option<String> {
    match val {
        serde_yaml::Value::String(s) => {
            if is_url(s) {
                Some(s.to_string())
            } else {
                None
            }
        }
        serde_yaml::Value::Sequence(seq) => seq.iter().find_map(first_url),
        serde_yaml::Value::Mapping(map) => {
            for (_, v) in map {
                if let Some(url) = first_url(v) {
                    return Some(url);
                }
            }
            None
        }
        _ => None,
    }
}

fn is_url(s: &str) -> bool {
    s.starts_with("https://") || s.starts_with("http://")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_root_exposes_chinese_summary_and_source_links() {
        let tmp = std::env::temp_dir().join(format!(
            "skillhub_inventory_meta_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        let skill_dir = tmp.join("demo-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: demo-skill
description: Use when checking source metadata and update links.
metadata:
  docs:
    - "https://example.com/demo-docs"
  source: "https://github.com/example/demo-skill"
  update: "https://github.com/example/demo-skill/releases"
---

# Demo
"#,
        )
        .unwrap();

        let skills = scan_root(&tmp, "codex", "codex_user");
        let skill = skills.iter().find(|s| s.slug == "demo-skill").unwrap();

        assert_eq!(skill.suited_software, "Codex");
        assert_eq!(skill.scope_level, "global");
        assert_eq!(skill.scope_level_cn, "全局");
        assert!(skill.summary_cn.contains("功能："));
        assert!(skill.summary_cn.contains("适配：Codex"));
        assert_eq!(
            skill.source_url.as_deref(),
            Some("https://github.com/example/demo-skill")
        );
        assert_eq!(
            skill.update_url.as_deref(),
            Some("https://github.com/example/demo-skill/releases")
        );
        assert!(
            skill.installed_at_ms.is_some(),
            "应提供技能下载/安装时间"
        );
        assert!(
            skill.updated_at_ms.is_some(),
            "应提供技能更新时间"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    #[cfg(unix)]
    fn scan_root_marks_symlinked_project_skill_scope() {
        let tmp = std::env::temp_dir().join(format!(
            "skillhub_inventory_scope_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        let global_root = tmp.join("global");
        let project_skill = tmp
            .join("project")
            .join(".codex")
            .join("skills")
            .join("project-demo");
        fs::create_dir_all(&global_root).unwrap();
        fs::create_dir_all(&project_skill).unwrap();
        fs::write(
            project_skill.join("SKILL.md"),
            r#"---
name: project-demo
description: Project scoped demo skill.
---

# Project demo
"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(&project_skill, global_root.join("project-demo")).unwrap();

        let skills = scan_root(&global_root, "codex", "codex_user");
        let skill = skills.iter().find(|s| s.slug == "project-demo").unwrap();

        assert_eq!(skill.scope_level, "project");
        assert_eq!(skill.scope_level_cn, "项目级");

        let _ = fs::remove_dir_all(&tmp);
    }
}
