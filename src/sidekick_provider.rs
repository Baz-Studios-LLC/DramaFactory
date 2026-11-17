//! Local Sidekick routing preferences; never contains credentials or project data.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum Provider {
    #[default]
    OpenRouter,
    OpenAi,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Connection {
    pub provider: Provider,
    pub openai_model: String,
    #[serde(default = "default_effort")]
    pub effort: String,
}
fn default_effort() -> String {
    "medium".into()
}
impl Default for Connection {
    fn default() -> Self {
        Self {
            provider: Provider::OpenRouter,
            openai_model: "gpt-6-astra".into(),
            effort: default_effort(),
        }
    }
}
impl Connection {
    fn path() -> std::path::PathBuf {
        crate::store::default_path().with_file_name("sidekick-connection.json")
    }
    pub fn load() -> Result<Self> {
        match std::fs::read(Self::path()) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .context("Could not read Sidekick connection settings"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut connection = Self::default();
                if crate::credentials::Provider::OpenAi.read()?.is_some() {
                    connection.provider = Provider::OpenAi;
                }
                Ok(connection)
            }
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self) -> Result<()> {
        self.validate()?;
        let path = Self::path();
        std::fs::create_dir_all(path.parent().context("Missing settings directory")?)?;
        let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        if self.openai_model.is_empty()
            || self.openai_model.len() > 128
            || !self
                .openai_model
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
        {
            bail!("Enter an OpenAI model ID, such as gpt-6-astra.");
        }
        if !["low", "medium", "high", "xhigh", "max"].contains(&self.effort.as_str()) {
            bail!("Choose a supported effort level");
        }
        Ok(())
    }
    pub fn name(&self) -> &'static str {
        match self.provider {
            Provider::OpenRouter => "OpenRouter",
            Provider::OpenAi => "OpenAI",
        }
    }
    pub fn endpoint(&self) -> &'static str {
        match self.provider {
            Provider::OpenRouter => "https://openrouter.ai/api/v1/chat/completions",
            Provider::OpenAi => "https://api.openai.com/v1/responses",
        }
    }
    pub fn credential(&self) -> crate::credentials::Provider {
        match self.provider {
            Provider::OpenRouter => crate::credentials::Provider::OpenRouter,
            Provider::OpenAi => crate::credentials::Provider::OpenAi,
        }
    }
    pub fn wire_request(&self, request: &serde_json::Value) -> serde_json::Value {
        use serde_json::{Value, json};
        if self.provider != Provider::OpenAi {
            return request.clone();
        }
        let mut input = Vec::new();
        for message in request["messages"].as_array().into_iter().flatten() {
            if let Some(output) = message["_response_output"].as_array() {
                input.extend(output.clone());
            } else if message["role"] == "tool" {
                input.push(json!({"type":"function_call_output","call_id":message["tool_call_id"],"output":message["content"]}));
            } else {
                let mut content = message["content"].clone();
                if let Some(parts) = content.as_array_mut() {
                    for part in parts {
                        if part["type"] == "image_url" {
                            *part =
                                json!({"type":"input_image","image_url":part["image_url"]["url"]});
                        } else if part["type"] == "text" {
                            part["type"] = json!("input_text");
                        }
                    }
                }
                input.push(json!({"role":message["role"],"content":content}));
            }
        }
        let mut format = request["response_format"].clone();
        if format["type"] == "json_schema" {
            format = format["json_schema"].clone();
            format["type"] = json!("json_schema");
        }
        let mut wire = json!({"model":self.openai_model,"input":input,"store":false,
            "max_output_tokens":request["max_completion_tokens"],
            "reasoning":{"effort":self.effort},"include":["reasoning.encrypted_content"],
            "text":{"format":format}});
        if let Some(tools) = request["tools"].as_array() {
            wire["tools"] = Value::Array(
                tools
                    .iter()
                    .map(|tool| {
                        let mut function = tool["function"].clone();
                        function["type"] = json!("function");
                        function["strict"] = json!(false);
                        function
                    })
                    .collect(),
            );
        }
        wire
    }
    pub fn normalize_response(&self, value: serde_json::Value) -> serde_json::Value {
        use serde_json::json;
        if self.provider != Provider::OpenAi || !value["error"].is_null() {
            return value;
        }
        let mut text = String::new();
        let mut calls = Vec::new();
        for item in value["output"].as_array().into_iter().flatten() {
            if item["type"] == "function_call" {
                calls.push(json!({"id":item["call_id"],"type":"function","function":{"name":item["name"],"arguments":item["arguments"]}}));
            }
            for part in item["content"].as_array().into_iter().flatten() {
                if let Some(part) = part["text"].as_str() {
                    text.push_str(part);
                }
            }
        }
        json!({"id":value["id"],"provider":"OpenAI",
            "usage":{"prompt_tokens":value["usage"]["input_tokens"],"completion_tokens":value["usage"]["output_tokens"]},
            "choices":[{"finish_reason":if value["status"] == "completed" {"stop"} else {"length"},
                "message":{"role":"assistant","content":text,"tool_calls":calls,"_response_output":value["output"]}}]})
    }
    pub fn configure(&self, request: &mut serde_json::Value) {
        if self.provider == Provider::OpenAi {
            request["model"] = serde_json::json!(self.openai_model);
            let object = request.as_object_mut().expect("request object");
            object.remove("provider");
            if let Some(limit) = object.remove("max_tokens") {
                object.insert("max_completion_tokens".into(), limit);
            }
            object.insert("store".into(), false.into());
        }
    }
}
pub fn test_openai_connection() -> Result<String> {
    let key = crate::credentials::Provider::OpenAi
        .read()?
        .context("Save an OpenAI API key first")?;
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .max_redirects(0)
        .build();
    let agent: ureq::Agent = config.into();
    agent
        .get("https://api.openai.com/v1/models")
        .header("Authorization", &format!("Bearer {key}"))
        .call()
        .map_err(|_| anyhow::anyhow!("OpenAI connection failed. Check your key and connection."))?;
    Ok("OpenAI connected. Model access is checked when you send a message.".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn responses_preserve_portrait_tool_round_and_effort() {
        use serde_json::json;
        let c = Connection {
            provider: Provider::OpenAi,
            effort: "high".into(),
            ..Default::default()
        };
        let mut request = json!({"messages":[{"role":"user","content":[{"type":"text","text":"look"},{"type":"image_url","image_url":{"url":"data:image/png;base64,test"}}]}],"max_tokens":16384,"response_format":{"type":"json_schema","json_schema":{"name":"edit","strict":true,"schema":{"type":"object"}}},"tools":[{"type":"function","function":{"name":"view_character_portraits","parameters":{"type":"object"}}}]});
        c.configure(&mut request);
        let wire = c.wire_request(&request);
        assert_eq!(wire["reasoning"]["effort"], "high");
        assert_eq!(wire["input"][0]["content"][1]["type"], "input_image");
        assert_eq!(wire["tools"][0]["name"], "view_character_portraits");
        assert_eq!(wire["text"]["format"]["name"], "edit");
        let output = json!([{"type":"reasoning","encrypted_content":"fixture"},{"type":"function_call","call_id":"call1","name":"view_character_portraits","arguments":"{}"}]);
        let normalized =
            c.normalize_response(json!({"status":"completed","error":null,"output":output}));
        request["messages"]
            .as_array_mut()
            .unwrap()
            .push(normalized["choices"][0]["message"].clone());
        request["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"tool","tool_call_id":"call1","content":"images follow"}));
        let wire = c.wire_request(&request);
        assert_eq!(wire["input"][1], output[0]);
        assert_eq!(wire["input"][2], output[1]);
        assert_eq!(wire["input"][3]["call_id"], "call1");
        assert_eq!(wire["input"][3]["type"], "function_call_output");
        assert_eq!(
            c.normalize_response(json!({"status":"incomplete","output":[]}))["choices"][0]["finish_reason"],
            "length"
        );
    }
    #[test]
    fn direct_requests_keep_tools_images_and_schema_but_remove_router_options() {
        let mut request = serde_json::json!({"model":"google/example","max_tokens":16384,"provider":{"sort":"throughput"},"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,fixture"}}]}],"tools":[{"type":"function"}],"response_format":{"type":"json_object"}});
        let original = request.clone();
        let connection = Connection {
            provider: Provider::OpenAi,
            ..Default::default()
        };
        connection.configure(&mut request);
        assert_eq!(request["model"], "gpt-6-astra");
        assert!(request.get("provider").is_none());
        assert!(request.get("max_tokens").is_none());
        assert_eq!(request["max_completion_tokens"], 16384);
        for field in ["messages", "tools", "response_format"] {
            assert_eq!(request[field], original[field]);
        }
        let mut router = original.clone();
        Connection::default().configure(&mut router);
        assert_eq!(router, original);
        assert_ne!(connection.endpoint(), Connection::default().endpoint());
    }
}
