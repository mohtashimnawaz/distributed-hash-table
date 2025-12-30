use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Default, Clone)]
pub struct Store {
    inner: Arc<RwLock<HashMap<Vec<u8>, Vec<u8>>>>,
}

impl Store {
    pub fn new() -> Self {
        Self { inner: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub async fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        let mut w = self.inner.write().await;
        w.insert(key, value);
    }

    pub async fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let r = self.inner.read().await;
        r.get(key).cloned()
    }
}
