use clap::{Parser, ValueEnum};

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LspClient {
    #[value(name = "vscode")]
    VSCode,
}

#[derive(Parser, Debug)]
#[command(name = "ADL Language Server")]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[clap(short, long)]
    pub client: Option<LspClient>,

    #[clap(long, value_parser, num_args = 1.., value_delimiter = ',')]
    pub search_dirs: Vec<String>,
}
