//! CollectionService（M3.6；FR-RULE/FR-FAV）：四种规则作用域与命中匹配、
//! 收藏（AI/手动来源保持）、输入历史（200 条上限、清理只删记录）。

use crate::dto::{AppError, AppEvent, AppResult};
use crate::ports::{Clock, CollectionStore, EventSink, IdGenerator};
use fleqi_domain::revision::Revision;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RuleScope {
    Global,
    Directory { path: String },
    Phrase { phrase: String },
    DirectoryAndPhrase { path: String, phrase: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub content: String,
    pub enabled: bool,
    pub scope: RuleScope,
    pub revision: Revision,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Favorite {
    pub id: String,
    pub name: String,
    /// ai | manual：手动命令收藏保持手动来源（FR-FAV-002）。
    pub kind: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

pub struct CollectionService {
    store: Arc<dyn CollectionStore>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGenerator>,
    events: Arc<dyn EventSink>,
}

impl CollectionService {
    pub fn new(
        store: Arc<dyn CollectionStore>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        events: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            store,
            clock,
            ids,
            events,
        }
    }

    // ---------- 规则 ----------

    pub fn create_rule(
        &self,
        name: impl Into<String>,
        content: impl Into<String>,
        scope: Option<RuleScope>,
    ) -> AppResult<Rule> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(AppError::validation(vec![
                fleqi_domain::settings::FieldError {
                    field: "name".into(),
                    code: "invalid".into(),
                    message: "规则名称不能为空".into(),
                },
            ]));
        }
        let valid_scope = match scope.as_ref() {
            Some(RuleScope::Directory { path }) => std::path::Path::new(path).is_absolute(),
            Some(RuleScope::Phrase { phrase }) => !phrase.trim().is_empty(),
            Some(RuleScope::DirectoryAndPhrase { path, phrase }) => {
                std::path::Path::new(path).is_absolute() && !phrase.trim().is_empty()
            }
            _ => true,
        };
        if !valid_scope {
            return Err(AppError::unavailable("目录范围须为绝对路径，短语不能为空"));
        }
        let now = self.clock.now_rfc3339();
        let rule = Rule {
            id: self.ids.next_id("rule"),
            name,
            content: content.into(),
            enabled: true,
            scope: scope.unwrap_or(RuleScope::Global),
            revision: Revision::new(1),
            created_at: now.clone(),
            updated_at: now,
        };
        self.store
            .upsert_rule(&rule)
            .map_err(|e| AppError::storage(e.to_string()))?;
        self.events.emit(AppEvent::RulesChanged {
            revision: rule.revision,
        });
        Ok(rule)
    }

    pub fn list_rules(&self) -> AppResult<Vec<Rule>> {
        self.store
            .load_rules()
            .map_err(|e| AppError::storage(e.to_string()))
    }

    /// 部分更新：名称/启用/内容；版本递增。
    pub fn update_rule(
        &self,
        rule_id: &str,
        name: Option<String>,
        enabled: Option<bool>,
        content: Option<String>,
    ) -> AppResult<Rule> {
        let mut rules = self.list_rules()?;
        let rule = rules
            .iter_mut()
            .find(|rule| rule.id == rule_id)
            .ok_or_else(|| AppError::not_found(format!("规则 {rule_id} 不存在")))?;
        if let Some(name) = name {
            if name.trim().is_empty() {
                return Err(AppError::validation(vec![
                    fleqi_domain::settings::FieldError {
                        field: "name".into(),
                        code: "invalid".into(),
                        message: "规则名称不能为空".into(),
                    },
                ]));
            }
            rule.name = name;
        }
        if let Some(enabled) = enabled {
            rule.enabled = enabled;
        }
        if let Some(content) = content {
            rule.content = content;
        }
        rule.revision = rule.revision.next();
        rule.updated_at = self.clock.now_rfc3339();
        let updated = rule.clone();
        self.store
            .upsert_rule(&updated)
            .map_err(|e| AppError::storage(e.to_string()))?;
        self.events.emit(AppEvent::RulesChanged {
            revision: updated.revision,
        });
        Ok(updated)
    }

    pub fn delete_rule(&self, rule_id: &str) -> AppResult<()> {
        self.store
            .delete_rule(rule_id)
            .map_err(|e| AppError::storage(e.to_string()))?;
        self.events.emit(AppEvent::RulesChanged {
            revision: Revision::new(0),
        });
        Ok(())
    }

    /// 命中规则（FR-RULE-001/002）：目录/短语/组合按各自条件；停用不命中。
    /// 规则不能越过用户当前请求或执行策略（计划形成时注入；策略判定在 RunService）。
    pub fn matching_rules(&self, directory: &str, prompt: &str) -> Vec<Rule> {
        let mut matched = Vec::new();
        for rule in self.list_rules().unwrap_or_default() {
            if !rule.enabled {
                continue;
            }
            let hit = match &rule.scope {
                RuleScope::Global => true,
                RuleScope::Directory { path } => path == directory,
                RuleScope::Phrase { phrase } => prompt.contains(phrase.as_str()),
                RuleScope::DirectoryAndPhrase { path, phrase } => {
                    path == directory && prompt.contains(phrase.as_str())
                }
            };
            if hit {
                matched.push(rule);
            }
        }
        matched
    }

    // ---------- 收藏 ----------

    pub fn create_favorite(
        &self,
        name: impl Into<String>,
        content: impl Into<String>,
        kind: impl Into<String>,
    ) -> AppResult<Favorite> {
        let kind = kind.into();
        if kind != "ai" && kind != "manual" {
            return Err(AppError::validation(vec![
                fleqi_domain::settings::FieldError {
                    field: "kind".into(),
                    code: "invalid".into(),
                    message: "收藏来源必须是 ai 或 manual".into(),
                },
            ]));
        }
        let favorite = Favorite {
            id: self.ids.next_id("favorite"),
            name: name.into(),
            kind,
            content: content.into(),
            tags: Vec::new(),
            created_at: self.clock.now_rfc3339(),
            last_used_at: None,
        };
        self.store
            .upsert_favorite(&favorite)
            .map_err(|e| AppError::storage(e.to_string()))?;
        self.events.emit(AppEvent::FavoritesChanged {
            revision: Revision::new(1),
        });
        Ok(favorite)
    }

    pub fn list_favorites(&self) -> AppResult<Vec<Favorite>> {
        self.store
            .load_favorites()
            .map_err(|e| AppError::storage(e.to_string()))
    }

    pub fn update_favorite(
        &self,
        favorite_id: &str,
        name: Option<String>,
        tags: Option<Vec<String>>,
    ) -> AppResult<Favorite> {
        let mut favorites = self.list_favorites()?;
        let favorite = favorites
            .iter_mut()
            .find(|favorite| favorite.id == favorite_id)
            .ok_or_else(|| AppError::not_found(format!("收藏 {favorite_id} 不存在")))?;
        if let Some(name) = name {
            favorite.name = name;
        }
        if let Some(tags) = tags {
            favorite.tags = tags;
        }
        favorite.last_used_at = Some(self.clock.now_rfc3339());
        let updated = favorite.clone();
        self.store
            .upsert_favorite(&updated)
            .map_err(|e| AppError::storage(e.to_string()))?;
        Ok(updated)
    }

    pub fn delete_favorite(&self, favorite_id: &str) -> AppResult<()> {
        self.store
            .delete_favorite(favorite_id)
            .map_err(|e| AppError::storage(e.to_string()))?;
        Ok(())
    }

    // ---------- 输入历史 ----------

    pub fn append_history(&self, entry: &str) -> AppResult<()> {
        self.store
            .history_append(entry)
            .map_err(|e| AppError::storage(e.to_string()))
    }

    pub fn list_history(&self) -> AppResult<Vec<String>> {
        self.store
            .history_list()
            .map_err(|e| AppError::storage(e.to_string()))
    }

    /// 清空只删除 App 记录（FR-DATA-003：不删用户文件）。
    pub fn clear_history(&self) -> AppResult<()> {
        self.store
            .history_clear()
            .map_err(|e| AppError::storage(e.to_string()))
    }
}
