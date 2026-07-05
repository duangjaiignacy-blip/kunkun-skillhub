//! 用户元数据存储 —— 优先级/备注/启用状态
//!
//! 架构铁律:用户手打的标签与扫描出的客观数据【分开存】。
//! 重扫只覆盖客观字段(description/health/call_count),绝不触碰这里。
//! 存在应用私有目录(~/Library/Application Support/SkillHub/),不在被管理的技能目录里。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// 单个技能的用户元数据。key = "engine:slug"(稳定主键,跨源不串)
/// 注意:手动实现 Default,让 enabled 默认为 true。
/// #[derive(Default)] 会用 bool 的默认 false,那会导致 or_default() 把新技能误设为禁用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMeta {
    #[serde(default = "default_priority")]
    pub priority: String, // high | normal | low
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub updated_at_ms: u128,
}

impl Default for UserMeta {
    fn default() -> Self {
        UserMeta {
            priority: "normal".into(),
            enabled: true, // 关键:默认启用,不是 bool 的 false
            note: String::new(),
            updated_at_ms: 0,
        }
    }
}

fn default_priority() -> String {
    "normal".into()
}
fn default_true() -> bool {
    true
}

/// 整个用户元数据库(内存镜像 + 落盘)
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MetaStore {
    #[serde(default)]
    pub items: HashMap<String, UserMeta>,
}

/// 存储文件路径:应用私有目录,不在技能目录里(避免自指)
fn store_path() -> PathBuf {
    let base = dirs::data_dir() // ~/Library/Application Support on macOS
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"));
    base.join("SkillHub").join("user_meta.json")
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

impl MetaStore {
    /// 从磁盘加载(不存在则空库)
    pub fn load() -> Self {
        let p = store_path();
        match fs::read_to_string(&p) {
            Ok(txt) => serde_json::from_str(&txt).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// 落盘(原子写:先写临时文件再 rename,防并发/中断损坏)
    pub fn save(&self) -> std::io::Result<()> {
        let p = store_path();
        if let Some(dir) = p.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = p.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        fs::write(&tmp, json)?;
        fs::rename(&tmp, &p)?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> UserMeta {
        self.items.get(key).cloned().unwrap_or_default()
    }

    /// 设优先级(high/normal/low),自动落盘
    pub fn set_priority(&mut self, key: &str, priority: &str) -> std::io::Result<()> {
        let m = self.items.entry(key.to_string()).or_default();
        m.priority = match priority {
            "high" | "low" | "normal" => priority.to_string(),
            _ => "normal".to_string(),
        };
        m.updated_at_ms = now_ms();
        self.save()
    }

    /// 设启用/禁用
    pub fn set_enabled(&mut self, key: &str, enabled: bool) -> std::io::Result<()> {
        let m = self.items.entry(key.to_string()).or_default();
        m.enabled = enabled;
        m.updated_at_ms = now_ms();
        self.save()
    }

    /// 写备注
    pub fn set_note(&mut self, key: &str, note: &str) -> std::io::Result<()> {
        let m = self.items.entry(key.to_string()).or_default();
        m.note = note.to_string();
        m.updated_at_ms = now_ms();
        self.save()
    }
}

/// 稳定主键:engine:slug
pub fn key_for(engine: &str, slug: &str) -> String {
    format!("{}:{}", engine, slug)
}
