#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KnowledgeQueryCacheKey(String);

impl KnowledgeQueryCacheKey {
    pub(crate) fn new(
        workspace: uuid::Uuid,
        principal: uuid::Uuid,
        generation: i64,
        model: &tect_domain::KnowledgeEmbeddingModelIdentity,
        input_digest: &str,
    ) -> Self {
        Self(format!(
            "{workspace}:{principal}:{generation}:{}:{}:{}:{input_digest}",
            model.name, model.revision, model.recipe
        ))
    }
}

pub(crate) struct KnowledgeQueryCache {
    entries: std::collections::VecDeque<(KnowledgeQueryCacheKey, std::time::Instant, Vec<f32>)>,
}

impl KnowledgeQueryCache {
    pub(super) fn new() -> Self {
        Self {
            entries: std::collections::VecDeque::new(),
        }
    }

    pub(crate) fn get(&mut self, key: &KnowledgeQueryCacheKey) -> Option<Vec<f32>> {
        let now = std::time::Instant::now();
        self.entries
            .retain(|(_, created, _)| now.duration_since(*created).as_secs() <= 60);
        self.entries
            .iter()
            .find(|(candidate, _, _)| candidate == key)
            .map(|(_, _, values)| values.clone())
    }

    pub(crate) fn put(&mut self, key: KnowledgeQueryCacheKey, values: Vec<f32>) {
        self.entries.retain(|(candidate, _, _)| candidate != &key);
        if self.entries.len() >= 64 {
            self.entries.pop_front();
        }
        self.entries
            .push_back((key, std::time::Instant::now(), values));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_returns_only_exact_key_and_replaces_existing_values() {
        let mut cache = KnowledgeQueryCache::new();
        let first = KnowledgeQueryCacheKey("first".into());
        let other = KnowledgeQueryCacheKey("other".into());
        assert_eq!(cache.get(&first), None);
        cache.put(first.clone(), vec![1.0]);
        assert_eq!(cache.get(&first), Some(vec![1.0]));
        assert_eq!(cache.get(&other), None);
        cache.put(first.clone(), vec![2.0]);
        assert_eq!(cache.get(&first), Some(vec![2.0]));
    }
}
