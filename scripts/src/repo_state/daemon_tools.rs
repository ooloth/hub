//! See docs/invariants/every-program-the-daemon-runs-is-on-its-launchd-path.md
use std::collections::BTreeSet;
use std::path::PathBuf;

use super::domain_purity::rust_files_under;
use super::workspace::WorkspaceMember;
use crate::daemon_agent::DAEMON_TOOLS;

/// The calls that start a program, each taking the program as its first argument.
const STARTS: [&str; 2] = ["Command::new", "disclaimed_spawn::spawn"];

/// The functions that pass the program they are given on to a start, so the program the daemon
/// runs is named where they are called. A function that does this and is missing here fails the
/// check, and so does a name here that no longer does it.
const WRAPPERS: [&str; 3] = ["killed_on_drop", "start_disclaimed", "start_op"];

/// What a call to a start or a wrapper is given as its program.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ProgramArgument {
    /// A program named in the call itself.
    Literal(String),
    /// The first parameter of the function the call sits in, which makes that function a wrapper.
    EnclosingParameter { function: String },
    /// Anything else, which names no program the check can see.
    Unresolved { expression: String },
}

/// One call to a start or a wrapper, and the line it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SpawnSite {
    line: usize,
    program: ProgramArgument,
}

/// Every call to a start or a listed wrapper in `source`, outside its test module. Tests start
/// helpers such as `sh` and `sleep` that the daemon never runs.
fn spawn_sites(source: &str) -> Vec<SpawnSite> {
    let outside_tests = source
        .find("#[cfg(test)]")
        .map_or(source, |start| &source[..start]);
    let mut sites: Vec<(usize, SpawnSite)> = STARTS
        .iter()
        .chain(WRAPPERS.iter())
        .flat_map(|callee| calls_to(outside_tests, callee))
        .map(|(at, argument)| {
            let site = SpawnSite {
                line: outside_tests[..at].matches('\n').count() + 1,
                program: classify(&argument, enclosing_function(&outside_tests[..at])),
            };
            (at, site)
        })
        .collect();
    sites.sort_by_key(|(at, _)| *at);
    sites.into_iter().map(|(_, site)| site).collect()
}

/// Where each call to `callee` in `source` starts, with its first argument as written. A
/// definition of `callee` is not a call, and nor is a longer name that ends in it.
fn calls_to(source: &str, callee: &str) -> Vec<(usize, String)> {
    let opening = format!("{callee}(");
    source
        .match_indices(&opening)
        .filter(|(at, _)| {
            let before = source[..*at].trim_end_matches(|c: char| c == ' ');
            !before.ends_with(is_identifier_char) && !before.ends_with("fn")
        })
        .map(|(at, _)| (at, first_argument(&source[at + opening.len()..])))
        .collect()
}

/// The text of the first argument in `after_paren`, up to the comma or closing parenthesis that
/// ends it at its own depth.
fn first_argument(after_paren: &str) -> String {
    let mut depth = 0_usize;
    let mut in_string = false;
    let end = after_paren
        .char_indices()
        .find(|&(_, c)| {
            match c {
                '"' => in_string = !in_string,
                '(' | '[' | '{' if !in_string => depth += 1,
                ')' | ']' | '}' if !in_string && depth > 0 => depth -= 1,
                ',' | ')' if !in_string && depth == 0 => return true,
                _ => {}
            }
            false
        })
        .map_or(after_paren.len(), |(end, _)| end);
    after_paren[..end].trim().to_string()
}

/// The name and first parameter of the last function declared in `before`, the one a call at
/// the end of `before` sits in.
fn enclosing_function(before: &str) -> Option<(String, String)> {
    let at = before
        .match_indices("fn ")
        .filter(|(at, _)| !before[..*at].ends_with(is_identifier_char))
        .last()?
        .0;
    let rest = &before[at + "fn ".len()..];
    let name: String = rest
        .chars()
        .take_while(|&c| is_identifier_char(c))
        .collect();
    let parameters = &rest[rest.find('(')? + 1..];
    let first: String = parameters
        .trim_start()
        .chars()
        .take_while(|&c| is_identifier_char(c))
        .collect();
    Some((name, first))
}

fn classify(argument: &str, enclosing: Option<(String, String)>) -> ProgramArgument {
    if let Some(literal) = argument
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        return ProgramArgument::Literal(literal.to_string());
    }
    match enclosing {
        Some((function, parameter)) if !parameter.is_empty() && parameter == argument => {
            ProgramArgument::EnclosingParameter { function }
        }
        _ => ProgramArgument::Unresolved {
            expression: argument.to_string(),
        },
    }
}

const fn is_identifier_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// What the spawn calls across the daemon's crates add up to.
#[derive(Debug, Default)]
struct DaemonSpawns {
    programs: BTreeSet<String>,
    wrappers: BTreeSet<String>,
    unresolved: Vec<String>,
}

impl DaemonSpawns {
    fn add(&mut self, file: &std::path::Path, sites: Vec<SpawnSite>) {
        for site in sites {
            match site.program {
                ProgramArgument::Literal(program) => {
                    let _ = self.programs.insert(program);
                }
                ProgramArgument::EnclosingParameter { function } => {
                    let _ = self.wrappers.insert(function);
                }
                ProgramArgument::Unresolved { expression } => self.unresolved.push(format!(
                    "  {}:{}: {expression}",
                    file.display(),
                    site.line
                )),
            }
        }
    }

    /// Every way these spawns break the rule, with what a reader needs to find each one.
    fn unaccounted(&self, listed_wrappers: &[&str]) -> Vec<String> {
        let listed: BTreeSet<String> = listed_wrappers.iter().map(ToString::to_string).collect();
        let unlisted = self.wrappers.difference(&listed).map(|function| {
            format!("  {function} passes its own parameter to a start: add it to WRAPPERS")
        });
        let stale = listed.difference(&self.wrappers).map(|function| {
            format!("  {function} is in WRAPPERS but no longer passes a program on: remove it")
        });
        unlisted
            .chain(stale)
            .chain(self.unresolved.iter().cloned())
            .collect()
    }
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
    let mut spawns = DaemonSpawns::default();
    for member in daemon_crates(&members) {
        let files: Vec<PathBuf> = rust_files_under(&member.dir().join("src")).unwrap();
        for file in files {
            spawns.add(&file, spawn_sites(&std::fs::read_to_string(&file).unwrap()));
        }
    }

    assert!(
        !spawns.programs.is_empty(),
        "found no program the daemon runs, so the check checked nothing"
    );
    let unaccounted = spawns.unaccounted(&WRAPPERS);
    assert!(
        unaccounted.is_empty(),
        "the check cannot see what these start, so it cannot hold them to DAEMON_TOOLS:\n{}",
        unaccounted.join("\n")
    );
    assert_eq!(
        spawns.programs,
        DAEMON_TOOLS
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>(),
        "update DAEMON_TOOLS in scripts/src/daemon_agent/launch_agent.rs to match"
    );
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use rstest::rstest;

    use super::*;

    fn programs(source: &str) -> BTreeSet<String> {
        let mut spawns = DaemonSpawns::default();
        spawns.add(Path::new("x.rs"), spawn_sites(source));
        spawns.programs
    }

    #[rstest]
    #[case::command(
        r#"let out = tokio::process::Command::new("git").arg("fetch");"#,
        "git"
    )]
    #[case::killed_on_drop(
        r#"let output = killed_on_drop("gcloud").args(["logging"]);"#,
        "gcloud"
    )]
    #[case::start_disclaimed(r#"start_disclaimed("op", &["read", value]).await"#, "op")]
    #[case::disclaimed_spawn(r#"disclaimed_spawn::spawn("op", &["read"])?"#, "op")]
    fn a_program_started_by_a_literal_name_is_found(#[case] source: &str, #[case] program: &str) {
        assert_eq!(programs(source), BTreeSet::from([program.to_string()]));
    }

    #[test]
    fn programs_started_in_the_test_module_are_not_the_daemons() {
        let source = "fn run() { Command::new(\"op\"); }\n#[cfg(test)]\nmod tests { fn t() { Command::new(\"ps\"); } }";

        assert_eq!(programs(source), BTreeSet::from(["op".to_string()]));
    }

    #[test]
    fn a_function_that_passes_its_own_parameter_to_a_start_is_a_wrapper() {
        let source = "fn killed_on_drop(program: &str) -> Command {\n    let mut command = tokio::process::Command::new(program);\n    command\n}";

        assert_eq!(
            spawn_sites(source),
            [SpawnSite {
                line: 2,
                program: ProgramArgument::EnclosingParameter {
                    function: "killed_on_drop".to_string()
                },
            }]
        );
    }

    #[test]
    fn a_program_named_by_anything_but_a_literal_or_its_functions_parameter_is_unresolved() {
        let source =
            "fn fetch(project: &str) {\n    let tool = \"gcloud\";\n    Command::new(tool);\n}";

        assert_eq!(
            spawn_sites(source),
            [SpawnSite {
                line: 3,
                program: ProgramArgument::Unresolved {
                    expression: "tool".to_string()
                },
            }]
        );
    }

    #[test]
    fn a_wrapper_the_list_does_not_name_is_unaccounted_for() {
        let mut spawns = DaemonSpawns::default();
        spawns.add(
            Path::new("x.rs"),
            spawn_sites("fn run_quietly(program: &str) { Command::new(program); }"),
        );

        let unaccounted = spawns.unaccounted(&[]);

        assert_eq!(unaccounted.len(), 1, "{unaccounted:?}");
        assert!(unaccounted[0].contains("run_quietly"), "{unaccounted:?}");
    }

    #[test]
    fn a_listed_wrapper_that_no_longer_wraps_is_unaccounted_for() {
        let spawns = DaemonSpawns::default();

        let unaccounted = spawns.unaccounted(&["killed_on_drop"]);

        assert_eq!(unaccounted.len(), 1, "{unaccounted:?}");
        assert!(unaccounted[0].contains("killed_on_drop"), "{unaccounted:?}");
    }

    #[test]
    fn an_unresolved_call_is_unaccounted_for_with_its_file_and_line() {
        let mut spawns = DaemonSpawns::default();
        spawns.add(
            Path::new("clients/src/x.rs"),
            spawn_sites("fn fetch() {\n    Command::new(tool);\n}"),
        );

        assert_eq!(
            spawns.unaccounted(&[]),
            ["  clients/src/x.rs:2: tool".to_string()]
        );
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
