use crate::store::Store;
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
fn tools() -> Value {
    json!([
     {"name":"dramafactory_list_series","description":"List local series","inputSchema":{"type":"object","properties":{}}},
     {"name":"dramafactory_create_series","description":"Create a series","inputSchema":{"type":"object","properties":{"name":{"type":"string"},"premise":{"type":"string"}},"required":["name"]}},
     {"name":"dramafactory_create_episode","description":"Create an episode","inputSchema":{"type":"object","properties":{"series_id":{"type":"string"},"title":{"type":"string"}},"required":["series_id","title"]}},
     {"name":"dramafactory_list_episodes","description":"List episodes","inputSchema":{"type":"object","properties":{"series_id":{"type":"string"}},"required":["series_id"]}},
     {"name":"dramafactory_save_screenplay","description":"Save episode screenplay","inputSchema":{"type":"object","properties":{"episode_id":{"type":"string"},"text":{"type":"string"}},"required":["episode_id","text"]}},
     {"name":"dramafactory_queue_render","description":"Persist a render request. Currently blocked until a verified local worker is configured.","inputSchema":{"type":"object","properties":{"prompt":{"type":"string"}},"required":["prompt"]}},
     {"name":"dramafactory_list_jobs","description":"List durable render requests","inputSchema":{"type":"object","properties":{}}},
     {"name":"dramafactory_cancel_job","description":"Cancel a waiting request","inputSchema":{"type":"object","properties":{"job_id":{"type":"string"}},"required":["job_id"]}}
    ])
}
pub fn run(store: Store) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut out = io::stdout().lock();
    for line in stdin.lock().lines() {
        let v: Value = match serde_json::from_str(&line?) {
            Ok(v) => v,
            Err(_) => {
                writeln!(
                    out,
                    "{}",
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}})
                )?;
                out.flush()?;
                continue;
            }
        };
        let Some(id) = v.get("id") else { continue };
        let result = match v["method"].as_str().unwrap_or("") {
            "initialize" => Ok(
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"DramaFactory","version":"0.1.0"}}),
            ),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools":tools()})),
            "tools/call" => {
                let a = &v["params"]["arguments"];
                let get = |k: &str| a[k].as_str().unwrap_or("");
                let r: anyhow::Result<Value> = (|| {
                    Ok(match v["params"]["name"].as_str().unwrap_or("") {
                        "dramafactory_list_series" => serde_json::to_value(store.series()?)?,
                        "dramafactory_create_series" => {
                            serde_json::to_value(store.create_series(get("name"), get("premise"))?)?
                        }
                        "dramafactory_create_episode" => serde_json::to_value(
                            store.create_episode(get("series_id"), get("title"))?,
                        )?,
                        "dramafactory_list_episodes" => {
                            serde_json::to_value(store.episodes(get("series_id"))?)?
                        }
                        "dramafactory_save_screenplay" => {
                            store.save_screenplay(get("episode_id"), get("text"))?;
                            json!({"saved":true})
                        }
                        "dramafactory_queue_render" => {
                            serde_json::to_value(store.queue(get("prompt"))?)?
                        }
                        "dramafactory_list_jobs" => serde_json::to_value(store.jobs()?)?,
                        "dramafactory_cancel_job" => {
                            store.cancel(get("job_id"))?;
                            json!({"canceled":true})
                        }
                        _ => anyhow::bail!("Unknown tool"),
                    })
                })();
                Ok(match r {
                    Ok(x) => {
                        json!({"content":[{"type":"text","text":x.to_string()}],"isError":false})
                    }
                    Err(e) => {
                        json!({"content":[{"type":"text","text":e.to_string()}],"isError":true})
                    }
                })
            }
            _ => Err("Method not found"),
        };
        let response = match result {
            Ok(r) => json!({"jsonrpc":"2.0","id":id,"result":r}),
            Err(e) => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":e}}),
        };
        writeln!(out, "{response}")?;
        out.flush()?;
    }
    Ok(())
}
