//! Stdio MCP server exposing notchpad's task store (`tasks.txt`) to Claude.
//!
//! Speaks newline-delimited JSON-RPC 2.0. Tasks are addressed by `ref`: the task's `ext`
//! id when it has one (`linear:ENG-12`), otherwise its 1-based line number (`line:3`).
//! The running app notices file changes within ~2s and reloads.

#[path = "../store.rs"]
#[allow(dead_code)] // `modified` is only used by the app
mod store;

use std::io::{self, BufRead, Write};

use serde_json::{Value, json};
use store::Task;

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        // Notifications carry no id and get no response.
        let Some(id) = msg.get("id").cloned() else { continue };
        let reply = match handle(msg["method"].as_str().unwrap_or(""), &msg["params"]) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(message) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": message}}),
        };
        let _ = writeln!(stdout, "{reply}").and_then(|()| stdout.flush());
    }
}

fn handle(method: &str, params: &Value) -> Result<Value, String> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "notchpad", "version": env!("CARGO_PKG_VERSION")},
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_list()})),
        "tools/call" => {
            let args = &params["arguments"];
            let outcome = call_tool(params["name"].as_str().unwrap_or(""), args);
            let (text, is_error) = match outcome {
                Ok(text) => (text, false),
                Err(text) => (text, true),
            };
            Ok(json!({"content": [{"type": "text", "text": text}], "isError": is_error}))
        }
        _ => Err(format!("method not found: {method}")),
    }
}

fn tool_list() -> Value {
    let day = json!({"type": "string", "description": "YYYY-MM-DD, or \"\" for the Inbox"});
    let reference = json!({"type": "string", "description": "A task's `ref` from list_tasks (ext id or line:N)"});
    json!([
        {"name": "list_tasks",
         "description": "List notchpad tasks as JSON, each with ref, text, done, day and ext.",
         "inputSchema": {"type": "object", "properties": {
             "day": {"type": "string", "description": "Only tasks on this YYYY-MM-DD day (\"\" = Inbox)"},
             "include_done": {"type": "boolean", "description": "Default true"}}}},
        {"name": "add_task",
         "description": "Add a task. With `ext`, behaves as an upsert for syncing: an existing task with that ext keeps its day, ORs its done flag and takes the new text.",
         "inputSchema": {"type": "object", "required": ["text"], "properties": {
             "text": {"type": "string"}, "day": day, "done": {"type": "boolean"},
             "ext": {"type": "string", "description": "Stable external id, e.g. notion:<page-id>, linear:ENG-12, gcal:<event-id>"}}}},
        {"name": "complete_task",
         "description": "Mark a task done (or not done with done=false).",
         "inputSchema": {"type": "object", "required": ["ref"], "properties": {
             "ref": reference, "done": {"type": "boolean", "description": "Default true"}}}},
        {"name": "move_task",
         "description": "Move a task to another day (\"\" = Inbox).",
         "inputSchema": {"type": "object", "required": ["ref", "day"], "properties": {"ref": reference, "day": day}}},
    ])
}

fn call_tool(name: &str, args: &Value) -> Result<String, String> {
    let mut tasks = store::load();
    let str_arg = |key: &str| args[key].as_str().map(str::to_owned);
    let result = match name {
        "list_tasks" => {
            let day = str_arg("day");
            let include_done = args["include_done"].as_bool().unwrap_or(true);
            let rows: Vec<Value> = tasks
                .iter()
                .enumerate()
                .filter(|(_, t)| day.as_ref().is_none_or(|d| *d == t.day) && (include_done || !t.done))
                .map(|(i, t)| {
                    json!({"ref": task_ref(i, t), "text": t.text, "done": t.done, "day": t.day, "ext": t.ext})
                })
                .collect();
            return Ok(Value::Array(rows).to_string());
        }
        "add_task" => {
            let text = str_arg("text").filter(|t| !t.trim().is_empty()).ok_or("`text` is required")?;
            let ext = str_arg("ext").unwrap_or_default();
            let done = args["done"].as_bool().unwrap_or(false);
            let existing = (!ext.is_empty()).then(|| tasks.iter().position(|t| t.ext == ext)).flatten();
            match existing {
                Some(i) => {
                    tasks[i].text = single_line(&text);
                    tasks[i].done |= done;
                    format!("updated {ext}")
                }
                None => {
                    let day = str_arg("day").unwrap_or_default();
                    tasks.push(Task { text: single_line(&text), done, day, ext: ext.clone() });
                    format!("added {}", task_ref(tasks.len() - 1, &tasks[tasks.len() - 1]))
                }
            }
        }
        "complete_task" => {
            let i = find(&tasks, args)?;
            tasks[i].done = args["done"].as_bool().unwrap_or(true);
            format!("{} done={}", task_ref(i, &tasks[i]), tasks[i].done)
        }
        "move_task" => {
            let i = find(&tasks, args)?;
            tasks[i].day = str_arg("day").ok_or("`day` is required")?;
            format!("{} moved to {:?}", task_ref(i, &tasks[i]), tasks[i].day)
        }
        _ => return Err(format!("unknown tool: {name}")),
    };
    store::save(&tasks).map_err(|e| format!("could not save tasks: {e}"))?;
    Ok(result)
}

fn task_ref(index: usize, task: &Task) -> String {
    if task.ext.is_empty() { format!("line:{}", index + 1) } else { task.ext.clone() }
}

fn find(tasks: &[Task], args: &Value) -> Result<usize, String> {
    let reference = args["ref"].as_str().ok_or("`ref` is required")?;
    let found = match reference.strip_prefix("line:").and_then(|n| n.parse::<usize>().ok()) {
        Some(n) => n.checked_sub(1).filter(|i| *i < tasks.len()),
        None => tasks.iter().position(|t| t.ext == reference),
    };
    found.ok_or_else(|| format!("no task with ref {reference}"))
}

/// The store is one task per line, so tabs and newlines in text would corrupt it.
fn single_line(text: &str) -> String {
    text.split(['\t', '\n', '\r']).map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}
