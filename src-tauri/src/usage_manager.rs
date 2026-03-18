use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

pub const AUTH_REQUIRED: &str = "AUTH_REQUIRED";
pub const STALE_REASON_RATE_LIMITED: &str = "rate_limited";
pub const STALE_REASON_NETWORK_ERROR: &str = "network_error";
pub const STALE_REASON_AUTH_ERROR: &str = "auth_error";

pub trait UsageData: Clone + Send + Sync + 'static {
    fn mark_stale(&self, reason: &str, retry_after: Option<String>) -> Self;
}

pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[async_trait]
pub trait UsageOps: Send + Sync + 'static {
    type Data: UsageData;
    type Credentials: Clone + Send + Sync + 'static;
    type Refresh: Send + Sync + 'static;

    fn ttl(&self) -> Duration;
    fn credentials_refresh_token<'a>(&self, creds: &'a Self::Credentials) -> &'a str;
    fn merge_refresh(
        &self,
        creds: &Self::Credentials,
        refreshed: Self::Refresh,
    ) -> Self::Credentials;

    async fn read_credentials(&self) -> Result<Self::Credentials, String>;
    async fn fetch_usage(&self, creds: &Self::Credentials) -> Result<Self::Data, String>;
    async fn refresh_credentials(&self, refresh_token: &str) -> Result<Self::Refresh, String>;
}

struct CachedData<T: Clone> {
    data: Option<T>,
    fetched_at: Option<DateTime<Utc>>,
    stale: bool,
}

impl<T: Clone> CachedData<T> {
    fn new() -> Self {
        Self {
            data: None,
            fetched_at: None,
            stale: false,
        }
    }

    fn get_if_fresh(&self, now: DateTime<Utc>, ttl: Duration) -> Option<&T> {
        if self.stale {
            return None;
        }

        if let (Some(data), Some(fetched_at)) = (&self.data, &self.fetched_at) {
            if now <= *fetched_at {
                return Some(data);
            }

            if let Ok(elapsed) = (now - *fetched_at).to_std() {
                if elapsed < ttl {
                    return Some(data);
                }
            }
        }

        None
    }
}

pub struct UsageManager<O, C = SystemClock>
where
    O: UsageOps,
    C: Clock,
{
    ops: O,
    clock: C,
    cache: RwLock<CachedData<O::Data>>,
    credentials: RwLock<Option<O::Credentials>>,
    retry_after: RwLock<Option<DateTime<Utc>>>,
    fetch_lock: Mutex<()>,
}

impl<O> UsageManager<O, SystemClock>
where
    O: UsageOps,
{
    pub fn new(ops: O) -> Self {
        Self::with_clock(ops, SystemClock)
    }
}

impl<O, C> UsageManager<O, C>
where
    O: UsageOps,
    C: Clock,
{
    pub fn with_clock(ops: O, clock: C) -> Self {
        Self {
            ops,
            clock,
            cache: RwLock::new(CachedData::new()),
            credentials: RwLock::new(None),
            retry_after: RwLock::new(None),
            fetch_lock: Mutex::new(()),
        }
    }

    pub async fn get_usage(&self, force_refresh: bool) -> Result<O::Data, String> {
        if let Some(response) = self.cooldown_response().await {
            return response;
        }

        if !force_refresh {
            if let Some(data) = self.fresh_cached_data().await {
                return Ok(data);
            }
        }

        let _fetch_guard = self.fetch_lock.lock().await;

        if let Some(response) = self.cooldown_response().await {
            return response;
        }

        if !force_refresh {
            if let Some(data) = self.fresh_cached_data().await {
                return Ok(data);
            }
        }

        let credentials = self.credentials().await?;
        self.fetch_live(credentials).await
    }

    async fn cooldown_response(&self) -> Option<Result<O::Data, String>> {
        let retry_at = self.retry_after.read().await.as_ref().cloned();
        let retry_at = retry_at?;

        if retry_at <= self.clock.now() {
            return None;
        }

        let retry_iso = retry_at.to_rfc3339();
        let cache = self.cache.read().await;
        if let Some(data) = cache.data.clone() {
            return Some(Ok(
                data.mark_stale(STALE_REASON_RATE_LIMITED, Some(retry_iso))
            ));
        }

        Some(Err(format!("RATE_LIMITED_UNTIL:{retry_iso}")))
    }

    async fn fresh_cached_data(&self) -> Option<O::Data> {
        let cache = self.cache.read().await;
        cache
            .get_if_fresh(self.clock.now(), self.ops.ttl())
            .cloned()
    }

    async fn credentials(&self) -> Result<O::Credentials, String> {
        let cached = self.credentials.read().await;
        if let Some(credentials) = cached.clone() {
            return Ok(credentials);
        }
        drop(cached);

        let fresh = self.ops.read_credentials().await?;
        *self.credentials.write().await = Some(fresh.clone());
        Ok(fresh)
    }

    async fn fetch_live(&self, credentials: O::Credentials) -> Result<O::Data, String> {
        match self.ops.fetch_usage(&credentials).await {
            Ok(data) => {
                self.store_fresh(data.clone()).await;
                Ok(data)
            }
            Err(e) if e == "UNAUTHORIZED" => self.handle_unauthorized(credentials).await,
            Err(e) if e.starts_with("RATE_LIMITED") => self.handle_rate_limited(&e).await,
            Err(e) if is_network_error(&e) => self.handle_network_error(e).await,
            Err(e) => Err(e),
        }
    }

    async fn handle_unauthorized(&self, credentials: O::Credentials) -> Result<O::Data, String> {
        match self
            .ops
            .refresh_credentials(self.ops.credentials_refresh_token(&credentials))
            .await
        {
            Ok(refreshed) => {
                let new_credentials = self.ops.merge_refresh(&credentials, refreshed);
                *self.credentials.write().await = Some(new_credentials.clone());

                match self.ops.fetch_usage(&new_credentials).await {
                    Ok(data) => {
                        self.store_fresh(data.clone()).await;
                        Ok(data)
                    }
                    Err(e) if e.starts_with("RATE_LIMITED") => self.handle_rate_limited(&e).await,
                    Err(e) if is_network_error(&e) => self.handle_network_error(e).await,
                    Err(e) if e == "UNAUTHORIZED" => self.handle_auth_failure().await,
                    Err(e) => Err(e),
                }
            }
            Err(e) if e == "UNAUTHORIZED" => self.handle_auth_failure().await,
            Err(e) if is_network_error(&e) => self.handle_network_error(e).await,
            Err(e) => Err(e),
        }
    }

    async fn handle_auth_failure(&self) -> Result<O::Data, String> {
        *self.retry_after.write().await = None;
        *self.credentials.write().await = None;
        self.return_stale(STALE_REASON_AUTH_ERROR, None, AUTH_REQUIRED.to_string())
            .await
    }

    async fn handle_rate_limited(&self, error: &str) -> Result<O::Data, String> {
        let retry_secs = parse_retry_secs(error);
        let retry_at = self.clock.now() + chrono::Duration::seconds(retry_secs as i64);
        let retry_iso = retry_at.to_rfc3339();

        *self.retry_after.write().await = Some(retry_at);

        self.return_stale(
            STALE_REASON_RATE_LIMITED,
            Some(retry_iso.clone()),
            format!("RATE_LIMITED_UNTIL:{retry_iso}"),
        )
        .await
    }

    async fn handle_network_error(&self, error: String) -> Result<O::Data, String> {
        self.return_stale(STALE_REASON_NETWORK_ERROR, None, error)
            .await
    }

    async fn return_stale(
        &self,
        reason: &str,
        retry_after: Option<String>,
        fallback_error: String,
    ) -> Result<O::Data, String> {
        let mut cache = self.cache.write().await;
        cache.stale = true;

        if let Some(data) = cache.data.clone() {
            return Ok(data.mark_stale(reason, retry_after));
        }

        Err(fallback_error)
    }

    async fn store_fresh(&self, data: O::Data) {
        *self.retry_after.write().await = None;
        *self.cache.write().await = CachedData {
            data: Some(data),
            fetched_at: Some(self.clock.now()),
            stale: false,
        };
    }
}

fn is_network_error(error: &str) -> bool {
    error.contains("Network error")
}

fn parse_retry_secs(error: &str) -> u64 {
    error
        .split(':')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60)
}

#[cfg(test)]
mod tests {
    use super::{
        Clock, UsageData, UsageManager, UsageOps, AUTH_REQUIRED, STALE_REASON_AUTH_ERROR,
        STALE_REASON_NETWORK_ERROR, STALE_REASON_RATE_LIMITED,
    };
    use async_trait::async_trait;
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::Duration;
    use tokio::sync::Notify;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct TestData {
        value: String,
        stale: bool,
        stale_reason: Option<String>,
        retry_after: Option<String>,
    }

    impl TestData {
        fn fresh(value: &str) -> Self {
            Self {
                value: value.to_string(),
                stale: false,
                stale_reason: None,
                retry_after: None,
            }
        }
    }

    impl UsageData for TestData {
        fn mark_stale(&self, reason: &str, retry_after: Option<String>) -> Self {
            let mut clone = self.clone();
            clone.stale = true;
            clone.stale_reason = Some(reason.to_string());
            clone.retry_after = retry_after;
            clone
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct TestCredentials {
        access_token: String,
        refresh_token: String,
    }

    #[derive(Clone, Debug)]
    struct TestRefresh {
        access_token: String,
        refresh_token: Option<String>,
    }

    enum FetchPlan {
        Immediate(Result<TestData, String>),
        Blocked {
            notify: Arc<Notify>,
            result: Result<TestData, String>,
        },
    }

    struct FakeOps {
        ttl: Duration,
        read_results: StdMutex<VecDeque<Result<TestCredentials, String>>>,
        fetch_results: StdMutex<VecDeque<FetchPlan>>,
        refresh_results: StdMutex<VecDeque<Result<TestRefresh, String>>>,
        read_calls: AtomicUsize,
        fetch_calls: AtomicUsize,
        refresh_calls: AtomicUsize,
        fetch_tokens: StdMutex<Vec<String>>,
    }

    impl FakeOps {
        fn new(ttl: Duration) -> Self {
            Self {
                ttl,
                read_results: StdMutex::new(VecDeque::new()),
                fetch_results: StdMutex::new(VecDeque::new()),
                refresh_results: StdMutex::new(VecDeque::new()),
                read_calls: AtomicUsize::new(0),
                fetch_calls: AtomicUsize::new(0),
                refresh_calls: AtomicUsize::new(0),
                fetch_tokens: StdMutex::new(Vec::new()),
            }
        }

        fn push_read(&self, result: Result<TestCredentials, String>) {
            self.read_results.lock().unwrap().push_back(result);
        }

        fn push_fetch(&self, result: Result<TestData, String>) {
            self.fetch_results
                .lock()
                .unwrap()
                .push_back(FetchPlan::Immediate(result));
        }

        fn push_blocked_fetch(&self, notify: Arc<Notify>, result: Result<TestData, String>) {
            self.fetch_results
                .lock()
                .unwrap()
                .push_back(FetchPlan::Blocked { notify, result });
        }

        fn push_refresh(&self, result: Result<TestRefresh, String>) {
            self.refresh_results.lock().unwrap().push_back(result);
        }
    }

    #[async_trait]
    impl UsageOps for Arc<FakeOps> {
        type Data = TestData;
        type Credentials = TestCredentials;
        type Refresh = TestRefresh;

        fn ttl(&self) -> Duration {
            self.ttl
        }

        fn credentials_refresh_token<'a>(&self, creds: &'a Self::Credentials) -> &'a str {
            &creds.refresh_token
        }

        fn merge_refresh(
            &self,
            creds: &Self::Credentials,
            refreshed: Self::Refresh,
        ) -> Self::Credentials {
            Self::Credentials {
                access_token: refreshed.access_token,
                refresh_token: refreshed
                    .refresh_token
                    .unwrap_or_else(|| creds.refresh_token.clone()),
            }
        }

        async fn read_credentials(&self) -> Result<Self::Credentials, String> {
            self.read_calls.fetch_add(1, Ordering::SeqCst);
            self.read_results
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing queued read result")
        }

        async fn fetch_usage(&self, creds: &Self::Credentials) -> Result<Self::Data, String> {
            self.fetch_calls.fetch_add(1, Ordering::SeqCst);
            self.fetch_tokens
                .lock()
                .unwrap()
                .push(creds.access_token.clone());

            let plan = self
                .fetch_results
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing queued fetch result");

            match plan {
                FetchPlan::Immediate(result) => result,
                FetchPlan::Blocked { notify, result } => {
                    notify.notified().await;
                    result
                }
            }
        }

        async fn refresh_credentials(&self, _refresh_token: &str) -> Result<Self::Refresh, String> {
            self.refresh_calls.fetch_add(1, Ordering::SeqCst);
            self.refresh_results
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing queued refresh result")
        }
    }

    #[derive(Clone)]
    struct TestClock {
        now: Arc<StdMutex<chrono::DateTime<Utc>>>,
    }

    impl TestClock {
        fn new(now: chrono::DateTime<Utc>) -> Self {
            Self {
                now: Arc::new(StdMutex::new(now)),
            }
        }

        fn advance(&self, seconds: i64) {
            let mut now = self.now.lock().unwrap();
            *now += ChronoDuration::seconds(seconds);
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> chrono::DateTime<Utc> {
            self.now.lock().unwrap().to_owned()
        }
    }

    fn test_manager(
        ttl_secs: u64,
    ) -> (
        UsageManager<Arc<FakeOps>, TestClock>,
        Arc<FakeOps>,
        TestClock,
    ) {
        let clock = TestClock::new(Utc.with_ymd_and_hms(2026, 3, 18, 12, 0, 0).unwrap());
        let ops = Arc::new(FakeOps::new(Duration::from_secs(ttl_secs)));
        let manager = UsageManager::with_clock(ops.clone(), clock.clone());
        (manager, ops, clock)
    }

    fn creds(access_token: &str, refresh_token: &str) -> TestCredentials {
        TestCredentials {
            access_token: access_token.to_string(),
            refresh_token: refresh_token.to_string(),
        }
    }

    #[tokio::test]
    async fn returns_fresh_cache_when_within_ttl_and_not_forced() {
        let (manager, ops, clock) = test_manager(300);
        ops.push_read(Ok(creds("token-a", "refresh-a")));
        ops.push_fetch(Ok(TestData::fresh("live-a")));

        let first = manager.get_usage(false).await.unwrap();
        assert_eq!(first.value, "live-a");

        clock.advance(30);

        let second = manager.get_usage(false).await.unwrap();
        assert_eq!(second.value, "live-a");
        assert_eq!(ops.read_calls.load(Ordering::SeqCst), 1);
        assert_eq!(ops.fetch_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn force_refresh_bypasses_cache() {
        let (manager, ops, _clock) = test_manager(300);
        ops.push_read(Ok(creds("token-a", "refresh-a")));
        ops.push_fetch(Ok(TestData::fresh("live-a")));
        ops.push_fetch(Ok(TestData::fresh("live-b")));

        let first = manager.get_usage(false).await.unwrap();
        let second = manager.get_usage(true).await.unwrap();

        assert_eq!(first.value, "live-a");
        assert_eq!(second.value, "live-b");
        assert_eq!(ops.fetch_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn stale_cache_is_not_reused_as_fresh_after_network_failure() {
        let (manager, ops, _clock) = test_manager(300);
        ops.push_read(Ok(creds("token-a", "refresh-a")));
        ops.push_fetch(Ok(TestData::fresh("live-a")));
        ops.push_fetch(Err("Network error fetching usage".to_string()));
        ops.push_fetch(Ok(TestData::fresh("live-b")));

        let initial = manager.get_usage(false).await.unwrap();
        assert_eq!(initial.value, "live-a");

        let stale = manager.get_usage(true).await.unwrap();
        assert_eq!(stale.value, "live-a");
        assert!(stale.stale);
        assert_eq!(
            stale.stale_reason.as_deref(),
            Some(STALE_REASON_NETWORK_ERROR)
        );

        let recovered = manager.get_usage(false).await.unwrap();
        assert_eq!(recovered.value, "live-b");
        assert!(!recovered.stale);
        assert_eq!(ops.fetch_calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retries_immediately_after_cooldown_even_when_cached_data_is_recent() {
        let (manager, ops, clock) = test_manager(300);
        ops.push_read(Ok(creds("token-a", "refresh-a")));
        ops.push_fetch(Ok(TestData::fresh("live-a")));
        ops.push_fetch(Err("RATE_LIMITED:90".to_string()));
        ops.push_fetch(Ok(TestData::fresh("live-b")));

        let initial = manager.get_usage(false).await.unwrap();
        assert_eq!(initial.value, "live-a");

        let rate_limited = manager.get_usage(true).await.unwrap();
        assert_eq!(rate_limited.value, "live-a");
        assert!(rate_limited.stale);
        assert_eq!(
            rate_limited.stale_reason.as_deref(),
            Some(STALE_REASON_RATE_LIMITED)
        );

        let during_cooldown = manager.get_usage(false).await.unwrap();
        assert_eq!(during_cooldown.value, "live-a");
        assert!(during_cooldown.retry_after.is_some());
        assert_eq!(ops.fetch_calls.load(Ordering::SeqCst), 2);

        clock.advance(91);

        let recovered = manager.get_usage(false).await.unwrap();
        assert_eq!(recovered.value, "live-b");
        assert!(!recovered.stale);
        assert_eq!(ops.fetch_calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn rate_limited_without_cache_returns_retry_until_error() {
        let (manager, ops, _clock) = test_manager(300);
        ops.push_read(Ok(creds("token-a", "refresh-a")));
        ops.push_fetch(Err("RATE_LIMITED:45".to_string()));

        let error = manager.get_usage(false).await.unwrap_err();
        assert!(error.starts_with("RATE_LIMITED_UNTIL:"));
        assert_eq!(ops.fetch_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unauthorized_fetch_refreshes_and_retries_once() {
        let (manager, ops, _clock) = test_manager(300);
        ops.push_read(Ok(creds("expired-token", "refresh-a")));
        ops.push_fetch(Err("UNAUTHORIZED".to_string()));
        ops.push_refresh(Ok(TestRefresh {
            access_token: "fresh-token".to_string(),
            refresh_token: Some("refresh-b".to_string()),
        }));
        ops.push_fetch(Ok(TestData::fresh("live-b")));

        let data = manager.get_usage(false).await.unwrap();
        assert_eq!(data.value, "live-b");
        assert_eq!(ops.refresh_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            ops.fetch_tokens.lock().unwrap().clone(),
            vec!["expired-token".to_string(), "fresh-token".to_string()]
        );
    }

    #[tokio::test]
    async fn auth_failure_clears_cached_credentials_and_surfaces_auth_required() {
        let (manager, ops, _clock) = test_manager(300);
        ops.push_read(Ok(creds("expired-token", "refresh-a")));
        ops.push_fetch(Err("UNAUTHORIZED".to_string()));
        ops.push_refresh(Err("UNAUTHORIZED".to_string()));
        ops.push_read(Ok(creds("new-token", "refresh-b")));
        ops.push_fetch(Ok(TestData::fresh("live-c")));

        let error = manager.get_usage(false).await.unwrap_err();
        assert_eq!(error, AUTH_REQUIRED);

        let recovered = manager.get_usage(false).await.unwrap();
        assert_eq!(recovered.value, "live-c");
        assert_eq!(ops.read_calls.load(Ordering::SeqCst), 2);
        assert_eq!(ops.refresh_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_inflight_fetch() {
        let (manager, ops, _clock) = test_manager(300);
        let manager = Arc::new(manager);
        let notify = Arc::new(Notify::new());

        ops.push_read(Ok(creds("token-a", "refresh-a")));
        ops.push_blocked_fetch(notify.clone(), Ok(TestData::fresh("live-a")));

        let first_manager = manager.clone();
        let second_manager = manager.clone();
        let first = tokio::spawn(async move { first_manager.get_usage(false).await });
        let second = tokio::spawn(async move { second_manager.get_usage(false).await });

        tokio::task::yield_now().await;
        assert_eq!(ops.fetch_calls.load(Ordering::SeqCst), 1);

        notify.notify_waiters();

        let first = first.await.unwrap().unwrap();
        let second = second.await.unwrap().unwrap();

        assert_eq!(first.value, "live-a");
        assert_eq!(second.value, "live-a");
        assert_eq!(ops.fetch_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn stale_auth_response_is_returned_when_cache_exists() {
        let (manager, ops, _clock) = test_manager(300);
        ops.push_read(Ok(creds("token-a", "refresh-a")));
        ops.push_fetch(Ok(TestData::fresh("live-a")));
        ops.push_fetch(Err("UNAUTHORIZED".to_string()));
        ops.push_refresh(Err("UNAUTHORIZED".to_string()));

        let initial = manager.get_usage(false).await.unwrap();
        assert_eq!(initial.value, "live-a");

        let stale = manager.get_usage(true).await.unwrap();
        assert_eq!(stale.value, "live-a");
        assert!(stale.stale);
        assert_eq!(stale.stale_reason.as_deref(), Some(STALE_REASON_AUTH_ERROR));
    }
}
