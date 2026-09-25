//! 短生命周期能力口令：计划只保存不透明引用，明文不写 Run/历史/日志。
use crate::ports::{IdGenerator, SequenceIds};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};
pub const PREFIX: &str = "secret-ref:";
struct Vault {
    values: Mutex<HashMap<String, String>>,
    ids: SequenceIds,
}
fn vault() -> &'static Vault {
    static VAULT: OnceLock<Vault> = OnceLock::new();
    VAULT.get_or_init(|| Vault {
        values: Mutex::new(HashMap::new()),
        ids: SequenceIds::new(),
    })
}
pub fn store(value: String) -> String {
    let id = format!("{PREFIX}{}", vault().ids.next_id("capability"));
    vault()
        .values
        .lock()
        .expect("secrets")
        .insert(id.clone(), value);
    id
}
pub fn resolve(reference: &str) -> Result<String, String> {
    vault()
        .values
        .lock()
        .expect("secrets")
        .get(reference)
        .cloned()
        .ok_or_else(|| "本次口令已释放或应用已重启，请重新输入".into())
}
pub fn release(reference: &str) {
    vault().values.lock().expect("secrets").remove(reference);
}
pub fn release_plan(plan: &fleqi_domain::execution::ExecutionPlan) {
    for step in &plan.steps {
        for arg in &step.args {
            if let Ok(values) = serde_json::from_str::<HashMap<String, String>>(arg) {
                for value in values.values() {
                    if value.starts_with(PREFIX) {
                        release(value);
                    }
                }
            }
        }
    }
}
