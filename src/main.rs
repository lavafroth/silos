use std::sync::Arc;

use actix_web::web::Data;
use actix_web::{App, HttpServer, web};
use anyhow::{Context, Error as E, Result, bail};
use clap::Parser;
use hora::core::ann_index::ANNIndex;
use hora::index::hnsw_idx::HNSWIndex;
use kdl::KdlDocument;
use state::{State, StateWrapper};
use std::collections::HashMap;
use tokio::sync::Mutex;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};
use tracing::{error, info};
use tree_sitter::Parser as TSParser;

mod embed;
mod state;
mod v1;
mod v2;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    mode: Option<String>,

    /// Run on the Nth GPU device.
    #[arg(long)]
    gpu: Option<usize>,

    /// The model to use, check out available models: https://huggingface.co/models?library=sentence-transformers&sort=trending
    #[arg(long)]
    model_id: Option<String>,

    /// Revision or branch.
    #[arg(long)]
    revision: Option<String>,

    /// The port for the API to listen on
    #[arg(long, default_value = "8000")]
    port: u16,
}

impl Args {
    fn resolve_model_and_revision(&self) -> (String, String) {
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

fn path_to_parent_base(p: &std::path::Path) -> Result<String> {
    let Some(parent) = p
        .parent()
        .and_then(|v| v.file_name())
        .and_then(|v| v.to_str())
        .map(|v| v.to_string())
    else {
        bail!("failed to parse snippets path, make sure the directory structure is valid");
    };
    Ok(parent)
}

#[actix_web::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    let port = args.port;
    let mode = args.mode.clone();
    let mut embed = embed::Embed::new(args)?;
    let mut dict = HashMap::default();

    let paths = glob::glob("./snippets/v1/*/*.kdl")?;
    for path in paths {
        let path = path?;
        let parent = path_to_parent_base(&path)?;

        let current_lang_index = dict.entry(parent).or_insert_with(|| {
            let dimension = 384;
            let params = hora::index::hnsw_params::HNSWParams::<f32>::default();

            HNSWIndex::<f32, String>::new(dimension, &params)
        });

        let doc_str = std::fs::read_to_string(&path)?;
        let doc: KdlDocument = doc_str
            .parse()
            .context(format!("failed to parse KDL: {}", path.display()))?;

        let Some(desc) = doc.get_arg("desc").and_then(|v| v.as_string()) else {
            continue;
        };
        let Some(body) = doc.get_arg("body").and_then(|v| v.as_string()) else {
            continue;
        };
        current_lang_index
            .add(&embed.embed(desc)?, body.to_string())
            .map_err(E::msg)?;
    }

    for index in dict.values_mut() {
        index
            .build(hora::core::metrics::Metric::Euclidean)
            .map_err(E::msg)?;
    }

    // v2 stuff
    let paths = glob::glob("./snippets/v2/*/*.kdl")?;
    let mut v2_dict = HashMap::new();
    let mut v2_mutations_collection = vec![];
    for (i, path) in paths.enumerate() {
        let path = path?;
        let parent = path_to_parent_base(&path)?;

        let mutations = v2::mutation::from_path(path)?;
        let current_lang_index = v2_dict.entry(parent).or_insert_with(|| {
            let dimension = 384;
            let params = hora::index::hnsw_params::HNSWParams::<f32>::default();

            HNSWIndex::<f32, usize>::new(dimension, &params)
        });

        current_lang_index
            .add(&embed.embed(&mutations.description)?, i)
            .map_err(E::msg)?;
        v2_mutations_collection.push(mutations);
    }

    for index in v2_dict.values_mut() {
        index
            .build(hora::core::metrics::Metric::Euclidean)
            .map_err(E::msg)?;
    }

    let appstate = State {
        embed,
        v1: v1::api::State { dict },
        v2: v2::api::State {
            dict: v2_dict,
            mutations_collection: v2_mutations_collection,
        },
    };

    let appstate_wrapped = web::Data::new(appstate.build());

    if mode.is_some_and(|v| v == "http") {
        HttpServer::new(move || {
            App::new()
                .app_data(appstate_wrapped.clone())
                .service(v1::api::get_snippet)
                .service(v1::api::add_snippet)
                .service(v2::api::get_snippet)
        })
        .bind(("127.0.0.1", port))?
        .run()
        .await
        .map_err(E::from)
    } else {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let (service, socket) = LspService::new(|client| Backend {
            client,
            body: Arc::new(Mutex::new(String::default())),
            appstate: appstate_wrapped.clone(),
        });
        Server::new(stdin, stdout, socket).serve(service).await;
        Ok(())
    }
}

struct Backend {
    client: Client,
    body: Arc<Mutex<String>>,
    appstate: Data<StateWrapper>,
}

pub fn string_range_index(s: &str, r: Range) -> &str {
    let mut newline_count = 0;
    let mut start = None;
    let mut end = None;
    for (i, c) in s.chars().enumerate() {
        if newline_count == r.start.line && start.is_none() {
            start.replace(i + r.start.character as usize);
        }

        if newline_count == r.end.line && end.is_none() {
            end.replace(i + r.end.character as usize);
        }
        if c == '\n' {
            newline_count += 1;
        }
    }
    &s[start.unwrap_or_default()..end.unwrap_or(s.len())]
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(
        &self,
        _: InitializeParams,
    ) -> tower_lsp::jsonrpc::Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                code_action_provider: Some(
                    tower_lsp::lsp_types::CodeActionProviderCapability::Options(
                        CodeActionOptions::default(),
                    ),
                ),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "server initialized!")
            .await;
    }

    async fn shutdown(&self) -> tower_lsp::jsonrpc::Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        // TODO: build an index for multiple documents in workdir
        *self.body.lock().await = params.text_document.text;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(body) = params.content_changes.into_iter().next() {
            *self.body.lock().await = body.text;
        }
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> tower_lsp::jsonrpc::Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let body = self.body.lock().await.to_string();

        let range = params.range;
        let new_text = string_range_index(&body, range);
        let Some((_before, after)) = new_text.split_once("silos: ") else {
            return Ok(None);
        };
        let Some((desc, _after)) = after.split_once("\n") else {
            return Ok(None);
        };
        

        let Some((prompt, lang)) = desc.rsplit_once(" in ") else {
            error!("{}", v2::errors::Error::MissingSuffix);
            return Ok(None);
        };

        let langfn = match v2::api::get_lang(lang) {
            Ok(o) => o,
            Err(e) => {
                error!("{e}");
                return Ok(None);
            }
        };

        info!(prompt = prompt, language = lang, "v2 request");

        let mut appstate = self
            .appstate
            .inner
            .lock()
            .map_err(|_| v2::errors::Error::Busy)
            .expect("booo");
        let target = appstate
            .embed
            .embed(prompt)
            .map_err(|_| v2::errors::Error::EmbedFailed)
            .expect("booo");
        let mut parser = TSParser::new();
        parser
            .set_language(&langfn)
            .map_err(|_| v2::errors::Error::UnknownLang)
            .expect("boo");

        let source_code = new_text;
        let source_bytes = source_code.as_bytes();
        let tree = parser
            .parse(source_code, None)
            .ok_or(v2::errors::Error::SnippetParsing)
            .expect("boo");
        let root_node = tree.root_node();

        // search for k nearest neighbors
        let closest: Vec<String> = appstate.v2.dict[lang]
            .search(&target, 1)
            .iter()
            .filter_map(|&index| {
                let applied = v2::mutation::apply(
                    langfn.clone(),
                    source_bytes,
                    root_node,
                    &appstate.v2.mutations_collection[index],
                );
                match applied {
                    Ok(v) => Some(v),
                    Err(e) => {
                        error!(
                            collection_index = index,
                            "failed to apply mutations from collection {}", e
                        );
                        None
                    }
                }
                // TODO: change the expect to a log
            })
            .collect();

        let closest = closest[0].clone();

        let text_edit = TextEdit {
            range,
            new_text: closest,
        };
        let changes: HashMap<Url, _> = [(uri, vec![text_edit])].into_iter().collect();
        let edit = WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        };
        let actions = vec![CodeActionOrCommand::CodeAction(CodeAction {
            title: "ask silos".to_string(),
            kind: None,
            diagnostics: None,
            edit: Some(edit),
            command: None,
            is_preferred: None,
            disabled: None,
            data: None,
        })];
        Ok(Some(actions))
    }
}
