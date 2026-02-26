use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait ToolPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn execute(&self, input: Value) -> anyhow::Result<Value>;
}

pub struct ToolRegistry {
    tools: Vec<Box<dyn ToolPlugin>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: vec![] }
    }

    pub fn register(&mut self, tool: Box<dyn ToolPlugin>) {
        self.tools.push(tool);
    }

    pub fn list(&self) -> &[Box<dyn ToolPlugin>] {
        &self.tools
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
