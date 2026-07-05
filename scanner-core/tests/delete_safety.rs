// 删除安全测试:验证致命坑「删软链连累共享源」已被堵住
use scanner_core::deleter;
use std::fs;
use std::os::unix::fs::MetadataExt;

#[test]
fn delete_link_never_touches_source() {
    // 造场景:临时目录里一个"共享源"文件 + 一个软链指向它
    let tmp = std::env::temp_dir().join(format!("skillhub_del_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let source = tmp.join("shared_source.txt");
    fs::write(&source, b"PRECIOUS SHARED CONTENT").unwrap();
    let source_ino = fs::metadata(&source).unwrap().ino();

    let link = tmp.join("my_symlink");
    std::os::unix::fs::symlink(&source, &link).unwrap();

    // 预检:应识别为软链
    let pv = deleter::preview(&link);
    println!("preview: verb={} is_shared={}", pv.verb, pv.is_shared);

    // 删软链
    let res = deleter::delete(&link);
    println!("delete: ok={} action={} msg={}", res.ok, res.action, res.message);
    assert_eq!(res.action, "delete-link", "软链必须走 delete-link");

    // === 核心断言:共享源必须安然无恙 ===
    assert!(source.exists(), "❌ 灾难:共享源被删了!");
    let ino_after = fs::metadata(&source).unwrap().ino();
    assert_eq!(ino_after, source_ino, "❌ 共享源 inode 变了");
    let content = fs::read(&source).unwrap();
    assert_eq!(content, b"PRECIOUS SHARED CONTENT", "❌ 共享源内容被改");

    println!("✅ 删软链后,共享源文件、inode、内容全部完好 —— 致命坑已堵住");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn delete_source_rejects_symlink() {
    let tmp = std::env::temp_dir().join(format!("skillhub_reject_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let target = tmp.join("real");
    fs::create_dir_all(&target).unwrap();
    let link = tmp.join("lnk");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    // preview 对软链应给 delete-link,不是 delete-source
    let pv = deleter::preview(&link);
    assert_ne!(pv.verb, "delete-source", "软链不能被判为 delete-source");
    println!("✅ 软链被正确判为 {},不会走 delete-source", pv.verb);
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn delete_link_in_real_manage_root_succeeds_and_spares_source() {
    // 在真实管理区 ~/.claude/skills 下造临时软链,验证删成功且源安全
    let home = dirs::home_dir().unwrap();
    let manage = home.join(".claude/skills");
    if !manage.is_dir() { eprintln!("跳过:无 ~/.claude/skills"); return; }

    // 假源放在临时目录(模拟共享源)
    let tmp = std::env::temp_dir().join(format!("skillhub_realtest_src_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let source = tmp.join("fake_shared_skill");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), b"---\nname: fake\ndescription: test\n---\nPRECIOUS").unwrap();
    let src_ino = fs::metadata(&source).unwrap().ino();

    // 在管理区造软链指向假源
    let link = manage.join(format!("__skillhub_test_link_{}", std::process::id()));
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(&source, &link).unwrap();

    // 预检应允许 delete-link
    let pv = deleter::preview(&link);
    println!("real preview: verb={} allowed={}", pv.verb, pv.allowed);
    assert!(pv.allowed, "管理区内的软链应允许删除");
    assert_eq!(pv.verb, "delete-link");

    // 真删
    let res = deleter::delete(&link);
    println!("real delete: ok={} source_safe={:?} msg={}", res.ok, res.source_safe, res.message);
    assert!(res.ok, "删除应成功: {}", res.message);
    assert_eq!(res.source_safe, Some(true), "共享源应被判定安全");

    // 核心断言:假源完好
    assert!(source.exists(), "❌ 源被连累删除");
    assert_eq!(fs::metadata(&source).unwrap().ino(), src_ino, "❌ 源 inode 变了");
    assert!(source.join("SKILL.md").exists(), "❌ 源内容丢失");
    println!("✅ 管理区内删软链成功,假共享源完好无损");

    // 清理:软链已进废纸篓,清临时源
    let _ = fs::remove_file(&link); // 若残留
    let _ = fs::remove_dir_all(&tmp);
}
