use crate::database_client::DatabaseClientTrait;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ClientHolder {
    client: Arc<RwLock<Option<Arc<dyn DatabaseClientTrait>>>>,
}

impl ClientHolder {
    pub fn new(client: Option<Arc<dyn DatabaseClientTrait>>) -> Self {
        Self {
            client: Arc::new(RwLock::new(client)),
        }
    }
    pub async fn get(&self) -> Option<Arc<dyn DatabaseClientTrait>> {
        self.client.read().await.clone()
    }

    pub async fn set(&self, client: Arc<dyn DatabaseClientTrait>) {
        *self.client.write().await = Some(client);
    }
}

impl Clone for ClientHolder {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database_client::MockDatabaseClientTrait;
    use ntest::timeout;

    #[tokio::test]
    #[timeout(100)]
    async fn test_holder_get_set_clone() {
        let holder = ClientHolder::new(None);
        assert!(holder.get().await.is_none());

        let mock_client = MockDatabaseClientTrait::new();
        holder.set(Arc::new(mock_client)).await;

        assert!(holder.get().await.is_some());

        let holder_clone = holder.clone();
        assert!(holder_clone.get().await.is_some());
    }
}
