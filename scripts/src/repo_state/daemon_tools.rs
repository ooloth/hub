//! See docs/invariants/every-program-the-daemon-runs-is-on-its-launchd-path.md
use std::collections::BTreeSet;

use super::domain_purity::rust_files_under;
use super::workspace::WorkspaceMember;
use crate::daemon_agent::DAEMON_TOOLS;

/// The ways the daemon's code starts a program by name.
const SPAWN_CALLS: [&str; 2] = ["Command::new(\"", "killed_on_drop(\""];

/// Every program `source` starts by a literal name, outside its test module. Tests start
/// helpers such as `sh` and `ps` that the daemon never runs.
fn spawned_programs(source: &str) -> BTreeSet<String> {
    let outside_tests = source
        .find("#[cfg(test)]")
        .map_or(source, |start| &source[..start]);
    SPAWN_CALLS
        .iter()
        .flat_map(|call| {
            outside_tests
                .match_indices(call)
                .filter_map(move |(at, _)| {
                    let name = &outside_tests[at + call.len()..];
                    name.find('"').map(|end| name[..end].to_string())
                })
        })
        .collect()
}

/// The workspace crates `hub-daemon` is built from, itself included, by following path
/// dependencies.
fn daemon_crates(members: &[WorkspaceMember]) -> Vec<WorkspaceMember> {
    let mut reached: Vec<WorkspaceMember> = Vec::new();
    let mut pending = vec!["hub-daemon".to_string()];
    while let Some(name) = pending.pop() {
        if reached.iter().any(|member| member.name == name) {
            continue;
        }
        if let Some(member) = members.iter().find(|member| member.name == name) {
            pending.extend(member.local_dependencies.iter().cloned());
            reached.push(member.clone());
        }
    }
    reached
}

/// The daemon runs under launchd with a PATH holding only the directories of `DAEMON_TOOLS`, so
/// a program missing from that list is not found when the daemon runs it.
#[test]
fn daemon_tools_are_every_program_the_daemon_runs() {
    let members = WorkspaceMember::of_this_repo().unwrap();
    let mut spawned = BTreeSet::new();
    for member in daemon_crates(&members) {
        for file in rust_files_under(&member.dir().join("src")).unwrap() {
            spawned.extend(spawned_programs(&std::fs::read_to_string(&file).unwrap()));
        }
    }

    assert!(
        !spawned.is_empty(),
        "found no program the daemon runs, so the check checked nothing"
    );
    assert_eq!(
        spawned,
        DAEMON_TOOLS
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>(),
        "update DAEMON_TOOLS in scripts/src/daemon_agent/launch_agent.rs to match"
    );
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn a_command_started_by_a_literal_name_is_found() {
        let source = r#"let out = tokio::process::Command::new("git").arg("fetch");"#;

        assert_eq!(
            spawned_programs(source),
            BTreeSet::from(["git".to_string()])
        );
    }

    #[test]
    fn a_program_started_through_killed_on_drop_is_found() {
        let source = r#"let output = killed_on_drop("gcloud").args(["logging"]);"#;

        assert_eq!(
            spawned_programs(source),
            BTreeSet::from(["gcloud".to_string()])
        );
    }

    #[test]
    fn programs_started_in_the_test_module_are_not_the_daemons() {
        let source = "fn run() { Command::new(\"op\"); }\n#[cfg(test)]\nmod tests { fn t() { Command::new(\"ps\"); } }";

        assert_eq!(spawned_programs(source), BTreeSet::from(["op".to_string()]));
    }

    #[test]
    fn a_program_named_by_a_variable_is_not_a_literal_name() {
        assert!(spawned_programs("let mut command = Command::new(program);").is_empty());
    }

    fn member(name: &str, dependencies: &[&str]) -> WorkspaceMember {
        WorkspaceMember {
            name: name.to_string(),
            manifest: PathBuf::from(format!("/repo/{name}/Cargo.toml")),
            local_dependencies: dependencies.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn the_daemons_crates_are_it_and_everything_it_depends_on_by_path() {
        let members = [
            member("hub-daemon", &["workflows"]),
            member("workflows", &["clients", "domain"]),
            member("clients", &["domain"]),
            member("domain", &[]),
            member("hub-tui", &["workflows"]),
        ];

        let mut names: Vec<String> = daemon_crates(&members)
            .into_iter()
            .map(|member| member.name)
            .collect();
        names.sort();

        assert_eq!(names, ["clients", "domain", "hub-daemon", "workflows"]);
    }
}
