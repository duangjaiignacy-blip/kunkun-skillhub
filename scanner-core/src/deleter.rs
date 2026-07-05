//! 删除模块 —— 项目风险最高的部分,严格按 docs/03-fix-backfill.md 的双动词方案
//!
//! 铁律:
//! 1. 软链只删「链接节点」本身,永不 realpath 跟随(否则连累两端共享源)。
//! 2. 用 symlink_metadata(lstat)判类型,不用 exists(断链也要能删)。
//! 3. 删后断言:若曾指向共享源,断言源 inode 仍在。
//! 4. 一律移废纸篓(可撤销),绝不 rm。
//! 5. writeDeny 用完全解析的绝对 realpath 判定,不用别名。

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// 删除结果
#[derive(Debug, Serialize)]
pub struct DeleteResult {
    pub ok: bool,
    pub action: String,        // delete-link | delete-source
    pub target: String,        // 实际删除的路径
    pub trashed: bool,         // 是否成功进废纸篓
    pub source_safe: Option<bool>, // 共享源是否安然无恙(delete-link 时校验)
    pub message: String,
}

/// 删除前的预检信息(给 UI 展示确认框用)
#[derive(Debug, Serialize)]
pub struct DeletePreview {
    pub allowed: bool,
    pub verb: String,          // delete-link | delete-source | denied
    pub link_target: Option<String>,
    pub warning: Option<String>,
    pub is_shared: bool,
}

/// 硬黑名单:共享/外部真实内容的绝对 realpath 前缀。delete-source 命中即拒。
fn deny_realpaths() -> Vec<PathBuf> {
    let h = dirs::home_dir().unwrap_or_default();
    // ~/.agents 是软链 → 共享源;canonicalize 拿真实落点
    let mut v = Vec::new();
    if let Ok(agents) = fs::canonicalize(h.join(".agents")) {
        v.push(agents);
    }
    v
}

/// 本侧唯一允许「摘链接节点」的管理区(运行时动态求值,不写死)
fn link_manage_roots() -> Vec<PathBuf> {
    let h = dirs::home_dir().unwrap_or_default();
    let mut v = Vec::new();
    for p in [".claude/skills", ".codex/skills"] {
        if let Ok(rp) = fs::canonicalize(h.join(p)) {
            v.push(rp);
        }
    }
    v
}

fn under(child: &Path, parent: &Path) -> bool {
    // 归一化后前缀判定;macOS 大小写不敏感 → casefold 比较
    let c = child.to_string_lossy().to_lowercase();
    let p = parent.to_string_lossy().to_lowercase();
    c == p || c.starts_with(&format!("{}/", p))
}

/// 预检:决定这个路径该用哪个动词、能不能删、要不要警告
pub fn preview(path: &Path) -> DeletePreview {
    let is_link = fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);

    if is_link {
        let target = fs::read_link(path).ok().map(|p| p.to_string_lossy().into_owned());
        // 软链 → 只允许 delete-link,且必须在管理区内
        let parent_real = path.parent().and_then(|p| fs::canonicalize(p).ok());
        let in_manage = parent_real
            .as_ref()
            .map(|pr| link_manage_roots().iter().any(|r| under(pr, r)))
            .unwrap_or(false);
        // 是否指向共享源
        let is_shared = fs::canonicalize(path)
            .ok()
            .map(|rp| deny_realpaths().iter().any(|d| under(&rp, d)))
            .unwrap_or(false);
        return DeletePreview {
            allowed: in_manage,
            verb: if in_manage { "delete-link".into() } else { "denied".into() },
            link_target: target,
            warning: if is_shared {
                Some("这是软链,指向 Claude/Codex 共享源。删除只移除链接,不动共享内容。".into())
            } else {
                Some("这是软链,删除只移除链接节点本身。".into())
            },
            is_shared,
        };
    }

    // 真实目录/文件 → delete-source,但要过 denylist
    let cand_real = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    for deny in deny_realpaths() {
        if under(&cand_real, &deny) {
            return DeletePreview {
                allowed: false,
                verb: "denied".into(),
                link_target: None,
                warning: Some(format!("拒绝:{} 属于共享/外部真实内容,本工具无权删除。", cand_real.display())),
                is_shared: true,
            };
        }
    }
    DeletePreview {
        allowed: true,
        verb: "delete-source".into(),
        link_target: None,
        warning: Some("这是真实目录,将连同内容一起移到废纸篓(可从废纸篓恢复)。".into()),
        is_shared: false,
    }
}

/// 执行删除。内部据类型自动选 delete-link / delete-source。
pub fn delete(path: &Path) -> DeleteResult {
    let is_link = fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    if is_link {
        delete_link(path)
    } else {
        delete_source(path)
    }
}

/// 只删链接节点本身;永不 realpath 跟随;断链也能删。
fn delete_link(path: &Path) -> DeleteResult {
    // 校验:必须真是软链
    let is_link = fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    if !is_link {
        return err("delete-link", path, "目标不是软链");
    }
    // 校验:父目录必须在管理区
    let parent_real = match path.parent().and_then(|p| fs::canonicalize(p).ok()) {
        Some(pr) => pr,
        None => return err("delete-link", path, "无法解析父目录"),
    };
    if !link_manage_roots().iter().any(|r| under(&parent_real, r)) {
        return err("delete-link", path, "链接不在允许的管理区内");
    }

    // 删前快照:记录共享源 realpath + inode(仅用于删后断言,不喂给删除函数)
    let src_real = fs::canonicalize(path).ok();
    let src_ino_before = src_real.as_ref().and_then(|p| inode_of(p));

    // 执行:移废纸篓(对软链 = 把链接项移进废纸篓,不触碰目标)
    // trash::delete 对软链的行为是移动链接本身,不跟随
    match trash::delete(path) {
        Ok(_) => {}
        Err(e) => return err("delete-link", path, &format!("移废纸篓失败: {}", e)),
    }

    // 删后断言:链接节点已消失
    if fs::symlink_metadata(path).is_ok() {
        return err("delete-link", path, "链接节点仍存在(删除未生效)");
    }
    // 断言:共享源安然无恙
    let mut source_safe = None;
    if let (Some(src), Some(ino_before)) = (&src_real, src_ino_before) {
        let still_exists = src.exists();
        let ino_after = inode_of(src);
        let safe = still_exists && ino_after == Some(ino_before);
        source_safe = Some(safe);
        if !safe {
            return DeleteResult {
                ok: false,
                action: "delete-link".into(),
                target: path.to_string_lossy().into_owned(),
                trashed: true,
                source_safe: Some(false),
                message: format!("⚠️ 灾难:共享源可能被连累 {}", src.display()),
            };
        }
    }

    DeleteResult {
        ok: true,
        action: "delete-link".into(),
        target: path.to_string_lossy().into_owned(),
        trashed: true,
        source_safe,
        message: "已断开链接(移到废纸篓,共享源未受影响)".into(),
    }
}

/// 删真实内容;拒绝一切软链;拒绝共享/外部真实路径。
fn delete_source(path: &Path) -> DeleteResult {
    if fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return err("delete-source", path, "禁止对软链用 delete-source");
    }
    let cand_real = match fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => return err("delete-source", path, "无法解析路径"),
    };
    for deny in deny_realpaths() {
        if under(&cand_real, &deny) {
            return err("delete-source", path, "属于共享/外部真实内容,拒绝删除");
        }
    }
    // 移废纸篓(trash crate 对目录树不跟随内部软链)
    match trash::delete(path) {
        Ok(_) => DeleteResult {
            ok: true,
            action: "delete-source".into(),
            target: cand_real.to_string_lossy().into_owned(),
            trashed: true,
            source_safe: None,
            message: "已移到废纸篓(可恢复)".into(),
        },
        Err(e) => err("delete-source", path, &format!("移废纸篓失败: {}", e)),
    }
}

fn inode_of(p: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(p).ok().map(|m| m.ino())
}

fn err(action: &str, path: &Path, msg: &str) -> DeleteResult {
    DeleteResult {
        ok: false,
        action: action.into(),
        target: path.to_string_lossy().into_owned(),
        trashed: false,
        source_safe: None,
        message: msg.into(),
    }
}
