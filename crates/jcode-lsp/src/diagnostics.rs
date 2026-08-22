use crate::protocol::Diagnostic;
use std::collections::BTreeMap;

#[derive(Debug, Default, Clone)]
pub struct DiagnosticStore {
    entries: BTreeMap<String, Vec<Diagnostic>>,
}

impl DiagnosticStore {
    pub fn replace(&mut self, uri: &str, diagnostics: Vec<Diagnostic>) {
        self.entries.insert(
            uri.to_owned(),
            diagnostics
                .into_iter()
                .filter(|diagnostic| !diagnostic.stale)
                .collect(),
        );
    }

    pub fn get(&self, uri: &str) -> Vec<Diagnostic> {
        self.entries
            .get(uri)
            .map_or_else(Vec::new, |diagnostics| diagnostics.clone())
    }

    pub fn all(&self) -> Vec<Diagnostic> {
        self.entries.values().flatten().cloned().collect()
    }

    pub fn count(&self) -> usize {
        self.entries.values().map(Vec::len).sum()
    }

    pub fn clear(&mut self, uri: &str) {
        self.entries.remove(uri);
    }
}
