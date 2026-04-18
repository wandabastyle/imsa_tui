use std::collections::HashMap;

use serde_json::{Map, Value};

#[derive(Debug, Default, Clone)]
pub struct CollectionStore {
    collections: HashMap<String, HashMap<String, Value>>,
}

impl CollectionStore {
    pub fn apply_added(&mut self, collection: &str, id: &str, fields: Map<String, Value>) {
        let docs = self.collections.entry(collection.to_string()).or_default();
        docs.insert(id.to_string(), Value::Object(fields));
    }

    pub fn apply_changed(
        &mut self,
        collection: &str,
        id: &str,
        fields: Map<String, Value>,
        cleared: &[String],
    ) {
        let docs = self.collections.entry(collection.to_string()).or_default();
        let doc = docs
            .entry(id.to_string())
            .or_insert_with(|| Value::Object(Map::new()));

        let object = match doc {
            Value::Object(object) => object,
            _ => {
                *doc = Value::Object(Map::new());
                match doc {
                    Value::Object(object) => object,
                    _ => return,
                }
            }
        };

        for (key, value) in fields {
            object.insert(key, value);
        }
        for key in cleared {
            object.remove(key);
        }
    }

    pub fn apply_removed(&mut self, collection: &str, id: &str) {
        if let Some(docs) = self.collections.get_mut(collection) {
            docs.remove(id);
            if docs.is_empty() {
                self.collections.remove(collection);
            }
        }
    }

    pub fn collection(&self, name: &str) -> Option<&HashMap<String, Value>> {
        self.collections.get(name)
    }

    pub fn collections(&self) -> &HashMap<String, HashMap<String, Value>> {
        &self.collections
    }

    pub fn collection_count_summary(&self) -> String {
        let mut parts: Vec<(String, usize)> = self
            .collections
            .iter()
            .map(|(name, docs)| (name.clone(), docs.len()))
            .collect();
        parts.sort_by(|a, b| a.0.cmp(&b.0));
        if parts.is_empty() {
            return "none".to_string();
        }
        parts
            .into_iter()
            .map(|(name, count)| format!("{name}={count}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}
