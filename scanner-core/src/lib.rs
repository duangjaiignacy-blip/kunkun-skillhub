//! scanner-core 作为库:桌面端(Tauri)与 Web 端(axum)共用的扫描内核
pub mod model;
pub mod inventory;
pub mod usage;
pub mod deleter;
pub mod usermeta;

use model::{Skill, UsageStat};
use std::collections::HashMap;
use std::path::PathBuf;

fn home() -> PathBuf {
    dirs::home_dir().expect("找不到 home 目录")
}

/// 扫描全部技能(inventory)
pub fn scan_all_skills() -> Vec<Skill> {
    let h = home();
    let mut all = Vec::new();
    all.extend(inventory::scan_root(&h.join(".claude/skills"), "claude", "claude_user"));
    all.extend(inventory::scan_root(&h.join(".codex/skills"), "codex", "codex_user"));
    all.extend(inventory::scan_root(&h.join(".agents/skills"), "codex", "agents_shared"));
    let plugin_root = h.join(".claude/plugins/cache");
    if plugin_root.is_dir() {
        for mp in walkdir::WalkDir::new(&plugin_root).max_depth(4).into_iter().flatten() {
            if mp.file_type().is_dir() && mp.file_name() == "skills" {
                let src = format!("plugin:{}", mp.path().display());
                all.extend(inventory::scan_root(mp.path(), "claude", &src));
            }
        }
    }
    all
}

/// 扫描全部调用统计,按 (engine,slug) 返回
pub fn scan_all_usage() -> Vec<UsageStat> {
    let h = home();
    let (claude, _) = usage::scan_claude_usage(&h.join(".claude/projects"));
    let (codex, _) = usage::scan_codex_usage(&h.join(".codex/sessions"));
    claude.into_iter().chain(codex.into_iter()).collect()
}

/// 一个合成给 UI 的技能视图(inventory + usage + 用户元数据 合并)
#[derive(serde::Serialize)]
pub struct SkillCard {
    #[serde(flatten)]
    pub skill: Skill,
    pub call_count: u32,
    pub confidence: String,
    pub first_used: Option<String>,
    pub last_used: Option<String>,
    // 用户元数据(与客观数据分开存,重扫不覆盖)
    pub priority: String,   // high | normal | low
    pub enabled: bool,
    pub note: String,
}

/// UI 直接消费:合并 inventory + usage + 用户元数据,按 slug+engine 对齐
pub fn build_catalog() -> Vec<SkillCard> {
    let skills = scan_all_skills();
    let usage = scan_all_usage();
    let meta = usermeta::MetaStore::load();
    let mut umap: HashMap<(String, String), &UsageStat> = HashMap::new();
    for u in &usage {
        umap.insert((u.engine.clone(), u.slug.clone()), u);
    }
    skills
        .into_iter()
        .map(|s| {
            let key = (s.engine.clone(), s.slug.clone());
            let u = umap.get(&key);
            let um = meta.get(&usermeta::key_for(&s.engine, &s.slug));
            SkillCard {
                call_count: u.map(|x| x.call_count).unwrap_or(0),
                confidence: u.map(|x| x.confidence.clone()).unwrap_or_else(|| "none".into()),
                first_used: u.and_then(|x| x.first_used.clone()),
                last_used: u.and_then(|x| x.last_used.clone()),
                priority: um.priority,
                enabled: um.enabled,
                note: um.note,
                skill: s,
            }
        })
        .collect()
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    #[test]
    fn catalog_matches_baseline() {
        let cards = build_catalog();
        assert!(cards.len() >= 170, "技能数应 >=170, 实际 {}", cards.len());
        // agent-reach 应有 58 次 exact
        let ar = cards.iter().find(|c| c.skill.slug=="agent-reach" && c.skill.engine=="claude");
        assert!(ar.is_some(), "找不到 agent-reach");
        let ar = ar.unwrap();
        // 调用次数是活数据(日志实时增长),基线是 ">=58"(6-14 首次那批),不是死等 58
        assert!(ar.call_count >= 58, "agent-reach 应 >=58 次(活数据), 实际 {}", ar.call_count);
        assert_eq!(ar.confidence, "exact");
        // 有未使用技能(call_count=0)
        let unused = cards.iter().filter(|c| c.call_count==0).count();
        assert!(unused > 0, "应有未使用技能");
        // 坏技能存在
        let broken = cards.iter().filter(|c| !matches!(c.skill.health, model::Health::Ok)).count();
        assert_eq!(broken, 2, "应有 2 个坏技能, 实际 {}", broken);
        println!("✅ catalog: {} 技能, agent-reach>={}, {} 未使用, {} 坏", cards.len(), ar.call_count, unused, broken);
    }
}
