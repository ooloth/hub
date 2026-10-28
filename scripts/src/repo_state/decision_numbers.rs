use std::collections::BTreeMap;

use crate::repo_root::repo_root;

/// Each number two or more decision records share, with the records that share it.
///
/// Two branches can each take the next free number without seeing the other's, as 029 was taken
/// twice on 2026-10-26, and "Decision 029" then names two records. `names` are file names such as
/// `029-hub-opens-windows.md`; the number is everything before the first `-`.
fn shared_numbers(names: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut by_number: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in names {
        let number = name
            .split_once('-')
            .map_or(name.as_str(), |(number, _)| number);
        by_number
            .entry(number.to_string())
            .or_default()
            .push(name.clone());
    }
    by_number
        .into_iter()
        .filter(|(_, records)| records.len() > 1)
        .map(|(number, mut records)| {
            records.sort();
            (number, records)
        })
        .collect()
}

#[test]
fn no_two_decision_records_share_a_number() {
    let dir = repo_root().join("docs/decisions");
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".md") && name != "README.md")
        .collect();
    assert!(
        names.len() >= 25,
        "found {} decision records in {}, so the scan cannot be trusted",
        names.len(),
        dir.display()
    );

    let shared: Vec<String> = shared_numbers(&names)
        .into_iter()
        .map(|(number, records)| format!("  {number}: {}", records.join(", ")))
        .collect();

    assert!(
        shared.is_empty(),
        "these decision records share a number, so a reference to it names more than one:\n{}\n\n\
         Give the record that merged last the next free number.",
        shared.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn records_that_share_a_number_are_named_together() {
        let shared = shared_numbers(&names(&[
            "028-macos-focus.md",
            "029-hub-opens-windows.md",
            "029-whether-a-pull-request-is-my-turn.md",
        ]));

        assert_eq!(
            shared,
            BTreeMap::from([(
                "029".to_string(),
                names(&[
                    "029-hub-opens-windows.md",
                    "029-whether-a-pull-request-is-my-turn.md"
                ])
            )])
        );
    }

    #[test]
    fn records_with_their_own_numbers_share_none() {
        let shared = shared_numbers(&names(&["029-hub-opens-windows.md", "030-whether.md"]));

        assert!(shared.is_empty(), "{shared:?}");
    }
}
