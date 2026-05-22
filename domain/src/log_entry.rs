#[derive(Clone, Debug)]
pub enum LogEntry {
    Gcp {
        project: String,
        env: String,
        title: String,
        message: String,
        line: String,
        url: String,
    },
    Loki {
        project: String,
        env: String,
        title: String,
        message: String,
        line: String,
        url: String,
    },
}
