use anyhow::Result;
use config::Config;

pub(crate) async fn run(config: &Config) -> Result<()> {
    let projects: Vec<(String, String)> = config
        .projects
        .iter()
        .map(|p| (p.name.clone(), p.repo.clone()))
        .collect();

    if projects.is_empty() {
        println!("no projects configured in hub.toml");
        return Ok(());
    }

    println!("fetching {} project(s)...", projects.len());
    workflows::fetch::run(&projects, &config.github_token).await?;
    println!("done");

    Ok(())
}
