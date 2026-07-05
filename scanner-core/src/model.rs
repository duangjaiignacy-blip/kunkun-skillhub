//! 统一数据模型 —— inventory / usage 的产物结构

use serde::Serialize;

/// 技能健康状态(实测发现的坏技能分类)
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Ok,
    NoFrontmatter,   // douyindashi:无 YAML 头,但仍是有效技能
    BrokenSymlink,   // frontend-design:断链
    Unreadable,      // SKILL.md 缺失或读不了
    YamlError,       // frontmatter 解析失败
}

/// 技能安装形态 —— 决定删除策略(见 03-fix-backfill)
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    RealDir,          // 真实目录:可 delete-source
    SymlinkShared,    // 软链 → 共享源:只可 delete-link
    SymlinkExternal,  // 软链 → 绝对外链(storage-analyzer)
    SymlinkBroken,    // 断链:建议清理
}

/// 一条技能记录。主键 = (engine, slug, source_root),防同名跨源串味。
#[derive(Debug, Clone, Serialize)]
pub struct Skill {
    pub slug: String,             // 目录名
    pub engine: String,           // claude | codex
    pub source: String,           // claude_user | codex_user | agents_shared | plugin:*
    pub source_root: String,      // 归一化的托管根 realpath(身份的一部分)
    pub display_name: Option<String>,   // frontmatter.name
    pub description: Option<String>,    // frontmatter.description(一句话)
    pub function_cn: String,       // 中文功能说明(前端简介/详情用)
    pub summary_cn: String,        // 统一中文简介:功能 + 适配软件
    pub suited_software: String,   // Codex | Claude Code | Claude Code / Desktop
    pub source_kind_cn: String,    // 本机用户技能 | 共享技能库 | 插件缓存 | 外部链接
    pub scope_level: String,       // global | project | plugin | external
    pub scope_level_cn: String,    // 全局 | 项目级 | 插件级 | 外部
    pub source_url: Option<String>, // frontmatter/metadata 声明的原始下载/来源链接
    pub update_url: Option<String>, // frontmatter/metadata 声明的更新链接
    pub installed_at_ms: Option<u128>, // 本地安装/下载时间(目录创建时间或入口修改时间)
    pub updated_at_ms: Option<u128>,   // 本地更新时间(SKILL.md 或技能目录修改时间)
    pub install_path: String,     // 入口路径(未解析)
    pub resolved_path: String,    // canonicalize 后
    pub node_type: NodeType,
    pub health: Health,
    pub health_detail: Option<String>,
    pub link_target: Option<String>,
    // 实测发现要接住的字段
    pub triggers_raw: Option<String>,   // frontmatter.triggers 序列化 JSON(主动提醒用)
    pub declared_version: Option<String>,
    pub has_chain_to: bool,
}

/// per-skill 调用统计
#[derive(Debug, Clone, Serialize)]
pub struct UsageStat {
    pub slug: String,
    pub engine: String,
    pub call_count: u32,
    pub confidence: String,       // exact(Claude 显式) | inferred(Codex 读取)
    pub first_used: Option<String>,
    pub last_used: Option<String>,
}
