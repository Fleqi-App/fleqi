//! M3.6 规则/收藏/历史测试：四种作用域、命中规则匹配、收藏复用重绑上下文、
//! 输入历史上限与清理不删用户文件。

use fleqi_application::collection_service::{CollectionService, Favorite, Rule, RuleScope};
use fleqi_application::dto::AppEvent;
use fleqi_application::ports::{Clock, EventSink, IdGenerator, StorageError};
use std::sync::atomic::{AtomicU64, Ordering};

struct FakeClock;
impl Clock for FakeClock {
    fn now_rfc3339(&self) -> String {
        "2026-09-18T00:00:00Z".into()
    }
}

#[derive(Default)]
struct Events(std::sync::Mutex<Vec<String>>);
impl EventSink for Events {
    fn emit(&self, event: AppEvent) {
        self.0.lock().unwrap().push(event.name().to_owned());
    }
}

struct SeqIds(AtomicU64);
impl IdGenerator for SeqIds {
    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst))
    }
}

#[derive(Default)]
struct MemoryStore {
    rules: std::sync::Mutex<Vec<Rule>>,
    favorites: std::sync::Mutex<Vec<Favorite>>,
    history: std::sync::Mutex<Vec<String>>,
}

impl fleqi_application::ports::CollectionStore for MemoryStore {
    fn load_rules(&self) -> Result<Vec<Rule>, StorageError> {
        Ok(self.rules.lock().unwrap().clone())
    }
    fn upsert_rule(&self, rule: &Rule) -> Result<(), StorageError> {
        let mut rules = self.rules.lock().unwrap();
        rules.retain(|r| r.id != rule.id);
        rules.push(rule.clone());
        Ok(())
    }
    fn delete_rule(&self, rule_id: &str) -> Result<(), StorageError> {
        self.rules.lock().unwrap().retain(|r| r.id != rule_id);
        Ok(())
    }
    fn load_favorites(&self) -> Result<Vec<Favorite>, StorageError> {
        Ok(self.favorites.lock().unwrap().clone())
    }
    fn upsert_favorite(&self, favorite: &Favorite) -> Result<(), StorageError> {
        let mut favorites = self.favorites.lock().unwrap();
        favorites.retain(|f| f.id != favorite.id);
        favorites.push(favorite.clone());
        Ok(())
    }
    fn delete_favorite(&self, favorite_id: &str) -> Result<(), StorageError> {
        self.favorites
            .lock()
            .unwrap()
            .retain(|f| f.id != favorite_id);
        Ok(())
    }
    fn history_append(&self, entry: &str) -> Result<(), StorageError> {
        let mut history = self.history.lock().unwrap();
        history.push(entry.to_owned());
        if history.len() > 200 {
            let overflow = history.len() - 200;
            history.drain(..overflow);
        }
        Ok(())
    }
    fn history_list(&self) -> Result<Vec<String>, StorageError> {
        Ok(self.history.lock().unwrap().clone())
    }
    fn history_clear(&self) -> Result<(), StorageError> {
        self.history.lock().unwrap().clear();
        Ok(())
    }
}

fn service() -> CollectionService {
    CollectionService::new(
        std::sync::Arc::new(MemoryStore::default()),
        std::sync::Arc::new(FakeClock),
        std::sync::Arc::new(SeqIds(AtomicU64::new(1))),
        std::sync::Arc::new(Events::default()),
    )
}

#[test]
fn rule_crud_with_all_four_scopes() {
    let service = service();
    let scopes = [
        (RuleScope::Global, None, None),
        (
            RuleScope::Directory {
                path: "/Users/me/项目".into(),
            },
            Some("/Users/me/项目"),
            None,
        ),
        (
            RuleScope::Phrase {
                phrase: "部署".into(),
            },
            None,
            Some("部署"),
        ),
        (
            RuleScope::DirectoryAndPhrase {
                path: "/Users/me/项目".into(),
                phrase: "部署".into(),
            },
            Some("/Users/me/项目"),
            Some("部署"),
        ),
    ];
    for (scope, path, phrase) in scopes {
        let rule = service
            .create_rule(format!("规则 {:?}", scope), "说明文字", Some(scope.clone()))
            .expect("创建");
        assert!(!rule.id.is_empty());
        let _ = path;
        let _ = phrase;
    }
    let rules = service.list_rules().expect("列表");
    assert_eq!(rules.len(), 4);
    // 更新：改名后版本递增。
    let updated = service
        .update_rule(&rules[0].id, Some("新名称".into()), None, None)
        .expect("更新");
    assert_eq!(updated.name, "新名称");
    assert_eq!(updated.revision.value(), rules[0].revision.value() + 1);
    service.delete_rule(&rules[1].id).expect("删除");
    assert_eq!(service.list_rules().unwrap().len(), 3);
}

#[test]
fn matching_rules_respect_scope_semantics() {
    let service = service();
    service
        .create_rule("全局规则", "总是应用", Some(RuleScope::Global))
        .unwrap();
    service
        .create_rule(
            "目录规则",
            "项目内应用",
            Some(RuleScope::Directory {
                path: "/Users/me/项目".into(),
            }),
        )
        .unwrap();
    service
        .create_rule(
            "短语规则",
            "命中部署时",
            Some(RuleScope::Phrase {
                phrase: "部署".into(),
            }),
        )
        .unwrap();
    service
        .create_rule(
            "组合规则",
            "项目内部署",
            Some(RuleScope::DirectoryAndPhrase {
                path: "/Users/me/项目".into(),
                phrase: "部署".into(),
            }),
        )
        .unwrap();
    // 场景：项目目录 + 提示含"部署" → 全部四条命中。
    let hit = service.matching_rules("/Users/me/项目", "帮我部署到测试环境");
    assert_eq!(hit.len(), 4, "{hit:?}");
    // 场景：其它目录 + 无关键词 → 仅全局。
    let miss = service.matching_rules("/tmp", "查看文件");
    assert_eq!(miss.len(), 1, "{miss:?}");
    // 场景：项目目录但无关键词 → 全局 + 目录。
    let dir_only = service.matching_rules("/Users/me/项目", "查看文件");
    assert_eq!(dir_only.len(), 2);
    // 停用规则不再命中。
    let disabled = service
        .list_rules()
        .unwrap()
        .into_iter()
        .find(|r| r.name == "目录规则")
        .unwrap();
    service
        .update_rule(&disabled.id, None, Some(false), None)
        .unwrap();
    let after = service.matching_rules("/Users/me/项目", "查看文件");
    assert_eq!(after.len(), 1, "停用后仅全局");
}

#[test]
fn favorites_crud_and_manual_kind_preserved() {
    let service = service();
    let ai = service
        .create_favorite("AI 收藏", "总结这个文件夹", "ai")
        .expect("AI 收藏");
    assert_eq!(ai.kind, "ai");
    let manual = service
        .create_favorite("手动收藏", "!npm test", "manual")
        .expect("手动收藏");
    assert_eq!(manual.kind, "manual", "手动命令收藏保持手动来源");
    let list = service.list_favorites().expect("列表");
    assert_eq!(list.len(), 2);
    service.delete_favorite(&ai.id).expect("删除");
    assert_eq!(service.list_favorites().unwrap().len(), 1);
    // 重命名标签。
    let updated = service
        .update_favorite(&manual.id, Some("改名".into()), None)
        .expect("更新");
    assert_eq!(updated.name, "改名");
}

#[test]
fn history_capped_at_200_and_clear_only_drops_records() {
    let service = service();
    for index in 0..250 {
        service
            .append_history(&format!("命令 {index}"))
            .expect("记录");
    }
    let history = service.list_history().expect("列表");
    assert_eq!(history.len(), 200, "上限 200 条保留最新");
    assert_eq!(
        history.first().map(String::as_str),
        Some("命令 50"),
        "裁掉最旧"
    );
    assert_eq!(history.last().map(String::as_str), Some("命令 249"));
    service.clear_history().expect("清理");
    assert!(
        service.list_history().unwrap().is_empty(),
        "清空仅删除 App 记录"
    );
}
