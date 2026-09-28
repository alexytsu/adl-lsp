use clap::{Parser, ValueEnum};

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LspClient {
    #[value(name = "vscode")]
    VSCode,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl From<LogLevel> for tracing::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => tracing::Level::ERROR,
            LogLevel::Warn => tracing::Level::WARN,
            LogLevel::Info => tracing::Level::INFO,
            LogLevel::Debug => tracing::Level::DEBUG,
            LogLevel::Trace => tracing::Level::TRACE,
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "ADL Language Server")]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[clap(short, long)]
    pub client: Option<LspClient>,

    #[clap(long, value_parser, num_args = 1.., value_delimiter = ',')]
    pub search_dirs: Vec<String>,

    /// Verbosity of the logs written to stderr
    #[clap(long, value_enum, env = "ADL_LSP_LOG_LEVEL", default_value_t)]
    pub log_level: LogLevel,
}
