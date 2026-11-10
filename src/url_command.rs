//! `notchgo://` links, so Shortcuts, Raycast or a browser can drive NotchGo without running a
//! script: `notchgo://add?text=Pay%20rent%205pm` adds a task with the quick-add syntax and
//! `notchgo://plan` runs "Plan my day".

use crate::linear::percent_decode;

#[derive(Debug, PartialEq)]
pub enum UrlCommand {
    /// A quick-add line.
    Add(String),
    Plan,
}

/// The command a `notchgo://` URL asks for; `None` for another scheme, an unknown command or an
/// empty task.
pub fn parse(url: &str) -> Option<UrlCommand> {
    let rest = url.strip_prefix("notchgo://")?;
    let (command, query) = rest.split_once('?').unwrap_or((rest, ""));
    match command.trim_end_matches('/') {
        "plan" => Some(UrlCommand::Plan),
        "add" => {
            let text = query.split('&').find_map(|pair| pair.strip_prefix("text=")).and_then(percent_decode)?;
            let text = text.trim();
            (!text.is_empty()).then(|| UrlCommand::Add(text.to_owned()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_add_and_plan() {
        assert_eq!(parse("notchgo://add?text=Pay%20rent+5pm%20%21high"), Some(UrlCommand::Add("Pay rent 5pm !high".into())));
        assert_eq!(parse("notchgo://add/?source=mail&text=Review%20%23finance"), Some(UrlCommand::Add("Review #finance".into())));
        assert_eq!(parse("notchgo://plan"), Some(UrlCommand::Plan));
    }

    #[test]
    fn ignores_empty_unknown_and_foreign_links() {
        for url in ["notchgo://add", "notchgo://add?text=%20", "notchgo://add?text=%ZZ", "notchgo://delete", "https://add?text=x"] {
            assert_eq!(parse(url), None, "{url}");
        }
    }
}
