// SkillHub 桌面端 —— Tauri 后端
// 架构铁律:前端(WebView)零文件系统权限,所有磁盘访问收敛到下面的 command。
// Phase 1 只暴露只读能力(scan_catalog / scan_stats)。删除/更新等写操作留待带 consent gate 后再加。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use std::time::Instant;

#[derive(Serialize)]
struct CatalogResponse {
    generated_at_ms: u128,
    scan_duration_ms: u128,
    counts: Counts,
    skills: Vec<scanner_core::SkillCard>,
}

#[derive(Serialize, Default)]
struct Counts {
    total: usize,
    claude: usize,
    codex: usize,
    ok: usize,
    broken: usize,
    unused: usize,
    used: usize,
}

/// 只读:扫描全部技能 + 合并调用统计,返回给 UI
#[tauri::command]
fn scan_catalog() -> CatalogResponse {
    let t0 = Instant::now();
    let cards = scanner_core::build_catalog();
    let dur = t0.elapsed().as_millis();

    let mut c = Counts::default();
    c.total = cards.len();
    for card in &cards {
        match card.skill.engine.as_str() {
            "claude" => c.claude += 1,
            "codex" => c.codex += 1,
            _ => {}
        }
        if matches!(card.skill.health, scanner_core::model::Health::Ok) {
            c.ok += 1;
        } else {
            c.broken += 1;
        }
        if card.call_count == 0 {
            c.unused += 1;
        } else {
            c.used += 1;
        }
    }

    CatalogResponse {
        generated_at_ms: now_ms(),
        scan_duration_ms: dur,
        counts: c,
        skills: cards,
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

// ============ 删除功能(consent gate)============
// 流程:前端点删除 → preview_delete(展示确认框内容)→ 用户确认 → confirm_delete(真删)
// 后端第一行就校验路径,前端无法绕过。

use std::path::Path;

/// 只读预检:告诉 UI 该用哪个动词、能不能删、要警告什么
#[tauri::command]
fn preview_delete(path: String) -> scanner_core::deleter::DeletePreview {
    scanner_core::deleter::preview(Path::new(&path))
}

/// 执行删除。confirmed 必须为 true(consent gate:前端确认框点了确认才传 true)
#[tauri::command]
fn confirm_delete(path: String, confirmed: bool) -> scanner_core::deleter::DeleteResult {
    if !confirmed {
        return scanner_core::deleter::DeleteResult {
            ok: false,
            action: "denied".into(),
            target: path,
            trashed: false,
            source_safe: None,
            message: "未确认,已取消".into(),
        };
    }
    scanner_core::deleter::delete(Path::new(&path))
}

// ============ 优先级打标(用户元数据,重扫不覆盖)============

#[derive(Serialize)]
struct MetaOpResult {
    ok: bool,
    message: String,
}

/// 设优先级:high | normal | low
#[tauri::command]
fn set_priority(engine: String, slug: String, priority: String) -> MetaOpResult {
    let mut store = scanner_core::usermeta::MetaStore::load();
    let key = scanner_core::usermeta::key_for(&engine, &slug);
    match store.set_priority(&key, &priority) {
        Ok(_) => MetaOpResult { ok: true, message: format!("{} → {}", slug, priority) },
        Err(e) => MetaOpResult { ok: false, message: format!("保存失败: {}", e) },
    }
}

/// 设启用/禁用
#[tauri::command]
fn set_enabled(engine: String, slug: String, enabled: bool) -> MetaOpResult {
    let mut store = scanner_core::usermeta::MetaStore::load();
    let key = scanner_core::usermeta::key_for(&engine, &slug);
    match store.set_enabled(&key, enabled) {
        Ok(_) => MetaOpResult { ok: true, message: "已保存".into() },
        Err(e) => MetaOpResult { ok: false, message: format!("保存失败: {}", e) },
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            scan_catalog,
            preview_delete,
            confirm_delete,
            set_priority,
            set_enabled
        ])
        .run(tauri::generate_context!())
        .expect("启动 SkillHub 失败");
}
