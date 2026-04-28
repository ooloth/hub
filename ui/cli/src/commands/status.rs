use anyhow::Result;
use config::Config;
use domain::{Issue, PullRequest};

pub async fn run(config: &Config) -> Result<()> {
    let report = workflows::status::run(
        &config.github_token,
        &config.github_pr_repos(),
        &config.github_open_issue_repos(),
        &config.github_assigned_issue_repos(),
    )
    .await?;

    print_section("github prs", &report.github_prs);
    print_section("github issues", &report.github_issues);

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

fn print_section<T: PrintLine>(label: &str, items: &[T]) {
    println!("{label} ({})", items.len());
    for item in items {
        item.print_line();
    }
}
