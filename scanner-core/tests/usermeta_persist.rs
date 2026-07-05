// 验证核心保证:用户打的优先级标签,重扫不覆盖
use scanner_core::usermeta::{MetaStore, key_for};

#[test]
fn priority_survives_rescan() {
    // 用真实存在的技能 key
    let key = key_for("claude", "agent-reach");

    // 1. 设高优先级
    let mut store = MetaStore::load();
    store.set_priority(&key, "high").unwrap();

    // 2. 重新加载(模拟重启/重扫:build_catalog 内部会 MetaStore::load)
    let reloaded = MetaStore::load();
    let m = reloaded.get(&key);
    assert_eq!(m.priority, "high", "优先级应持久化,实际 {}", m.priority);
    println!("✅ 设 high 后重新加载仍是 high");

    // 3. build_catalog(真正的重扫路径)应带上这个优先级
    let cards = scanner_core::build_catalog();
    let ar = cards.iter().find(|c| c.skill.slug=="agent-reach" && c.skill.engine=="claude").unwrap();
    assert_eq!(ar.priority, "high", "build_catalog 应保留用户优先级");
    // 同时客观数据(调用次数)也还在
    assert!(ar.call_count >= 58, "客观数据不受影响");
    println!("✅ 重扫后:优先级=high 保留,调用次数=58 客观数据也在 —— 两者分开存生效");

    // 4. 复原(测试幂等,不污染真实状态)
    let mut s = MetaStore::load();
    s.set_priority(&key, "normal").unwrap();
    println!("✅ 已复原为 normal");
}
