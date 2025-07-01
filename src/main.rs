use actix_web::web::Data;
use actix_web::{App, HttpServer, web};
use anyhow::{Context, Error as E, Result, bail};
use clap::Parser;
use hora::core::ann_index::ANNIndex;
use hora::index::hnsw_idx::HNSWIndex;
use kdl::KdlDocument;
use state::{State, StateWrapper};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};
use tracing::error;

mod args;
mod embed;
mod state;
mod v1;
mod v2;

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
    let args = args::Args::parse();
    let mode = args.mode();
    let (model_id, revision) = args.resolve_model_and_revision();
    let mut embed = embed::Embed::new(args.gpu, &model_id, &revision)?;
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

    if let args::RunMode::Http(port) = mode {
        return HttpServer::new(move || {
            App::new()
                .app_data(appstate_wrapped.clone())
                .service(v1::api::get_snippet)
                .service(v1::api::add_snippet)
                .service(v2::api::get_snippet)
        })
        .bind(("127.0.0.1", port))?
        .run()
        .await
        .map_err(E::from);
    };

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
        let extension = url_extension(&uri);
        let body = self.body.lock().await.to_string();

        let range = params.range;
        let new_text = string_range_index(&body, range);
        let Some((_before, after)) = new_text.split_once("silos: ") else {
            return Ok(None);
        };
        let Some((desc, _after)) = after.split_once("\n") else {
            return Ok(None);
        };

        let (prompt, lang) = if let Some(ext) = extension {
            (desc, ext)
        } else if let Some((prompt, lang)) = desc.rsplit_once(" in ") {
            (prompt, lang.to_string())
        } else {
            error!("{}", v2::errors::Error::MissingSuffix);
            return Ok(None);
        };

        let closest_matches =
            match v2::api::closest_mutation(&lang, prompt, &body, 1, &self.appstate) {
                Ok(v) => v,
                Err(e) => {
                    error!("{}", e);
                    return Ok(None);
                }
            };

        let Some(closest) = closest_matches.into_iter().next() else {
            return Ok(None);
        };
        let text_edit = TextEdit {
            range,
            new_text: closest,
        };
        let changes: HashMap<Url, _> = [(uri, vec![text_edit])].into_iter().collect();
        let edit = Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        });
        let actions = vec![CodeActionOrCommand::CodeAction(CodeAction {
            title: "ask silos".to_string(),
            edit,
            ..Default::default()
        })];
        Ok(Some(actions))
    }
}

pub fn url_extension(u: &Url) -> Option<String> {
    let file_path = u.to_file_path().ok()?;

    let extension = file_path.extension()?;
    let extension = extension.to_str()?;
    Some(extension.to_string())
}
