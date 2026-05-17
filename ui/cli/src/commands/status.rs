use anyhow::Result;
use config::Config;
use domain::Urgency;
use workflows::status::StatusItem;

pub(crate) async fn run(config: &Config) -> Result<()> {
    if config.linear_token.is_none() {
        println!("skipping: linear (LINEAR_TOKEN not set)");
    }

    let report = workflows::status::run(workflows::status::StatusParams {
        github_token: config.github_token.clone(),
        github_username: config.github_username.clone(),
        pr_repos: config.github_pr_repos(),
        issue_repos: config.github_issue_repos(),
        ci_repos: config.github_ci_repos(),
        linear_token: config.linear_token.clone(),
        private_workflow_names: config.private_monitor_workflow_names(),
        loki_envs: config.loki_envs(),
    })
    .await?;

    println!("status ({})", report.items.len());
    for item in &report.items {
        render_line(item);
    }

    if !report.errors.is_empty() {
        println!("\nunreachable ({})", report.errors.len());
        for source in &report.errors {
            println!("  {source}");
        }
    }

    Ok(())
}

fn tier_label(urgency: Urgency) -> &'static str {
    match urgency {
        Urgency::Critical => "[crit]",
        Urgency::High => "[high]",
        Urgency::Medium => "[med] ",
        Urgency::Low => "[low] ",
    }
}

fn fmt_age(age: chrono::Duration) -> String {
    let hours = age.num_hours();
    if hours < 24 {
        format!("{hours}h")
    } else {
        format!("{}d", age.num_days())
    }
}

fn render_line(item: &StatusItem) {
    match item {
        StatusItem::Pr(pr) => println!(
            "  {}  {}  {} (#{})  {}  {}",
            tier_label(pr.urgency),
            pr.repo,
            pr.title,
            pr.number,
            fmt_age(pr.age),
            pr.url,
        ),
        StatusItem::Issue(i) => {
            if i.labels.is_empty() {
                println!(
                    "  {}  {}  {} (#{})  {}  {}",
                    tier_label(i.urgency),
                    i.repo,
                    i.title,
                    i.number,
                    fmt_age(i.age),
                    i.url,
                );
            } else {
                println!(
                    "  {}  {}  {} (#{})  {}  [{}]  {}",
                    tier_label(i.urgency),
                    i.repo,
                    i.title,
                    i.number,
                    fmt_age(i.age),
                    i.labels.join(", "),
                    i.url,
                );
            }
        }
        StatusItem::Ci(c) => println!(
            "  {}  {}  {}  {}  {}",
            tier_label(c.urgency),
            c.repo,
            c.workflow_name,
            fmt_age(c.age),
            c.url,
        ),
        StatusItem::Linear(l) => println!(
            "  {}  {}  {}  [{}]  {}",
            tier_label(l.urgency),
            l.identifier,
            l.title,
            l.state,
            l.url,
        ),
        StatusItem::Loki(l) => println!(
            "  {}  {} · {} · {} · {}",
            tier_label(l.urgency),
            l.project,
            l.env,
            l.title,
            l.message,
        ),
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => println!(
            "  {}  {} · Import blocked · {}",
            tier_label(b.urgency),
            b.source,
            b.title,
        ),
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => println!(
            "  {}  {} · Not found · {} · aired {}",
            tier_label(m.urgency),
            m.source,
            m.title,
            m.air_date,
        ),
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => {
            println!("  {}  {} · {}", tier_label(h.urgency), h.source, h.message,)
        }
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { source, count } => println!(
            "  {}  {source} · {count} episodes in backlog",
            tier_label(domain::Urgency::Low),
        ),
    }
}
