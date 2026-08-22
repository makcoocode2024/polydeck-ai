//! Prompt template management with variable rendering.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PromptTemplate {
    pub id: String,
    pub name: String,
    pub content: String,
    pub variables: Vec<String>,
    pub scope: PromptScope,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum PromptScope {
    Global,
    Profile,
}

pub struct PromptStore {
    prompts: Vec<PromptTemplate>,
}

impl PromptStore {
    pub fn new() -> Self {
        Self { prompts: vec![] }
    }

    pub fn list(&self) -> Vec<PromptTemplate> {
        self.prompts.clone()
    }

    pub fn add(&mut self, prompt: PromptTemplate) {
        self.prompts.push(prompt);
    }

    pub fn remove(&mut self, id: &str) {
        self.prompts.retain(|p| p.id != id);
    }

    pub fn render(template: &str, vars: &HashMap<String, String>) -> String {
        let mut result = template.to_string();
        for (key, value) in vars {
            result = result.replace(&format!("{{{key}}}"), value);
        }
        result
    }
}
