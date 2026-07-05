//! skillscan —— Phase 0 CLI:证明 inventory + usage 三条数据链能稳定产出正确数字
//! 用法:skillscan inventory | usage | all [--json]

mod inventory;
mod model;
mod usage;

use model::{Health, Skill, UsageStat};
use std::path::PathBuf;
use std::time::Instant;

fn home() -> PathBuf {
    dirs::home_dir().expect("找不到 home 目录")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("all");
    let json = args.iter().any(|a| a == "--json");

    match cmd {
        "inventory" => run_inventory(json),
        "usage" => run_usage(json),
        "all" => {
            run_inventory(json);
            run_usage(json);
        }
        _ => {
            eprintln!("用法: skillscan [inventory|usage|all] [--json]");
            std::process::exit(1);
        }
    }
}

fn collect_inventory() -> Vec<Skill> {
    let h = home();
    let mut all = Vec::new();
    all.extend(inventory::scan_root(
        &h.join(".claude/skills"),
        "claude",
        "claude_user",
    ));
    all.extend(inventory::scan_root(
        &h.join(".codex/skills"),
        "codex",
        "codex_user",
    ));
    all.extend(inventory::scan_root(
        &h.join(".agents/skills"),
        "codex",
        "agents_shared",
    ));
    // 插件技能
    let plugin_glob = h.join(".claude/plugins/cache");
    if plugin_glob.is_dir() {
        for mp in walkdir::WalkDir::new(&plugin_glob)
            .max_depth(4)
            .into_iter()
            .flatten()
        {
            if mp.file_type().is_dir() && mp.file_name() == "skills" {
                let src = format!("plugin:{}", mp.path().display());
                all.extend(inventory::scan_root(mp.path(), "claude", &src));
            }
        }
    }
    all
}

fn run_inventory(json: bool) {
    let t0 = Instant::now();
    let skills = collect_inventory();
    let dur = t0.elapsed();

    if json {
        println!("{}", serde_json::to_string_pretty(&skills).unwrap());
        return;
    }

    let total = skills.len();
    let ok = skills.iter().filter(|s| s.health == Health::Ok).count();
    let broken: Vec<&Skill> = skills
        .iter()
        .filter(|s| s.health != Health::Ok)
        .collect();
    let with_name = skills
        .iter()
        .filter(|s| s.display_name.is_some())
        .count();
    let with_desc = skills.iter().filter(|s| s.description.is_some()).count();
    let with_triggers = skills.iter().filter(|s| s.triggers_raw.is_some()).count();

    println!("=== INVENTORY ({:.2?}) ===", dur);
    println!("技能总数:        {}", total);
    println!("health=ok:       {}", ok);
    println!("有 name:         {}", with_name);
    println!("有 description:  {}", with_desc);
    println!("有 triggers:     {}", with_triggers);
    println!("\n非 ok 技能:");
    for s in &broken {
        println!(
            "  [{:?}] {} ({}) {}",
            s.health,
            s.slug,
            s.source,
            s.health_detail.as_deref().unwrap_or("")
        );
    }

    // === 回归基线校验(对拍实测) ===
    println!("\n--- 回归基线校验 ---");
    check("name+description 全覆盖(ok 技能)", with_name >= ok && with_desc >= ok);
    check("真坏技能数应为 2(douyindashi + frontend-design)",
        broken.iter().filter(|s|
            matches!(s.health, Health::NoFrontmatter | Health::BrokenSymlink)
        ).count() == 2);
}

fn run_usage(json: bool) {
    let h = home();
    let t0 = Instant::now();
    let (claude, cmeta) = usage::scan_claude_usage(&h.join(".claude/projects"));
    let claude_dur = t0.elapsed();

    let t1 = Instant::now();
    let (codex, xmeta) = usage::scan_codex_usage(&h.join(".codex/sessions"));
    let codex_dur = t1.elapsed();

    if json {
        let combined: Vec<&UsageStat> = claude.iter().chain(codex.iter()).collect();
        println!("{}", serde_json::to_string_pretty(&combined).unwrap());
        return;
    }

    let claude_total: u32 = claude.iter().map(|s| s.call_count).sum();
    let codex_total: u32 = codex.iter().map(|s| s.call_count).sum();

    println!("\n=== USAGE ===");
    println!(
        "Claude: {} 文件 / {:.0}MB / {} 行 / {:.2?} → {} 技能 {} 次(exact)",
        cmeta.files, cmeta.bytes as f64 / 1e6, cmeta.lines, claude_dur,
        claude.len(), claude_total
    );
    println!(
        "Codex:  {} 文件 / {:.0}MB / {} 行 / {:.2?} → {} 技能 {} 次(inferred)",
        xmeta.files, xmeta.bytes as f64 / 1e6, xmeta.lines, codex_dur,
        codex.len(), codex_total
    );
    println!("Codex 吞吐: {:.0} MB/s", xmeta.bytes as f64 / 1e6 / codex_dur.as_secs_f64().max(0.001));

    let mut cl = claude.clone();
    cl.sort_by(|a, b| b.call_count.cmp(&a.call_count));
    println!("\nClaude TOP:");
    for s in cl.iter().take(6) {
        println!("  {:28} {:3} 次  ({}~{})", s.slug, s.call_count,
            s.first_used.as_deref().unwrap_or("?"), s.last_used.as_deref().unwrap_or("?"));
    }
    let mut cx = codex.clone();
    cx.sort_by(|a, b| b.call_count.cmp(&a.call_count));
    println!("Codex TOP:");
    for s in cx.iter().take(6) {
        println!("  {:28} {:3} 次", s.slug, s.call_count);
    }

    println!("\n--- 回归基线校验 ---");
    // 调用日志是活数据:只能用已验证的历史下限,不能死等旧快照。
    let agent_reach = claude.iter().find(|s| s.slug == "agent-reach").map(|s| s.call_count).unwrap_or(0);
    let has_codex_using = codex.iter().any(|s| s.slug == "using-superpowers" && s.call_count > 0);
    check("Claude agent-reach >= 58(活数据基线)", agent_reach_baseline_passes(agent_reach));
    check("Claude 总调用 >= 116(实测基线)", claude_total >= 116);
    check("Codex using-superpowers 存在且 > 0", has_codex_using);
}

fn agent_reach_baseline_passes(count: u32) -> bool {
    count >= 58
}

fn check(name: &str, pass: bool) {
    println!("  {} {}", if pass { "✅" } else { "❌" }, name);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_baseline_accepts_live_counts_above_original_floor() {
        assert!(agent_reach_baseline_passes(59));
    }

    #[test]
    fn usage_baseline_rejects_counts_below_original_floor() {
        assert!(!agent_reach_baseline_passes(57));
    }
}
