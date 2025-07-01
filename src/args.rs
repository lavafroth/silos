use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Args {
    /// The mode to run the server in. Defaults to LSP. The HTTP REST API can be run by specifying `http` or `http:port`. For example: `http:7047`
    pub(crate) mode: Option<String>,

    /// Run on the Nth GPU device.
    #[arg(long)]
    pub(crate) gpu: Option<usize>,

    /// The model to use, check out available models: https://huggingface.co/models?library=sentence-transformers&sort=trending
    #[arg(long)]
    pub(crate) model_id: Option<String>,

    /// Revision or branch.
    #[arg(long)]
    pub(crate) revision: Option<String>,
}

pub enum RunMode {
    Http(u16),
    Lsp,
}

impl Args {
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
    pub(crate) fn mode(&self) -> RunMode {
        let Some(http) = &self.mode else {
            return RunMode::Lsp;
        };
        if http == "http" {
            return RunMode::Http(8000);
        }
        let Some(port) = http.strip_prefix("http:") else {
            return RunMode::Lsp;
        };

        let Ok(port) = port.parse() else {
            return RunMode::Lsp;
        };

        RunMode::Http(port)
    }
}
