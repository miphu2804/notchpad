//! Registers the `notchpad-mcp` server with Claude Code (through its `claude` CLI) and with
//! Claude Desktop (by merging into its JSON config). Both are one click in Settings › Integrations
//! and both leave the user's other servers alone.

use std::path::{Path, PathBuf};
use std::{fs, process::Command};

use serde_json::{Map, Value, json};

const NAME: &str = "notchpad";

/// The server binary shipped beside the app's own executable.
pub fn server_path() -> PathBuf {
    std::env::current_exe().map(|exe| exe.with_file_name("notchpad-mcp")).unwrap_or_default()
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

// ---- Claude Code -------------------------------------------------------------------------

/// Whether Claude Code's user-scope `notchpad` server runs this app's binary (`~/.claude.json` is
/// where `--scope user` writes).
pub fn code_added() -> bool {
    points_here(&home().join(".claude.json"), &server_path())
}

/// Runs `claude mcp add`. The error is a line fit to show beside the button.
pub fn add_to_code() -> Result<(), String> {
    let path = std::env::var("PATH").unwrap_or_default();
    let claude = find_claude(&path, &home()).ok_or("Claude Code not found")?;
    // `add` refuses a name that is taken, e.g. by another build of the app; a missing server
    // makes `remove` fail, which is fine.
    let _ = Command::new(&claude).args(["mcp", "remove", "--scope", "user", NAME]).output();
    let output = Command::new(claude).args(add_args(&server_path())).output().map_err(|err| err.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(stderr.lines().next().unwrap_or("claude mcp add failed").to_owned())
}

/// The CLI arguments that register `server` for every project of the user.
fn add_args(server: &Path) -> Vec<String> {
    ["mcp", "add", "--scope", "user", NAME, "--"].map(str::to_owned).into_iter().chain([server.display().to_string()]).collect()
}

/// `claude` on `path`, else where its installers put it; a Finder-launched app gets a bare PATH.
fn find_claude(path: &str, home: &Path) -> Option<PathBuf> {
    let fallbacks = [home.join(".claude/local"), home.join(".local/bin"), "/opt/homebrew/bin".into(), "/usr/local/bin".into()];
    std::env::split_paths(path).chain(fallbacks).map(|dir| dir.join("claude")).find(|candidate| candidate.is_file())
}

// ---- Claude Desktop ----------------------------------------------------------------------

fn desktop_config(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Claude/claude_desktop_config.json")
}

pub fn desktop_added() -> bool {
    points_here(&desktop_config(&home()), &server_path())
}

/// Whether `config` registers the `notchpad` server at `server`. One left by another build of the
/// app counts as not added, so the row offers Add again and Add points it here.
fn points_here(config: &Path, server: &Path) -> bool {
    let text = fs::read_to_string(config).unwrap_or_default();
    serde_json::from_str::<Value>(&text).is_ok_and(|config| config["mcpServers"][NAME]["command"].as_str() == server.to_str())
}

/// Merges the server into Claude Desktop's config, keeping a `.bak` of what was there.
pub fn add_to_desktop() -> Result<(), String> {
    add_to_desktop_in(&home(), &server_path())
}

fn add_to_desktop_in(home: &Path, server: &Path) -> Result<(), String> {
    let path = desktop_config(home);
    let existing = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.to_string()),
    };
    let Some(merged) = merge_server(existing.as_deref().unwrap_or(""), &server.display().to_string())? else { return Ok(()) };
    let io = |err: std::io::Error| err.to_string();
    if existing.is_some() {
        fs::copy(&path, path.with_extension("json.bak")).map_err(io)?;
    }
    fs::create_dir_all(path.parent().unwrap_or(home)).map_err(io)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, merged).map_err(io)?;
    fs::rename(tmp, path).map_err(io)
}

/// `config` with `mcpServers.notchpad` pointing at `command`, every other key kept as it was.
/// `None` when it already does; an error when `config` is not a JSON object, so a file the user
/// hand-edited into something unreadable is never overwritten.
fn merge_server(config: &str, command: &str) -> Result<Option<String>, String> {
    let mut root = if config.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(config).map_err(|_| "Claude Desktop's config is not valid JSON".to_owned())?
    };
    let root_object = root.as_object_mut().ok_or("Claude Desktop's config is not a JSON object")?;
    let servers = root_object.entry("mcpServers").or_insert_with(|| json!({}));
    let servers = servers.as_object_mut().ok_or("\"mcpServers\" in Claude Desktop's config is not an object")?;
    let entry = json!({"command": command});
    if servers.get(NAME) == Some(&entry) {
        return Ok(None);
    }
    servers.insert(NAME.to_owned(), entry);
    Ok(Some(serde_json::to_string_pretty(&root).map_err(|err| err.to_string())? + "\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVER: &str = "/Applications/Notchday.app/Contents/MacOS/notchpad-mcp";

    fn parsed(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    /// A fresh directory standing in for `HOME`; tests never touch the real one.
    fn scratch_home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("notchpad-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_empty_file_gets_just_the_server() {
        let merged = merge_server("  \n", SERVER).unwrap().unwrap();
        assert_eq!(parsed(&merged), json!({"mcpServers": {"notchpad": {"command": SERVER}}}));
    }

    #[test]
    fn other_servers_and_keys_are_kept_in_order() {
        let before = r#"{"theme":"dark","mcpServers":{"zeta":{"command":"z"},"alpha":{"command":"a","args":["x"]}},"last":1}"#;
        let merged = merge_server(before, SERVER).unwrap().unwrap();
        let after = parsed(&merged);
        assert_eq!(after["theme"], "dark");
        assert_eq!(after["last"], 1);
        assert_eq!(after["mcpServers"]["alpha"], json!({"command": "a", "args": ["x"]}));
        assert_eq!(after["mcpServers"]["notchpad"]["command"], SERVER);
        let keys: Vec<_> = after.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["theme", "mcpServers", "last"]);
    }

    #[test]
    fn merging_twice_changes_nothing() {
        let once = merge_server("{}", SERVER).unwrap().unwrap();
        assert_eq!(merge_server(&once, SERVER).unwrap(), None);
    }

    #[test]
    fn a_stale_command_is_replaced() {
        let before = r#"{"mcpServers":{"notchpad":{"command":"/old/notchpad-mcp"}}}"#;
        let merged = merge_server(before, SERVER).unwrap().unwrap();
        assert_eq!(parsed(&merged)["mcpServers"]["notchpad"]["command"], SERVER);
    }

    #[test]
    fn unreadable_configs_are_refused() {
        assert!(merge_server("{ not json", SERVER).is_err());
        assert!(merge_server("[1,2]", SERVER).is_err());
        assert!(merge_server(r#"{"mcpServers":[]}"#, SERVER).is_err());
    }

    #[test]
    fn only_a_server_running_this_binary_counts_as_added() {
        let home = scratch_home("points");
        let config = home.join("config.json");
        assert!(!points_here(&config, Path::new(SERVER)));
        fs::write(&config, r#"{"mcpServers":{"notchpad":{"command":"/old/build/notchpad-mcp"}}}"#).unwrap();
        assert!(!points_here(&config, Path::new(SERVER)));
        fs::write(&config, format!(r#"{{"mcpServers":{{"notchpad":{{"type":"stdio","command":"{SERVER}"}}}}}}"#)).unwrap();
        assert!(points_here(&config, Path::new(SERVER)));
    }

    #[test]
    fn code_is_added_at_user_scope_after_the_double_dash() {
        assert_eq!(add_args(Path::new(SERVER)), ["mcp", "add", "--scope", "user", "notchpad", "--", SERVER]);
    }

    #[test]
    fn claude_is_found_on_path_then_in_the_installer_directories() {
        let home = scratch_home("find");
        let on_path = home.join("bin");
        let local = home.join(".claude/local");
        fs::create_dir_all(&on_path).unwrap();
        fs::create_dir_all(&local).unwrap();
        assert_eq!(find_claude(on_path.to_str().unwrap(), &home), None);
        fs::write(local.join("claude"), "").unwrap();
        assert_eq!(find_claude("", &home), Some(local.join("claude")));
        fs::write(on_path.join("claude"), "").unwrap();
        assert_eq!(find_claude(on_path.to_str().unwrap(), &home), Some(on_path.join("claude")));
    }

    #[test]
    fn desktop_config_is_created_then_backed_up_before_a_change() {
        let home = scratch_home("desktop");
        let path = desktop_config(&home);
        add_to_desktop_in(&home, Path::new(SERVER)).unwrap();
        assert_eq!(parsed(&fs::read_to_string(&path).unwrap())["mcpServers"]["notchpad"]["command"], SERVER);
        assert!(!path.with_extension("json.bak").exists());

        let hand_written = r#"{"mcpServers":{"other":{"command":"o"}}}"#;
        fs::write(&path, hand_written).unwrap();
        add_to_desktop_in(&home, Path::new(SERVER)).unwrap();
        assert_eq!(fs::read_to_string(path.with_extension("json.bak")).unwrap(), hand_written);
        assert!(fs::read_to_string(&path).unwrap().contains("other"));
    }

    #[test]
    fn an_invalid_desktop_config_is_left_untouched() {
        let home = scratch_home("invalid");
        let path = desktop_config(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ broken").unwrap();
        assert!(add_to_desktop_in(&home, Path::new(SERVER)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");
        assert!(!path.with_extension("json.bak").exists());
    }
}
