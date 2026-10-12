//! ProviderService（M3.2；FR-AI-002/003、architecture.md §9.1）：用户配置的
//! OpenAI 兼容端点。记录只保存配置形态（显示名、端点、模型、默认生成/摘要模型、
//! 超时）；API 密钥只写入系统凭据服务（键 `provider.<id>`），快照永不回传密钥。

use crate::dto::{AppError, AppEvent, AppResult};
use crate::ports::{Clock, CredentialPort, EventSink, ProviderStore};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use ts_rs::TS;

/// 端点记录（持久化形态；不含密钥）。协议首版只有 openai-compatible。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ProviderRecord {
    pub id: String,
    pub display_name: String,
    pub base_url: String,
    pub models: Vec<String>,
    pub default_generation_model: Option<String>,
    pub summary_model: Option<String>,
    pub timeout_ms: u64,
}

/// 保存请求（写路径）：api_key 为 None 表示保持现状；Some("") 表示清除密钥。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ProviderSaveRequest {
    pub id: String,
    pub display_name: String,
    pub base_url: String,
    #[serde(default)]
    pub models: Vec<String>,
    pub default_generation_model: Option<String>,
    pub summary_model: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub api_key: Option<String>,
}

fn default_timeout() -> u64 {
    30_000
}

/// 列表条目（读路径）：记录 + 密钥是否已配置（掩码语义，不回传密钥）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "packages/contracts/src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub record: ProviderRecord,
    pub credential_configured: bool,
}

pub struct ProviderService {
    store: Arc<dyn ProviderStore>,
    credentials: Arc<dyn CredentialPort>,
    clock: Arc<dyn Clock>,
    events: Arc<dyn EventSink>,
    operation: Mutex<()>,
}

impl ProviderService {
    pub fn new(
        store: Arc<dyn ProviderStore>,
        credentials: Arc<dyn CredentialPort>,
        clock: Arc<dyn Clock>,
        events: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            store,
            credentials,
            clock,
            events,
            operation: Mutex::new(()),
        }
    }

    fn credential_key(provider_id: &str) -> String {
        format!("provider.{provider_id}")
    }

    fn clear_credential(&self, provider_id: &str) -> AppResult<()> {
        match self.credentials.delete(&Self::credential_key(provider_id)) {
            Ok(()) | Err(crate::ports::CredentialError::NotFound) => Ok(()),
            Err(error) => Err(AppError::storage(format!("密钥清除失败：{error}"))),
        }
    }

    /// 校验端点形态：HTTPS 或回环地址（本地服务），与工具下载同一规则。
    pub fn validate_base_url(base_url: &str) -> Result<(), String> {
        let parsed = url::Url::parse(base_url).map_err(|_| "端点地址格式无效".to_owned())?;
        if parsed.host().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err("端点地址不能包含账号、密码、查询参数或片段；密钥请填写在密钥栏".into());
        }
        let loopback = match parsed.host() {
            Some(url::Host::Domain("localhost")) => true,
            Some(url::Host::Ipv4(address)) => address.is_loopback(),
            Some(url::Host::Ipv6(address)) => address.is_loopback(),
            _ => false,
        };
        match parsed.scheme() {
            "https" => Ok(()),
            "http" if loopback => Ok(()),
            _ => Err("端点地址必须是 HTTPS（本地回环地址除外）".into()),
        }
    }

    /// 保存：记录入库 + 密钥写入凭据服务（None 保持现状，空串清除）。
    pub fn save(&self, mut request: ProviderSaveRequest) -> AppResult<ProviderView> {
        let _guard = self
            .operation
            .lock()
            .map_err(|_| AppError::storage("连接配置锁不可用"))?;
        if request.id.trim().is_empty() || request.id.contains('/') {
            return Err(AppError::validation(vec![
                fleqi_domain::settings::FieldError {
                    field: "id".into(),
                    code: "invalid".into(),
                    message: "端点 ID 不能为空且不能包含路径分隔符".into(),
                },
            ]));
        }
        if request.display_name.trim().is_empty() {
            return Err(AppError::validation(vec![
                fleqi_domain::settings::FieldError {
                    field: "displayName".into(),
                    code: "required".into(),
                    message: "显示名称不能为空".into(),
                },
            ]));
        }
        Self::validate_base_url(&request.base_url).map_err(|message| {
            AppError::validation(vec![fleqi_domain::settings::FieldError {
                field: "baseUrl".into(),
                code: "invalid".into(),
                message,
            }])
        })?;
        if request.timeout_ms < 1_000 || request.timeout_ms > 300_000 {
            return Err(AppError::validation(vec![
                fleqi_domain::settings::FieldError {
                    field: "timeoutMs".into(),
                    code: "range".into(),
                    message: "超时必须在 1000–300000ms 之间".into(),
                },
            ]));
        }
        request.models = request
            .models
            .iter()
            .map(|model| model.trim().to_owned())
            .collect();
        if request.models.iter().any(|model| model.is_empty()) {
            return Err(AppError::validation(vec![
                fleqi_domain::settings::FieldError {
                    field: "models".into(),
                    code: "required".into(),
                    message: "模型 ID 不能为空".into(),
                },
            ]));
        }
        let mut seen = std::collections::HashSet::new();
        request.models.retain(|model| seen.insert(model.clone()));
        for (field, selected) in [
            ("defaultGenerationModel", &request.default_generation_model),
            ("summaryModel", &request.summary_model),
        ] {
            if let Some(model) = selected
                && !request.models.contains(model)
            {
                return Err(AppError::validation(vec![
                    fleqi_domain::settings::FieldError {
                        field: field.into(),
                        code: "invalid".into(),
                        message: "所选模型必须在此连接的模型列表中".into(),
                    },
                ]));
            }
        }
        if request.api_key.is_none() {
            let previous = self
                .store
                .load_all()
                .map_err(|e| AppError::storage(e.to_string()))?
                .into_iter()
                .find(|record| record.id == request.id);
            if let Some(previous) = previous
                && previous.base_url.trim_end_matches('/') != request.base_url.trim_end_matches('/')
                && self
                    .credentials
                    .exists(&Self::credential_key(&request.id))
                    .map_err(|e| AppError::storage(e.to_string()))?
            {
                return Err(AppError::unavailable(
                    "地址已修改，请重新输入密钥或明确清除已有密钥后保存",
                ));
            }
        }
        let key = Self::credential_key(&request.id);
        let previous_secret = if request.api_key.is_some() {
            match self.credentials.load(&key) {
                Ok(bytes) => Some(bytes),
                Err(crate::ports::CredentialError::NotFound) => None,
                Err(error) => return Err(AppError::storage(format!("读取凭据失败：{error}"))),
            }
        } else {
            None
        };
        match request.api_key.as_deref() {
            None => {}
            Some("") => {
                self.clear_credential(&request.id)?;
            }
            Some(secret) => {
                let outcome = if previous_secret.is_some() {
                    self.credentials.replace(&key, secret.as_bytes())
                } else {
                    self.credentials.store(&key, secret.as_bytes())
                };
                outcome.map_err(|e| AppError::storage(format!("密钥保存失败：{e}")))?;
            }
        }
        let record = ProviderRecord {
            id: request.id,
            display_name: request.display_name,
            base_url: request.base_url,
            models: request.models,
            default_generation_model: request.default_generation_model,
            summary_model: request.summary_model,
            timeout_ms: request.timeout_ms,
        };
        if let Err(error) = self.store.upsert(&record) {
            // 配置与系统凭据无法共用事务；入库失败时恢复此前凭据。
            if request.api_key.is_some() {
                let restore = match previous_secret {
                    Some(bytes) => self
                        .credentials
                        .replace(&key, &bytes)
                        .map_err(|error| AppError::storage(error.to_string())),
                    None => self.clear_credential(&record.id),
                };
                if let Err(restore_error) = restore {
                    return Err(AppError::storage(format!(
                        "配置保存失败：{error}；凭据恢复失败：{}",
                        restore_error.message
                    )));
                }
            }
            return Err(AppError::storage(error.to_string()));
        }
        let view = self.view_of(&record)?;
        self.events.emit(AppEvent::ProvidersChanged);
        Ok(view)
    }

    /// 只向原地址复用凭据；修改地址后必须显式提供新密钥，避免将已有密钥发往另一服务。
    pub fn probe_key(
        &self,
        provider_id: Option<&str>,
        base_url: &str,
        api_key: Option<String>,
    ) -> AppResult<Option<String>> {
        let _guard = self
            .operation
            .lock()
            .map_err(|_| AppError::storage("连接配置锁不可用"))?;
        if let Some(secret) = api_key {
            return Ok((!secret.is_empty()).then_some(secret));
        }
        let Some(id) = provider_id else {
            return Ok(None);
        };
        let record = self
            .store
            .load_all()
            .map_err(|e| AppError::storage(e.to_string()))?
            .into_iter()
            .find(|record| record.id == id)
            .ok_or_else(|| AppError::unavailable("此连接已删除，请重新打开配置"))?;
        let key = Self::credential_key(id);
        if !self
            .credentials
            .exists(&key)
            .map_err(|e| AppError::storage(e.to_string()))?
        {
            return Ok(None);
        }
        if record.base_url.trim_end_matches('/') != base_url.trim_end_matches('/') {
            return Err(AppError::unavailable(
                "地址已修改，请重新输入密钥后检查或保存",
            ));
        }
        let bytes = self
            .credentials
            .load(&key)
            .map_err(|e| AppError::storage(format!("读取凭据失败：{e}")))?;
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| AppError::storage("凭据编码无效"))
    }

    /// 列表：记录 + 密钥配置状态（掩码）。
    pub fn list(&self) -> AppResult<Vec<ProviderView>> {
        let _guard = self
            .operation
            .lock()
            .map_err(|_| AppError::storage("连接配置锁不可用"))?;
        let records = self
            .store
            .load_all()
            .map_err(|e| AppError::storage(e.to_string()))?;
        records.iter().map(|record| self.view_of(record)).collect()
    }

    /// 删除：记录与密钥一并清理。
    pub fn delete(&self, provider_id: &str) -> AppResult<()> {
        let _guard = self
            .operation
            .lock()
            .map_err(|_| AppError::storage("连接配置锁不可用"))?;
        let key = Self::credential_key(provider_id);
        let previous_secret = match self.credentials.load(&key) {
            Ok(bytes) => Some(bytes),
            Err(crate::ports::CredentialError::NotFound) => None,
            Err(error) => return Err(AppError::storage(error.to_string())),
        };
        self.clear_credential(provider_id)?;
        if let Err(error) = self.store.delete(provider_id) {
            if let Some(bytes) = previous_secret {
                self.credentials.replace(&key, &bytes).map_err(|restore| {
                    AppError::storage(format!("删除失败：{error}；凭据恢复失败：{restore}"))
                })?;
            }
            return Err(AppError::storage(error.to_string()));
        }
        self.events.emit(AppEvent::ProvidersChanged);
        Ok(())
    }

    fn view_of(&self, record: &ProviderRecord) -> AppResult<ProviderView> {
        let configured = self
            .credentials
            .exists(&Self::credential_key(&record.id))
            .map_err(|error| AppError::storage(error.to_string()))?;
        Ok(ProviderView {
            record: record.clone(),
            credential_configured: configured,
        })
    }

    /// 规划用：取默认端点（优先有默认生成模型的记录）与其密钥。
    pub fn default_runtime(&self) -> AppResult<(ProviderRecord, Option<String>)> {
        self.runtime_for_selection(None)
    }

    /// 限定端点的模型选择使用 JSON [providerId, model]；旧裸模型名仅在唯一匹配时接受。
    pub fn runtime_for_selection(
        &self,
        selection: Option<&str>,
    ) -> AppResult<(ProviderRecord, Option<String>)> {
        let _guard = self
            .operation
            .lock()
            .map_err(|_| AppError::storage("连接配置锁不可用"))?;
        let records = self
            .store
            .load_all()
            .map_err(|e| AppError::storage(e.to_string()))?;
        if records.is_empty() {
            return Err(AppError::unavailable(
                "尚未配置模型端点：请先在设置的模型页添加端点",
            ));
        }
        let qualified =
            selection.and_then(|value| serde_json::from_str::<(String, String)>(value).ok());
        let record = if let Some((provider, model)) = &qualified {
            records
                .iter()
                .find(|record| &record.id == provider && record.models.contains(model))
                .ok_or_else(|| AppError::unavailable("选定的模型或端点已不可用，请重新选择"))?
        } else if let Some(model) = selection.filter(|model| !model.trim().is_empty()) {
            let mut matches = records
                .iter()
                .filter(|record| record.models.iter().any(|candidate| candidate == model));
            let selected = matches
                .next()
                .ok_or_else(|| AppError::unavailable("选定模型不在任何端点中，请重新选择"))?;
            if matches.next().is_some() {
                return Err(AppError::unavailable(
                    "模型名对应多个端点，请在设置中明确选择端点",
                ));
            }
            selected
        } else {
            records
                .iter()
                .find(|record| record.default_generation_model.is_some())
                .or_else(|| records.first())
                .ok_or_else(|| {
                    AppError::unavailable(
                        "尚未配置模型端点：请先在设置的模型页添加 OpenAI 兼容端点并选择默认模型",
                    )
                })?
        };
        let secret = self
            .credentials
            .load(&Self::credential_key(&record.id))
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        let mut record = record.clone();
        if let Some((_, model)) = qualified {
            record.default_generation_model = Some(model);
        } else if let Some(model) = selection {
            record.default_generation_model = Some(model.to_owned());
        }
        Ok((record, secret))
    }

    #[allow(dead_code)]
    fn now(&self) -> String {
        self.clock.now_rfc3339()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{CredentialError, StorageError};
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct FakeStore(Mutex<Vec<ProviderRecord>>);
    impl ProviderStore for FakeStore {
        fn upsert(&self, provider: &ProviderRecord) -> Result<(), StorageError> {
            let mut providers = self.0.lock().unwrap();
            providers.retain(|existing| existing.id != provider.id);
            providers.push(provider.clone());
            Ok(())
        }
        fn load_all(&self) -> Result<Vec<ProviderRecord>, StorageError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn delete(&self, provider_id: &str) -> Result<(), StorageError> {
            self.0.lock().unwrap().retain(|p| p.id != provider_id);
            Ok(())
        }
    }

    struct FakeCredentials(Mutex<HashMap<String, Vec<u8>>>);
    impl CredentialPort for FakeCredentials {
        fn namespace(&self) -> &str {
            "test"
        }
        fn store(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError> {
            let mut map = self.0.lock().unwrap();
            if map.contains_key(key) {
                return Err(CredentialError::Failed("已存在".into()));
            }
            map.insert(key.into(), secret.to_vec());
            Ok(())
        }
        fn replace(&self, key: &str, secret: &[u8]) -> Result<(), CredentialError> {
            self.0.lock().unwrap().insert(key.into(), secret.to_vec());
            Ok(())
        }
        fn exists(&self, key: &str) -> Result<bool, CredentialError> {
            Ok(self.0.lock().unwrap().contains_key(key))
        }
        fn delete(&self, key: &str) -> Result<(), CredentialError> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
        fn load(&self, key: &str) -> Result<Vec<u8>, CredentialError> {
            self.0
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or(CredentialError::NotFound)
        }
    }

    struct FixedClock;
    impl Clock for FixedClock {
        fn now_rfc3339(&self) -> String {
            "2026-09-18T00:00:00Z".into()
        }
    }

    struct NoEvents;
    impl EventSink for NoEvents {
        fn emit(&self, _event: AppEvent) {}
    }

    fn service() -> ProviderService {
        ProviderService::new(
            Arc::new(FakeStore(Mutex::new(Vec::new()))),
            Arc::new(FakeCredentials(Mutex::new(HashMap::new()))),
            Arc::new(FixedClock),
            Arc::new(NoEvents),
        )
    }

    fn request(id: &str) -> ProviderSaveRequest {
        ProviderSaveRequest {
            id: id.into(),
            display_name: "本地端点".into(),
            base_url: "https://example.com/v1".into(),
            models: vec!["m-mini".into()],
            default_generation_model: Some("m-mini".into()),
            summary_model: None,
            timeout_ms: 30_000,
            api_key: None,
        }
    }

    #[test]
    fn save_stores_secret_separately_and_view_never_returns_it() {
        let provider = service();
        let mut req = request("p1");
        req.api_key = Some("secret-value".into());
        let view = provider.save(req).unwrap();
        assert!(view.credential_configured);
        // 视图与记录都不含密钥字段。
        let serialized = serde_json::to_string(&view).unwrap();
        assert!(!serialized.contains("secret-value"));
        let runtime = provider.default_runtime().unwrap();
        assert_eq!(runtime.0.id, "p1");
        assert_eq!(runtime.1.as_deref(), Some("secret-value"));
    }

    #[test]
    fn plain_http_rejected_except_loopback_and_delete_clears_secret() {
        let provider = service();
        let mut bad = request("p2");
        bad.base_url = "http://example.com/v1".into();
        assert!(provider.save(bad).is_err());
        let mut local = request("p2");
        local.base_url = "http://127.0.0.1:8080/v1".into();
        assert!(provider.save(local).is_ok());

        let mut keyed = request("p3");
        keyed.api_key = Some("k".into());
        provider.save(keyed).unwrap();
        provider.delete("p3").unwrap();
        assert!(provider.default_runtime().is_ok(), "p2 仍在");
        let view = provider.list().unwrap();
        assert!(view.iter().all(|entry| entry.record.id != "p3"));
        assert!(!provider.default_runtime().unwrap().1.is_some());
    }

    #[test]
    fn endpoint_validation_uses_the_actual_host_and_supports_ipv6_loopback() {
        for url in [
            "http://localhost:123@remote.example/v1",
            "http://127.0.0.1.example/v1",
            "https://user:example@example.com/v1",
            "https://example.com/v1?key=example",
            "https:///",
        ] {
            assert!(ProviderService::validate_base_url(url).is_err(), "{url}");
        }
        for url in [
            "http://[::1]:11434/v1",
            "http://127.0.0.1:1234/v1",
            "https://api.openai.com/v1",
        ] {
            assert!(ProviderService::validate_base_url(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn probe_reuses_saved_key_only_at_its_saved_endpoint() {
        let service = service();
        let mut req = request("saved");
        req.api_key = Some("fixture-secret".into());
        service.save(req).unwrap();
        assert_eq!(
            service
                .probe_key(Some("saved"), "https://example.com/v1/", None)
                .unwrap()
                .as_deref(),
            Some("fixture-secret")
        );
        assert!(
            service
                .probe_key(Some("saved"), "https://other.example/v1", None)
                .is_err()
        );
        assert_eq!(
            service
                .probe_key(
                    Some("saved"),
                    "https://other.example/v1",
                    Some("replacement".into())
                )
                .unwrap()
                .as_deref(),
            Some("replacement")
        );
        assert!(
            service
                .probe_key(
                    Some("saved"),
                    "https://other.example/v1",
                    Some(String::new())
                )
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .probe_key(Some("deleted"), "https://example.com/v1", None)
                .is_err()
        );
    }

    #[test]
    fn invalid_model_selection_cannot_mutate_credentials_and_models_are_deduplicated() {
        let service = service();
        let mut req = request("saved");
        req.api_key = Some("fixture-secret".into());
        service.save(req.clone()).unwrap();
        req.default_generation_model = Some("missing".into());
        req.api_key = Some(String::new());
        assert!(service.save(req).is_err());
        assert!(service.list().unwrap()[0].credential_configured);
        let mut req = request("saved");
        req.models = vec![" m-mini ".into(), "m-mini".into()];
        assert_eq!(service.save(req).unwrap().record.models, vec!["m-mini"]);
        let mut req = request("saved");
        req.summary_model = Some("missing".into());
        assert!(service.save(req).is_err());
    }

    #[test]
    fn changing_saved_address_requires_explicit_credential_choice() {
        let service = service();
        let mut req = request("saved");
        req.api_key = Some("fixture-secret".into());
        service.save(req).unwrap();
        let mut changed = request("saved");
        changed.base_url = "https://other.example/v1".into();
        assert!(service.save(changed.clone()).is_err());
        assert_eq!(
            service.list().unwrap()[0].record.base_url,
            "https://example.com/v1"
        );
        changed.api_key = Some(String::new());
        assert!(!service.save(changed).unwrap().credential_configured);
    }

    #[test]
    fn failed_configuration_write_restores_credentials() {
        struct ReadOnlyStore(ProviderRecord);
        impl ProviderStore for ReadOnlyStore {
            fn load_all(&self) -> Result<Vec<ProviderRecord>, StorageError> {
                Ok(vec![self.0.clone()])
            }
            fn upsert(&self, _: &ProviderRecord) -> Result<(), StorageError> {
                Err(StorageError::Unavailable("只读".into()))
            }
            fn delete(&self, _: &str) -> Result<(), StorageError> {
                Err(StorageError::Unavailable("只读".into()))
            }
        }
        let view = service().save(request("saved")).unwrap();
        let credentials = Arc::new(FakeCredentials(Mutex::new(HashMap::from([(
            "provider.saved".into(),
            b"original".to_vec(),
        )]))));
        let service = ProviderService::new(
            Arc::new(ReadOnlyStore(view.record)),
            credentials.clone(),
            Arc::new(FixedClock),
            Arc::new(NoEvents),
        );
        for key in ["replacement", ""] {
            let mut req = request("saved");
            req.api_key = Some(key.into());
            assert!(service.save(req).is_err());
            assert_eq!(credentials.load("provider.saved").unwrap(), b"original");
        }
        assert!(service.delete("saved").is_err());
        assert_eq!(credentials.load("provider.saved").unwrap(), b"original");
        let mut req = request("new");
        req.api_key = Some("new-secret".into());
        assert!(service.save(req).is_err());
        assert!(!credentials.exists("provider.new").unwrap());
    }
}
