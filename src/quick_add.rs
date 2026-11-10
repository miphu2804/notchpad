//! Turns a typed line such as `gym 6pm !high #health 45m tomorrow` into a task: recognised
//! words set the planning fields and are taken out of the title.
//!
//! - time: `17:30`, `5pm`, `5:30pm`, `9h`, `9h30` (an `at` or `lúc` right before it is dropped)
//! - priority: `!high` `!med` `!low`, or `!!!` `!!` `!`
//! - project: `#word`
//! - estimate: `30m`, `45min`, `~1h`, `~1.5h`
//! - day: `today`, `tomorrow`, `tmr`

use crate::store::Task;

/// `today` and `tomorrow` are day keys; `default_day` is used when no day word is typed.
pub fn parse(input: &str, default_day: &str, today: &str, tomorrow: &str) -> Task {
    let mut task = Task { day: default_day.to_owned(), ..Task::default() };
    let mut words: Vec<&str> = Vec::new();
    for word in input.split_whitespace() {
        let lower = word.to_lowercase();
        if let Some(time) = clock(&lower) {
            if words.last().is_some_and(|w| matches!(w.to_lowercase().as_str(), "at" | "lúc")) {
                words.pop();
            }
            task.time = time;
        } else if let Some(priority) = priority(&lower) {
            task.priority = priority.to_owned();
        } else if let Some(project) = word.strip_prefix('#').filter(|p| !p.is_empty()) {
            task.project = project.to_owned();
        } else if let Some(minutes) = estimate(&lower) {
            task.minutes = minutes;
        } else if lower == "today" {
            task.day = today.to_owned();
        } else if lower == "tomorrow" || lower == "tmr" {
            task.day = tomorrow.to_owned();
        } else {
            words.push(word);
        }
    }
    task.text = words.join(" ");
    if task.text.is_empty() {
        // Nothing left to call it: keep what was typed rather than an empty title.
        task.text = input.trim().to_owned();
    }
    task
}

/// `task` as a line `parse` reads back to the same fields: title, then time, priority, project
/// and estimate, and `tomorrow` when the task is on `tomorrow`. Other days are not named, so the
/// line is parsed with the task's own day as the default.
pub fn line(task: &Task, tomorrow: &str) -> String {
    let mut parts = vec![task.text.clone()];
    if !task.time.is_empty() {
        parts.push(task.time.clone());
    }
    if !task.priority.is_empty() {
        parts.push(format!("!{}", task.priority));
    }
    if !task.project.is_empty() {
        parts.push(format!("#{}", task.project));
    }
    if task.minutes > 0 {
        parts.push(format!("{}m", task.minutes));
    }
    if !task.day.is_empty() && task.day == tomorrow {
        parts.push("tomorrow".to_owned());
    }
    parts.join(" ")
}

/// Words offered after `/`, in the order shown; `#` projects are added from the task list.
const VOCABULARY: [&str; 10] = ["today", "tomorrow", "!high", "!medium", "!low", "9am", "2pm", "6pm", "25m", "~1h"];
const PRIORITIES: [&str; 3] = ["!high", "!medium", "!low"];
/// More would not fit on the strip above the input.
const MAX_SUGGESTIONS: usize = 6;

/// Completions for the word being typed at the end of `text`: priorities after `!`, the given
/// projects after `#`, and the whole vocabulary (projects included) after `/`, each narrowed
/// by what follows the trigger. Empty once the word is finished with a space.
pub fn suggestions(text: &str, projects: &[String]) -> Vec<String> {
    let Some(word) = text.split(char::is_whitespace).next_back().filter(|w| !w.is_empty()) else { return Vec::new() };
    let tags = projects.iter().map(|p| format!("#{p}"));
    let (candidates, typed): (Vec<String>, String) = match word.chars().next() {
        Some('!') => (PRIORITIES.map(String::from).to_vec(), word.to_lowercase()),
        Some('#') => (tags.collect(), word.to_lowercase()),
        Some('/') => (VOCABULARY.map(String::from).into_iter().chain(tags).collect(), word[1..].to_lowercase()),
        _ => return Vec::new(),
    };
    candidates
        .into_iter()
        .filter(|c| {
            let c = c.to_lowercase();
            c.starts_with(&typed) || c.trim_start_matches(['!', '#', '~']).starts_with(&typed)
        })
        .take(MAX_SUGGESTIONS)
        .collect()
}

/// `text` with its last word replaced by `choice` and a space, ready for the next word.
pub fn complete(text: &str, choice: &str) -> String {
    let kept = text.rfind(char::is_whitespace).map_or("", |i| &text[..=i]);
    format!("{kept}{choice} ")
}

/// `HH:MM` (24-hour) for a clock time, or `None` if the word isn't one.
fn clock(word: &str) -> Option<String> {
    let (body, pm) = match word.strip_suffix("pm") {
        Some(body) => (body, Some(true)),
        None => match word.strip_suffix("am") {
            Some(body) => (body, Some(false)),
            None => (word, None),
        },
    };
    let (hour, minute) = match body.split_once([':', 'h']) {
        Some((h, m)) => (h, if m.is_empty() { "0" } else { m }),
        // A bare number is only a time with am/pm ("5pm"), never on its own ("5 apples").
        None if pm.is_some() => (body, "0"),
        None => return None,
    };
    // Bare `9h` reads as 9 o'clock, but `24:00`-style text needs a colon or h.
    if !(1..=2).contains(&hour.len()) || minute.len() > 2 {
        return None;
    }
    let mut hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    match pm {
        Some(_) if !(1..=12).contains(&hour) => return None,
        Some(true) if hour < 12 => hour += 12,
        Some(false) if hour == 12 => hour = 0,
        _ => {}
    }
    (hour < 24 && minute < 60).then(|| format!("{hour:02}:{minute:02}"))
}

fn priority(word: &str) -> Option<&'static str> {
    match word {
        "!high" | "!h" | "!!!" => Some("high"),
        "!medium" | "!med" | "!m" | "!!" => Some("medium"),
        "!low" | "!l" | "!" => Some("low"),
        _ => None,
    }
}

/// Minutes from `30m`, `45min` or `~1.5h`. Hours need the `~`, since `9h` is a clock time.
fn estimate(word: &str) -> Option<u32> {
    let minutes = if let Some(hours) = word.strip_prefix('~').and_then(|w| w.strip_suffix('h')) {
        hours.parse::<f64>().ok()? * 60.0
    } else {
        let number = word.trim_start_matches('~');
        let number = number.strip_suffix("min").or_else(|| number.strip_suffix('m'))?;
        number.parse::<f64>().ok()?
    };
    (minutes > 0.0 && minutes <= 24.0 * 60.0).then(|| minutes.round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick(input: &str) -> Task {
        parse(input, "D0", "D0", "D1")
    }

    #[test]
    fn line_reads_back_to_the_same_fields() {
        let task = quick("gym 6pm !high #health 45m tomorrow");
        assert_eq!(line(&task, "D1"), "gym 18:00 !high #health 45m tomorrow");
        let back = parse(&line(&task, "D1"), "D0", "D0", "D1");
        assert_eq!((back.text, back.time, back.priority, back.project, back.minutes, back.day), ("gym".into(), "18:00".into(), "high".into(), "health".into(), 45, "D1".into()));
        assert_eq!(line(&quick("mail"), "D1"), "mail");
    }

    #[test]
    fn reads_every_field_and_keeps_the_rest_as_title() {
        let t = quick("gym 6pm !high #Health 45m tomorrow");
        assert_eq!(
            (t.text.as_str(), t.time.as_str(), t.priority.as_str(), t.project.as_str(), t.minutes, t.day.as_str()),
            ("gym", "18:00", "high", "Health", 45, "D1")
        );
    }

    #[test]
    fn clock_formats() {
        for (word, want) in [("17:30", "17:30"), ("5:05pm", "17:05"), ("12am", "00:00"), ("12pm", "12:00"), ("9h", "09:00"), ("9h30", "09:30"), ("7AM", "07:00")] {
            assert_eq!(quick(&format!("x {word}")).time, want, "{word}");
        }
    }

    #[test]
    fn plain_numbers_and_odd_words_stay_in_the_title() {
        for input in ["buy 5 apples", "room 101", "13pm meeting", "25:00 thing", "call at noon", "v2:3 release"] {
            let t = quick(input);
            assert_eq!((t.text.as_str(), t.time.as_str()), (input, ""), "{input}");
        }
    }

    #[test]
    fn drops_at_before_a_time_only() {
        assert_eq!(quick("meet Lan at 5pm").text, "meet Lan");
        assert_eq!(quick("họp lúc 9h").text, "họp");
        assert_eq!(quick("look at it").text, "look at it");
    }

    #[test]
    fn estimates() {
        assert_eq!(quick("read 30m").minutes, 30);
        assert_eq!(quick("read 90min").minutes, 90);
        assert_eq!(quick("read ~1.5h").minutes, 90);
        assert_eq!(quick("read ~2h").minutes, 120);
    }

    #[test]
    fn default_day_and_today() {
        assert_eq!(parse("x", "", "D0", "D1").day, "");
        assert_eq!(parse("x today", "", "D0", "D1").day, "D0");
        assert_eq!(parse("x tmr", "D0", "D0", "D1").day, "D1");
    }

    #[test]
    fn only_tokens_keeps_the_input_as_title() {
        assert_eq!(quick("#work").text, "#work");
    }

    #[test]
    fn suggests_by_trigger_and_narrows_by_prefix() {
        let projects = vec!["Work".to_owned(), "Health".to_owned()];
        assert_eq!(suggestions("call mom !", &projects), ["!high", "!medium", "!low"]);
        assert_eq!(suggestions("call mom !m", &projects), ["!medium"]);
        assert_eq!(suggestions("gym #he", &projects), ["#Health"]);
        assert_eq!(suggestions("gym /to", &projects), ["today", "tomorrow"]);
        assert_eq!(suggestions("gym /w", &projects), ["#Work"]);
        assert_eq!(suggestions("gym /1", &projects), ["~1h"]);
    }

    #[test]
    fn no_suggestions_for_plain_or_finished_words() {
        let projects = vec!["Work".to_owned()];
        assert!(suggestions("", &projects).is_empty());
        assert!(suggestions("gym", &projects).is_empty());
        assert!(suggestions("gym !high ", &projects).is_empty());
        assert!(suggestions("gym !x", &projects).is_empty());
        assert!(suggestions("gym #", &[]).is_empty());
    }

    #[test]
    fn complete_replaces_the_last_word() {
        assert_eq!(complete("call mom !h", "!high"), "call mom !high ");
        assert_eq!(complete("/tom", "tomorrow"), "tomorrow ");
        assert_eq!(complete("gym  #he", "#Health"), "gym  #Health ");
    }

    #[test]
    fn every_suggestion_is_understood_by_the_parser() {
        for word in VOCABULARY.iter().chain(&PRIORITIES) {
            let task = parse(&format!("x {word}"), "D", "T", "M");
            assert_eq!(task.text, "x", "{word} was left in the title");
        }
    }
}
