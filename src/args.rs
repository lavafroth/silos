use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args, Debug)]
pub(crate) struct Lsp {
    /// Run on the Nth GPU device.
    #[arg(long)]
    pub(crate) gpu: Option<usize>,

    /// The model to use, check out available models: https://huggingface.co/models?library=sentence-transformers&sort=trending
    #[arg(long)]
    pub(crate) model_id: Option<String>,

    /// Revision or branch.
    #[arg(long)]
    pub(crate) revision: Option<String>,

    /// Path to the directory containing `generate` and `refactor` snippets.
    #[arg(long, default_value = "./snippets")]
    pub(crate) snippets: std::path::PathBuf,
}

#[derive(Args, Debug)]
pub struct DumpExpression {
    pub path: PathBuf,
}

#[derive(Args, Debug)]
pub struct ShowCaptures {
    pub path: PathBuf,
    pub expression: String,
}

#[derive(Subcommand, Debug)]
pub enum Ast {
    /// Dump the S expression for a given source file
    DumpExpression(DumpExpression),
    /// Show what parts of a source file gets captured by an S expression
    ShowCaptures(ShowCaptures),
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// quick actions to dump, modify and verify abstract syntax trees
    #[command(subcommand)]
    Ast(Ast),
    /// spawn a language server for use with a text editor
    Lsp(Lsp),
}

impl Lsp {
    pub(crate) fn resolve_model_and_revision(&self) -> (String, String) {
        let default_model = "sentence-transformers/all-MiniLM-L6-v2".to_string();
        let default_revision = "refs/pr/21".to_string();

        match (self.model_id.clone(), self.revision.clone()) {
            (Some(model_id), Some(revision)) => (model_id, revision),
            (Some(model_id), None) => (model_id, "main".to_owned()),
            (None, Some(revision)) => (default_model, revision),
            (None, None) => (default_model, default_revision),
        }
    }
}
