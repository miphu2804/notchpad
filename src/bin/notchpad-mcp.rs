//! Stdio MCP server exposing notchpad's task store (`tasks.txt`) to Claude.
//!
//! Speaks newline-delimited JSON-RPC 2.0. Tasks are addressed by `ref`: the task's `ext`
//! id when it has one (`linear:ENG-12`), otherwise its 1-based line number (`line:3`).
//! The running app notices file changes within ~2s and reloads.

#[path = "../days.rs"]
#[allow(dead_code)] // `label` is only used by the app
mod days;
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
    let time = json!({"type": "string", "description": "Time of day as 24-hour HH:MM (\"5pm\" -> \"17:00\"); \"\" clears"});
    let priority = json!({"type": "string", "enum": ["none", "low", "medium", "high"],
        "description": "\"!high\" -> high, \"!med\" -> medium, \"!low\" -> low; none clears"});
    let project = json!({"type": "string", "description": "Short project or area label, e.g. Work, Home (\"#work\" -> Work); \"\" clears"});
    let minutes = json!({"type": "integer", "minimum": 0, "description": "Estimated focus length (\"45m\" -> 45, \"1h\" -> 60); 0 clears"});
    let ext = json!({"type": "string", "description": "Stable external id, e.g. notion:<page-id>, linear:ENG-12, gcal:<event-id>"});
    json!([
        {"name": "plan_day",
         "description": "Plan a day or part of one: add or update several tasks in one atomic save. Use this whenever the user asks to plan their day, morning, afternoon or evening, or lists several things to do, e.g. \"plan my afternoon: call mom at 5pm, gym after work !high\". Split the request into one item per task with a short title (\"Call mom\", \"Gym\") and parse its metadata: a stated time -> `time`, !high/!med/!low -> `priority`, #tag or an obvious area (work, home, errands) -> `project`, a duration -> `minutes`. Leave `time` empty when the user gives none. An open task on the same day with the same title (or the same `ext`) is updated instead of duplicated.",
         "inputSchema": {"type": "object", "required": ["tasks"], "properties": {
             "day": {"type": "string", "description": "YYYY-MM-DD for items without their own day; default today"},
             "tasks": {"type": "array", "items": {"type": "object", "required": ["text"], "properties": {
                 "text": {"type": "string"}, "day": day, "time": time, "priority": priority,
                 "project": project, "minutes": minutes, "ext": ext}}}}}},
        {"name": "list_tasks",
         "description": "List notchpad tasks as JSON, each with ref, text, done, day, ext, time, priority, project and minutes.",
         "inputSchema": {"type": "object", "properties": {
             "day": {"type": "string", "description": "Only tasks on this YYYY-MM-DD day (\"\" = Inbox)"},
             "include_done": {"type": "boolean", "description": "Default true"}}}},
        {"name": "add_task",
         "description": "Add one task. For several tasks or a plan, use plan_day instead. With `ext`, behaves as an upsert for syncing: an existing task with that ext keeps its day, ORs its done flag and takes the new text and any given metadata.",
         "inputSchema": {"type": "object", "required": ["text"], "properties": {
             "text": {"type": "string"}, "day": day, "done": {"type": "boolean"}, "ext": ext,
             "time": time, "priority": priority, "project": project, "minutes": minutes}}},
        {"name": "update_task",
         "description": "Change a task's text, day, done flag, time, priority, project or minutes. Only the given fields change.",
         "inputSchema": {"type": "object", "required": ["ref"], "properties": {
             "ref": reference, "text": {"type": "string"}, "day": day, "done": {"type": "boolean"},
             "time": time, "priority": priority, "project": project, "minutes": minutes}}},
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
                    json!({"ref": task_ref(i, t), "text": t.text, "done": t.done, "day": t.day, "ext": t.ext,
                           "time": t.time, "priority": t.priority, "project": t.project, "minutes": t.minutes})
                })
                .collect();
            return Ok(Value::Array(rows).to_string());
        }
        "add_task" => {
            let text = required_text(args)?;
            let ext = str_arg("ext").unwrap_or_default();
            let done = args["done"].as_bool().unwrap_or(false);
            let existing = (!ext.is_empty()).then(|| tasks.iter().position(|t| t.ext == ext)).flatten();
            match existing {
                Some(i) => {
                    tasks[i].text = text;
                    tasks[i].done |= done;
                    apply_meta(&mut tasks[i], args)?;
                    format!("updated {ext}")
                }
                None => {
                    let day = day_arg(args)?.unwrap_or_default();
                    let mut task = Task { text, done, day, ext, ..Task::default() };
                    apply_meta(&mut task, args)?;
                    tasks.push(task);
                    format!("added {}", task_ref(tasks.len() - 1, &tasks[tasks.len() - 1]))
                }
            }
        }
        "update_task" => {
            let i = find(&tasks, args)?;
            if args["text"].is_string() {
                tasks[i].text = required_text(args)?;
            }
            if let Some(day) = day_arg(args)? {
                tasks[i].day = day;
            }
            if let Some(done) = args["done"].as_bool() {
                tasks[i].set_done(done);
            }
            apply_meta(&mut tasks[i], args)?;
            format!("updated {}", task_ref(i, &tasks[i]))
        }
        "plan_day" => plan_day(&mut tasks, args)?,
        "complete_task" => {
            let i = find(&tasks, args)?;
            tasks[i].set_done(args["done"].as_bool().unwrap_or(true));
            format!("{} done={}", task_ref(i, &tasks[i]), tasks[i].done)
        }
        "move_task" => {
            let i = find(&tasks, args)?;
            tasks[i].day = day_arg(args)?.ok_or("`day` is required")?;
            format!("{} moved to {:?}", task_ref(i, &tasks[i]), tasks[i].day)
        }
        _ => return Err(format!("unknown tool: {name}")),
    };
    store::save(&tasks).map_err(|e| format!("could not save tasks: {e}"))?;
    Ok(result)
}

/// Upserts every item of `args.tasks`. Any invalid item fails the whole call before the
/// caller saves, so a plan lands completely or not at all.
fn plan_day(tasks: &mut Vec<Task>, args: &Value) -> Result<String, String> {
    let items = args["tasks"].as_array().filter(|a| !a.is_empty()).ok_or("`tasks` must be a non-empty array")?;
    let default_day = day_arg(args)?.unwrap_or_else(|| days::key(0));
    let mut refs = Vec::new();
    for (n, item) in items.iter().enumerate() {
        let at = |e: String| format!("tasks[{n}]: {e}");
        let text = required_text(item).map_err(at)?;
        let day = day_arg(item).map_err(at)?.unwrap_or_else(|| default_day.clone());
        let ext = item["ext"].as_str().unwrap_or_default().to_owned();
        let existing = tasks.iter().position(|t| {
            if ext.is_empty() { !t.done && t.day == day && t.text.eq_ignore_ascii_case(&text) } else { t.ext == ext }
        });
        let i = existing.unwrap_or_else(|| {
            tasks.push(Task { ext, ..Task::default() });
            tasks.len() - 1
        });
        tasks[i].text = text;
        tasks[i].day = day;
        apply_meta(&mut tasks[i], item).map_err(at)?;
        refs.push(format!("{} {}", if existing.is_some() { "updated" } else { "added" }, task_ref(i, &tasks[i])));
    }
    Ok(format!("planned {} task(s): {}", refs.len(), refs.join(", ")))
}

/// Copies whichever of time, priority, project and minutes `args` carries onto `task`.
fn apply_meta(task: &mut Task, args: &Value) -> Result<(), String> {
    if let Some(time) = args["time"].as_str() {
        task.time = parse_time(time)?;
    }
    if let Some(priority) = args["priority"].as_str() {
        task.priority = match priority.trim().to_ascii_lowercase().as_str() {
            "" | "none" => String::new(),
            "med" => "medium".to_owned(),
            p @ ("low" | "medium" | "high") => p.to_owned(),
            other => return Err(format!("priority must be none, low, medium or high, got {other:?}")),
        };
    }
    if let Some(project) = args["project"].as_str() {
        task.project = single_line(project);
    }
    if !args["minutes"].is_null() {
        let minutes = args["minutes"].as_u64().filter(|m| *m <= 24 * 60).ok_or("minutes must be a whole number 0-1440")?;
        task.minutes = minutes as u32;
    }
    Ok(())
}

/// Normalises `H:MM` / `HH:MM` (24-hour) to `HH:MM`; empty clears the time.
fn parse_time(time: &str) -> Result<String, String> {
    let time = time.trim();
    if time.is_empty() {
        return Ok(String::new());
    }
    let parsed = time.split_once(':').and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)));
    match parsed {
        Some((h, m)) if h < 24 && m < 60 && time.len() <= 5 => Ok(format!("{h:02}:{m:02}")),
        _ => Err(format!("time must be 24-hour HH:MM, got {time:?}")),
    }
}

/// The `day` argument, if present: `YYYY-MM-DD` or `""` for the Inbox.
fn day_arg(args: &Value) -> Result<Option<String>, String> {
    let Some(day) = args["day"].as_str() else { return Ok(None) };
    let b = day.as_bytes();
    let valid = day.is_empty()
        || (b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()));
    if valid { Ok(Some(day.to_owned())) } else { Err(format!("day must be YYYY-MM-DD or \"\", got {day:?}")) }
}

fn required_text(args: &Value) -> Result<String, String> {
    args["text"].as_str().map(single_line).filter(|t| !t.is_empty()).ok_or_else(|| "`text` is required".to_owned())
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
