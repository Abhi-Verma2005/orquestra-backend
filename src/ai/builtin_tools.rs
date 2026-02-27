use async_trait::async_trait;
use serde_json::{json, Value};

use crate::ai::tools::{Tool, ToolDefinition};

pub struct CurrentTimeTool;

#[async_trait]
impl Tool for CurrentTimeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_current_time".to_string(),
            description: "Get the current date and time in UTC".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, _arguments: Value) -> anyhow::Result<String> {
        Ok(chrono::Utc::now().to_rfc3339())
    }
}

pub struct CalculatorTool;

#[async_trait]
impl Tool for CalculatorTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "calculator".to_string(),
            description: "Perform basic math calculations. Supports +, -, *, /.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "Math expression like '2 + 2' or '10 * 5'"
                    }
                },
                "required": ["expression"]
            }),
        }
    }

    async fn execute(&self, arguments: Value) -> anyhow::Result<String> {
        let expr = arguments
            .get("expression")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Missing expression"))?
            .trim();

        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(anyhow::anyhow!(
                "Expression must be in the form: <number> <operator> <number>"
            ));
        }

        let lhs: f64 = parts[0].parse()?;
        let rhs: f64 = parts[2].parse()?;

        let result = match parts[1] {
            "+" => lhs + rhs,
            "-" => lhs - rhs,
            "*" => lhs * rhs,
            "/" => {
                if rhs == 0.0 {
                    return Err(anyhow::anyhow!("Division by zero"));
                }
                lhs / rhs
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unsupported operator. Use one of: + - * /"
                ));
            }
        };

        if (result.fract() - 0.0).abs() < f64::EPSILON {
            Ok((result as i64).to_string())
        } else {
            Ok(result.to_string())
        }
    }
}
