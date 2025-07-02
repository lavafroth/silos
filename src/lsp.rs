use crate::StateWrapper;
use crate::v2;
use actix_web::web::Data;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};
use tracing::error;

pub struct Backend {
    pub client: Client,
    pub body: Arc<Mutex<String>>,
    pub appstate: Data<StateWrapper>,
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

fn url_extension(u: &Url) -> Option<String> {
    let file_path = u.to_file_path().ok()?;

    let extension = file_path.extension()?;
    let extension = extension.to_str()?;
    Some(extension.to_string())
}
