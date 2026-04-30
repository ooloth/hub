use anyhow::Result;
use config::Config;
use domain::{CiFailure, Issue, LinearIssue, PullRequest};

pub(crate) async fn run(config: &Config) -> Result<()> {
    if config.linear_token.is_none() {
        println!("skipping: linear (LINEAR_TOKEN not set)");
    }

    let report = workflows::status::run(
        &config.github_token,
        &config.github_pr_repos(),
        &config.github_open_issue_repos(),
        &config.github_assigned_issue_repos(),
        &config.github_ci_repos(),
        config.linear_token.as_deref(),
    )
    .await?;

    print_section("github prs", &report.github_prs);
    print_section("github issues", &report.github_issues);

    if !report.github_ci_failures.is_empty() {
        print_section("github ci", &report.github_ci_failures);
    }

    if config.linear_token.is_some() {
        print_section("linear issues", &report.linear_issues);
    }

    Ok(())
}

trait PrintLine {
    fn print_line(&self);
}

impl PrintLine for PullRequest {
    fn print_line(&self) {
        println!(
            "  {}  {} (#{})  {}d  {}",
            self.repo, self.title, self.number, self.age_days, self.url
        );
    }
}

impl PrintLine for Issue {
    fn print_line(&self) {
        if self.labels.is_empty() {
            println!(
                "  {}  {} (#{})  {}d  {}",
                self.repo, self.title, self.number, self.age_days, self.url
            );
        } else {
            println!(
                "  {}  {} (#{})  {}d  [{}]  {}",
                self.repo,
                self.title,
                self.number,
                self.age_days,
                self.labels.join(", "),
                self.url
            );
        }
    }
}

impl PrintLine for CiFailure {
    fn print_line(&self) {
        let age = if self.age_hours < 24 {
            format!("{}h", self.age_hours)
        } else {
            format!("{}d", self.age_hours / 24)
        };
        println!(
            "  {}  {}  {}  {}  {}",
            self.repo, self.workflow_name, self.conclusion, age, self.url
        );
    }
}

impl PrintLine for LinearIssue {
    fn print_line(&self) {
        println!(
            "  {}  {}  [{}]  {}",
            self.identifier, self.title, self.state, self.url
        );
    }
}

fn print_section<T: PrintLine>(label: &str, items: &[T]) {
    println!("{label} ({})", items.len());
    for item in items {
        item.print_line();
    }
}
