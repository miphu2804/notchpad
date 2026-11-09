//! Linear: the issues assigned to the user become tasks (`ext = linear:<identifier>`), and
//! completing such a task in Notchpad moves the issue to its team's completed state.
//!
//! The personal API key lives only in the macOS Keychain. HTTP goes through `curl`, which reads
//! its URL, headers and body from a config on stdin, so the key never appears in a process listing.
//! Every request runs on a background thread; the main thread is told to pick up the result.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use objc2::msg_send;
use objc2::runtime::{AnyObject, Sel};
use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
use serde_json::{Value, json};

use crate::store::Task;

const ENDPOINT: &str = "https://api.linear.app/graphql";
const SERVICE: &str = "notchpad.linear";
const ACCOUNT: &str = "api-key";
const SYNC_EVERY: Duration = Duration::from_secs(10 * 60);
const EXT_PREFIX: &str = "linear:";

const VIEWER: &str = "query { viewer { id } }";
const ASSIGNED: &str = r#"query { viewer { assignedIssues(filter: {state: {type: {nin: ["completed", "canceled"]}}}) { nodes { id identifier title dueDate } } } }"#;
const COMPLETED_STATE: &str = r#"query($id: String!) { issue(id: $id) { id team { states(filter: {type: {eq: "completed"}}) { nodes { id } } } } }"#;
const SET_STATE: &str = "mutation($id: String!, $state: String!) { issueUpdate(id: $id, input: {stateId: $state}) { success } }";

/// An open issue assigned to the user.
#[derive(Debug, PartialEq)]
pub struct Issue {
    pub identifier: String,
    pub title: String,
    /// `YYYY-MM-DD`, or empty.
    pub due: String,
}

// ---- Keychain ----------------------------------------------------------------------------

/// Saves the key after checking that Linear accepts it, so a typo is caught on the spot.
pub fn connect(key: &str) -> Result<(), String> {
    request_with(key, VIEWER, json!({})).and_then(|text| data(&text))?;
    set_generic_password(SERVICE, ACCOUNT, key.as_bytes()).map_err(|err| format!("Keychain: {err}"))
}

pub fn disconnect() {
    let _ = delete_generic_password(SERVICE, ACCOUNT);
}

fn key() -> Result<String, String> {
    let bytes = get_generic_password(SERVICE, ACCOUNT).map_err(|_| "No Linear key in the Keychain".to_owned())?;
    String::from_utf8(bytes).map_err(|_| "The Linear key in the Keychain is not text".to_owned())
}

// ---- Sync --------------------------------------------------------------------------------

/// When the assigned issues were last fetched and the fetch in flight, if any.
#[derive(Default)]
pub struct Linear {
    last: Option<Instant>,
    pending: Option<Receiver<Result<Vec<Issue>, String>>>,
}

impl Linear {
    /// Whether the next fetch is due: none yet, or the last one was ten minutes ago. A failed fetch counts,
    /// so a dead network or a revoked key is retried at the same slow pace instead of in a loop.
    pub fn due(&self) -> bool {
        self.pending.is_none() && self.last.is_none_or(|at| at.elapsed() >= SYNC_EVERY)
    }

    /// Forgets the schedule and any fetch in flight; the next check fetches at once.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Fetches on a background thread, then has `target` call `selector` on the main thread.
    pub fn fetch(&mut self, target: &AnyObject, selector: Sel) {
        let (sender, receiver) = mpsc::channel();
        self.last = Some(Instant::now());
        self.pending = Some(receiver);
        let target = target as *const AnyObject as usize;
        thread::spawn(move || {
            let issues = request(ASSIGNED, json!({})).and_then(|text| parse_issues(&text));
            if sender.send(issues).is_ok() {
                let target = target as *const AnyObject;
                let _: () = unsafe { msg_send![target, performSelectorOnMainThread: selector, withObject: std::ptr::null::<AnyObject>(), waitUntilDone: false] };
            }
        });
    }

    /// The finished fetch, if there is one.
    pub fn take(&mut self) -> Option<Result<Vec<Issue>, String>> {
        let result = self.pending.as_ref()?.try_recv().ok()?;
        self.pending = None;
        Some(result)
    }

    /// Moves the issue behind a task's `ext` to its team's completed state. Failures only go to stderr.
    pub fn complete(ext: &str) {
        let Some(identifier) = ext.strip_prefix(EXT_PREFIX).map(str::to_owned) else { return };
        thread::spawn(move || {
            if let Err(err) = set_completed(&identifier) {
                eprintln!("notchpad: could not complete {identifier} in Linear: {err}");
            }
        });
    }
}

fn set_completed(identifier: &str) -> Result<(), String> {
    let lookup = request(COMPLETED_STATE, json!({"id": identifier}))?;
    let (issue, state) = parse_completed_state(&lookup)?;
    let reply = request(SET_STATE, json!({"id": issue, "state": state}))?;
    match data(&reply)?["issueUpdate"]["success"].as_bool() {
        Some(true) => Ok(()),
        _ => Err("Linear did not accept the update".to_owned()),
    }
}

// ---- HTTP --------------------------------------------------------------------------------

fn request(query: &str, variables: Value) -> Result<String, String> {
    request_with(&key()?, query, variables)
}

fn request_with(key: &str, query: &str, variables: Value) -> Result<String, String> {
    post(ENDPOINT, key, &json!({"query": query, "variables": variables}).to_string())
}

fn post(url: &str, key: &str, body: &str) -> Result<String, String> {
    let mut curl = Command::new("curl")
        .args(["--silent", "--show-error", "--max-time", "20", "--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("curl: {err}"))?;
    let config = curl_config(url, key, body);
    // Dropping stdin closes it, which is what tells curl the config is complete.
    curl.stdin.take().ok_or("curl has no stdin")?.write_all(config.as_bytes()).map_err(|err| err.to_string())?;
    let output = curl.wait_with_output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(stderr.lines().next().unwrap_or("curl failed").to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A curl config that POSTs `body` to `url`; `key` is Linear's personal key, sent as the Authorization header.
fn curl_config(url: &str, key: &str, body: &str) -> String {
    format!(
        "url = {}\nrequest = \"POST\"\nheader = {}\nheader = \"Content-Type: application/json\"\ndata = {}\n",
        quote(url),
        quote(&format!("Authorization: {key}")),
        quote(body),
    )
}

/// `text` as a curl config string: double-quoted, with `\`, `"` and line breaks escaped.
fn quote(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r");
    format!("\"{escaped}\"")
}

// ---- Parsing -----------------------------------------------------------------------------

/// The `data` of a GraphQL reply, or the first error Linear reported.
fn data(json: &str) -> Result<Value, String> {
    let mut reply: Value = serde_json::from_str(json).map_err(|_| "Linear sent a reply that is not JSON".to_owned())?;
    if let Some(message) = reply["errors"][0]["message"].as_str() {
        return Err(message.to_owned());
    }
    Ok(reply["data"].take())
}

pub fn parse_issues(json: &str) -> Result<Vec<Issue>, String> {
    let nodes = data(json)?["viewer"]["assignedIssues"]["nodes"].take();
    let nodes = nodes.as_array().ok_or("Linear's reply has no issues list")?;
    let text = |node: &Value, field: &str| node[field].as_str().unwrap_or_default().to_owned();
    Ok(nodes
        .iter()
        .filter(|node| node["identifier"].as_str().is_some_and(|id| !id.is_empty()))
        .map(|node| Issue { identifier: text(node, "identifier"), title: text(node, "title"), due: text(node, "dueDate") })
        .collect())
}

/// The issue's own id and the id of its team's first completed state.
fn parse_completed_state(json: &str) -> Result<(String, String), String> {
    let issue = data(json)?["issue"].take();
    let id = issue["id"].as_str().ok_or("Linear did not find the issue")?;
    let state = issue["team"]["states"]["nodes"][0]["id"].as_str().ok_or("The issue's team has no completed state")?;
    Ok((id.to_owned(), state.to_owned()))
}

/// Adds a task for each issue that has none yet and returns how many it added. An issue that already
/// has a task, finished or not, is left alone, so deleting nothing and re-running never duplicates.
/// New tasks go on their due date, else on `today`.
pub fn import(tasks: &mut Vec<Task>, issues: &[Issue], today: &str) -> usize {
    let before = tasks.len();
    for issue in issues {
        let ext = format!("{EXT_PREFIX}{}", issue.identifier);
        if tasks.iter().any(|task| task.ext == ext) {
            continue;
        }
        // Tabs and line breaks would corrupt the task file's columns.
        let text = issue.title.replace(['\t', '\n', '\r'], " ");
        let day = if issue.due.is_empty() { today } else { &issue.due };
        tasks.push(Task { text, day: day.to_owned(), ext, ..Task::default() });
    }
    tasks.len() - before
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = r#"{"data":{"viewer":{"assignedIssues":{"nodes":[
        {"id":"u1","identifier":"ENG-1","title":"Fix login","dueDate":"2026-11-12"},
        {"id":"u2","identifier":"ENG-2","title":"Write\tdocs","dueDate":null}]}}}}"#;

    fn issue(identifier: &str, due: &str) -> Issue {
        Issue { identifier: identifier.to_owned(), title: format!("Issue {identifier}"), due: due.to_owned() }
    }

    #[test]
    fn assigned_issues_are_parsed_with_missing_due_dates_empty() {
        let issues = parse_issues(REPLY).unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0], Issue { identifier: "ENG-1".into(), title: "Fix login".into(), due: "2026-11-12".into() });
        assert_eq!(issues[1].due, "");
    }

    #[test]
    fn errors_and_garbage_are_reported_not_parsed() {
        assert_eq!(parse_issues(r#"{"errors":[{"message":"Authentication required"}]}"#), Err("Authentication required".to_owned()));
        assert!(parse_issues("<html>").is_err());
        assert!(parse_issues(r#"{"data":{"viewer":null}}"#).is_err());
        assert_eq!(parse_issues(r#"{"data":{"viewer":{"assignedIssues":{"nodes":[]}}}}"#), Ok(Vec::new()));
    }

    #[test]
    fn import_keys_tasks_by_identifier_and_places_them_by_due_date() {
        let mut tasks = Vec::new();
        let added = import(&mut tasks, &parse_issues(REPLY).unwrap(), "2026-11-09");
        assert_eq!(added, 2);
        assert_eq!((tasks[0].ext.as_str(), tasks[0].day.as_str(), tasks[0].text.as_str()), ("linear:ENG-1", "2026-11-12", "Fix login"));
        assert_eq!((tasks[1].day.as_str(), tasks[1].text.as_str()), ("2026-11-09", "Write docs"));
    }

    #[test]
    fn import_skips_issues_that_already_have_a_task_even_a_finished_one() {
        let mut tasks = vec![Task { done: true, ext: "linear:ENG-1".into(), ..Task::default() }, Task { text: "Own".into(), ..Task::default() }];
        let issues = [issue("ENG-1", ""), issue("ENG-3", ""), issue("ENG-3", "")];
        assert_eq!(import(&mut tasks, &issues, "2026-11-09"), 1);
        assert_eq!(import(&mut tasks, &issues, "2026-11-09"), 0);
        assert_eq!(tasks.len(), 3);
        assert!(tasks[0].done);
    }

    #[test]
    fn completed_state_comes_from_the_issues_team() {
        let reply = r#"{"data":{"issue":{"id":"uuid-1","team":{"states":{"nodes":[{"id":"done-state"},{"id":"other"}]}}}}}"#;
        assert_eq!(parse_completed_state(reply), Ok(("uuid-1".to_owned(), "done-state".to_owned())));
        assert!(parse_completed_state(r#"{"data":{"issue":null}}"#).is_err());
        assert!(parse_completed_state(r#"{"data":{"issue":{"id":"u","team":{"states":{"nodes":[]}}}}}"#).is_err());
    }

    /// Runs the real `curl` against a one-shot server on the loopback interface (no outside network).
    #[test]
    fn a_post_reaches_the_server_with_header_and_body_intact() {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/graphql", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            // Read until the whole body (Content-Length) has arrived.
            loop {
                let n = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&request).into_owned();
                let Some((head, body)) = text.split_once("\r\n\r\n") else { continue };
                let length = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length: ").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                if body.len() >= length {
                    break;
                }
            }
            let reply = r#"{"data":{"ok":true}}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).unwrap();
            String::from_utf8(request).unwrap()
        });
        let body = json!({"query": "q \"x\"", "variables": {"t": "a\nb\\c"}}).to_string();
        assert_eq!(post(&url, "test-key", &body).unwrap(), r#"{"data":{"ok":true}}"#);
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /graphql"));
        assert!(request.contains("Authorization: test-key\r\n"));
        assert!(request.ends_with(&body));
    }

    #[test]
    fn curl_config_carries_the_key_and_escapes_the_body() {
        let body = json!({"query": "q \"x\"", "variables": {"t": "a\nb\\c"}}).to_string();
        let config = curl_config(ENDPOINT, "test-key", &body);
        assert!(config.contains("header = \"Authorization: test-key\"\n"));
        assert!(config.contains("url = \"https://api.linear.app/graphql\"\n"));
        // Every config line is one directive, so no value may contain a raw line break.
        assert!(config.lines().all(|line| line.starts_with("url") || line.starts_with("request") || line.starts_with("header") || line.starts_with("data")));
        let data_line = config.lines().find(|l| l.starts_with("data = ")).unwrap();
        assert!(data_line.contains(r#"\"query\":\"q \\\"x\\\"\""#));
    }
}
