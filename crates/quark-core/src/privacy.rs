use std::collections::HashMap;
use std::future::Future;

use quark_protocol::SecretClient;
use tokio_util::sync::CancellationToken;

use crate::{file_view, AppCore, CoreError, ListResult, Result};

#[derive(Default)]
pub(super) struct PrivacySessions {
    accounts: HashMap<String, PrivacyState>,
}

#[derive(Default)]
struct PrivacyState {
    epoch: u64,
    canceled: CancellationToken,
    session: Option<SecretClient>,
    logged_out: bool,
}

pub(super) struct PrivacyEpoch {
    pub(super) value: u64,
    pub(super) canceled: CancellationToken,
}

impl PrivacyState {
    fn revoke(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.canceled.cancel();
        self.canceled = CancellationToken::new();
        if let Some(session) = self.session.take() {
            session.lock();
        }
    }

    fn snapshot(&self) -> PrivacyEpoch {
        PrivacyEpoch {
            value: self.epoch,
            canceled: self.canceled.clone(),
        }
    }
}

impl Drop for PrivacyState {
    fn drop(&mut self) {
        self.canceled.cancel();
        if let Some(session) = &self.session {
            session.lock();
        }
    }
}

impl PrivacySessions {
    fn begin(&mut self, account_id: &str, unlock: bool) -> Result<PrivacyEpoch> {
        let state = self.accounts.entry(account_id.to_string()).or_default();
        if state.logged_out {
            return Err(locked());
        }
        if unlock {
            state.revoke();
        }
        Ok(state.snapshot())
    }

    pub(super) fn validate(&self, account_id: &str, epoch: &PrivacyEpoch) -> Result<()> {
        match self.accounts.get(account_id) {
            Some(state)
                if !state.logged_out
                    && state.epoch == epoch.value
                    && !epoch.canceled.is_cancelled() =>
            {
                Ok(())
            }
            _ => Err(locked()),
        }
    }

    pub(super) fn unlocked(&self, account_id: &str) -> Result<(PrivacyEpoch, SecretClient)> {
        let state = self.accounts.get(account_id).ok_or_else(locked)?;
        if state.logged_out {
            return Err(locked());
        }
        let session = state.session.clone().ok_or_else(locked)?;
        Ok((state.snapshot(), session))
    }

    fn install(
        &mut self,
        account_id: &str,
        epoch: &PrivacyEpoch,
        session: SecretClient,
    ) -> Result<()> {
        if let Err(error) = self.validate(account_id, epoch) {
            session.lock();
            return Err(error);
        }
        self.accounts.get_mut(account_id).unwrap().session = Some(session);
        Ok(())
    }

    fn lock(&mut self, account_id: &str) {
        self.accounts
            .entry(account_id.to_string())
            .or_default()
            .revoke();
    }

    pub(super) fn logout(&mut self, account_id: &str) {
        let state = self.accounts.entry(account_id.to_string()).or_default();
        state.revoke();
        state.logged_out = true;
    }

    pub(super) fn login(&mut self, account_id: &str) {
        let state = self.accounts.entry(account_id.to_string()).or_default();
        state.revoke();
        state.logged_out = false;
    }
}

impl AppCore {
    pub async fn privacy_registered(&self, account_id: &str) -> Result<bool> {
        let epoch = self
            .inner
            .privacy
            .lock()
            .expect("privacy mutex poisoned")
            .begin(account_id, false)?;
        let response = until_locked(&epoch, async {
            let client = self
                .client_for(account_id)
                .await
                .map_err(|_| failed("无法检查隐私空间状态，请稍后重试"))?;
            client
                .secret_is_registered()
                .await
                .map_err(|_| failed("无法检查隐私空间状态，请稍后重试"))
        })
        .await;
        self.inner
            .privacy
            .lock()
            .expect("privacy mutex poisoned")
            .validate(account_id, &epoch)?;
        response
    }

    pub async fn privacy_unlock(&self, account_id: &str, password: &str) -> Result<()> {
        self.cancel_privacy_transfers(account_id);
        let epoch = self
            .inner
            .privacy
            .lock()
            .expect("privacy mutex poisoned")
            .begin(account_id, true)?;
        let response = until_locked(&epoch, async {
            let client = self
                .client_for(account_id)
                .await
                .map_err(|_| failed("隐私空间解锁失败，请检查密码后重试"))?;
            client
                .secret_unlock(password)
                .await
                .map_err(|_| failed("隐私空间解锁失败，请检查密码后重试"))
        })
        .await;
        let mut sessions = self.inner.privacy.lock().expect("privacy mutex poisoned");
        match response {
            Ok(session) => sessions.install(account_id, &epoch, session),
            Err(error) => {
                sessions.validate(account_id, &epoch)?;
                Err(error)
            }
        }
    }

    pub async fn privacy_list_dir(
        &self,
        account_id: &str,
        parent_fid: &str,
        page: u32,
        size: u32,
    ) -> Result<ListResult> {
        let (epoch, session) = self
            .inner
            .privacy
            .lock()
            .expect("privacy mutex poisoned")
            .unlocked(account_id)?;
        let response = until_locked(&epoch, async {
            session
                .list_dir(
                    parent_fid,
                    page.max(1),
                    size.clamp(1, 200),
                    quark_protocol::DEFAULT_SORT,
                )
                .await
                .map_err(|_| failed("隐私空间读取失败，请重新解锁后重试"))
        })
        .await;
        let sessions = self.inner.privacy.lock().expect("privacy mutex poisoned");
        sessions.validate(account_id, &epoch)?;
        drop(sessions);
        match response {
            Ok(result) => Ok(ListResult {
                entries: result.entries.iter().map(file_view).collect(),
                total: result.total,
                page: result.page,
                size: result.size,
            }),
            Err(error) => {
                self.cancel_privacy_transfers(account_id);
                self.inner
                    .privacy
                    .lock()
                    .expect("privacy mutex poisoned")
                    .lock(account_id);
                Err(error)
            }
        }
    }

    pub async fn privacy_move_files(
        &self,
        account_id: &str,
        fids: &[String],
        parent_fid: &str,
    ) -> Result<()> {
        if fids.is_empty() {
            return Err(CoreError::InvalidInput("没有选择文件".to_string()));
        }
        let (epoch, session) = self.privacy_unlocked(account_id).await?;
        let response = until_locked(&epoch, async {
            session
                .move_files(fids, parent_fid)
                .await
                .map_err(|_| failed("无法移动隐私文件，请重新解锁后重试"))
        })
        .await;
        if self.validate_privacy_epoch(account_id, &epoch).is_err() {
            return Err(locked());
        }
        if response.is_err() {
            self.cancel_privacy_transfers(account_id);
            self.inner
                .privacy
                .lock()
                .expect("privacy mutex poisoned")
                .lock(account_id);
        }
        response
    }

    pub async fn privacy_lock(&self, account_id: &str) -> Result<()> {
        self.cancel_privacy_transfers(account_id);
        self.inner
            .privacy
            .lock()
            .expect("privacy mutex poisoned")
            .lock(account_id);
        Ok(())
    }

    pub(crate) async fn privacy_unlocked(
        &self,
        account_id: &str,
    ) -> Result<(PrivacyEpoch, SecretClient)> {
        self.inner
            .privacy
            .lock()
            .expect("privacy mutex poisoned")
            .unlocked(account_id)
    }

    pub(crate) fn validate_privacy_epoch(
        &self,
        account_id: &str,
        epoch: &PrivacyEpoch,
    ) -> Result<()> {
        self.inner
            .privacy
            .lock()
            .expect("privacy mutex poisoned")
            .validate(account_id, epoch)
    }
}

pub(crate) async fn until_locked<T>(
    epoch: &PrivacyEpoch,
    request: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::select! {
        biased;
        _ = epoch.canceled.cancelled() => Err(locked()),
        response = request => response,
    }
}

fn failed(message: &str) -> CoreError {
    CoreError::Other(message.to_string())
}

fn locked() -> CoreError {
    failed("隐私空间已锁定，请重新解锁")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn lock_invalidates_pending_unlock_without_installed_session() {
        let mut sessions = PrivacySessions::default();
        let pending = sessions.begin("account", true).unwrap();
        sessions.lock("account");
        assert!(pending.canceled.is_cancelled());
        assert!(sessions.validate("account", &pending).is_err());
        assert!(sessions.unlocked("account").is_err());
    }

    #[test]
    fn overlapping_unlocks_only_allow_the_newest_epoch() {
        let mut sessions = PrivacySessions::default();
        let older = sessions.begin("account", true).unwrap();
        let newer = sessions.begin("account", true).unwrap();
        assert!(sessions.validate("account", &older).is_err());
        assert!(sessions.validate("account", &newer).is_ok());
    }

    #[test]
    fn lock_and_logout_are_isolated_per_account() {
        let mut sessions = PrivacySessions::default();
        let first = sessions.begin("first", true).unwrap();
        let second = sessions.begin("second", true).unwrap();
        sessions.lock("missing");
        sessions.logout("first");
        assert!(sessions.validate("first", &first).is_err());
        assert!(sessions.validate("second", &second).is_ok());
        assert!(!second.canceled.is_cancelled());
    }

    #[test]
    fn logout_blocks_new_operations_until_login_without_reviving_old_requests() {
        let mut sessions = PrivacySessions::default();
        let older = sessions.begin("account", true).unwrap();
        sessions.logout("account");
        sessions.lock("account");
        assert!(sessions.begin("account", true).is_err());
        assert!(sessions.begin("account", false).is_err());
        sessions.login("account");
        let newer = sessions.begin("account", true).unwrap();
        assert!(sessions.validate("account", &older).is_err());
        assert!(sessions.validate("account", &newer).is_ok());
    }

    #[tokio::test]
    async fn locking_cancels_pending_request_without_waiting_for_a_response() {
        let sessions = Arc::new(Mutex::new(PrivacySessions::default()));
        let epoch = sessions.lock().unwrap().begin("account", true).unwrap();
        let (started, waiting) = tokio::sync::oneshot::channel();
        let request = tokio::spawn(async move {
            until_locked(&epoch, async {
                started.send(()).unwrap();
                std::future::pending::<Result<()>>().await
            })
            .await
        });
        waiting.await.unwrap();
        sessions.lock().unwrap().lock("account");
        let error = tokio::time::timeout(Duration::from_secs(2), request)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.to_string(), "隐私空间已锁定，请重新解锁");
    }

    #[tokio::test]
    async fn cancellation_takes_priority_over_an_already_ready_response() {
        let mut sessions = PrivacySessions::default();
        let epoch = sessions.begin("account", true).unwrap();
        sessions.lock("account");
        assert!(until_locked(&epoch, async { Ok("private response") })
            .await
            .is_err());
    }
}
