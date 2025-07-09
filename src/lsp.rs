use crate::{StateWrapper, v1, v2};
use actix_web::web::Data;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

pub struct Backend {
    pub client: Client,
    pub body: Arc<Mutex<String>>,
    pub appstate: Data<StateWrapper>,
}

fn string_range_index(s: &str, r: Range) -> &str {
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
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions::default(),
                )),
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
        let Some(lang) = url_extension(&uri) else {
            self.client
                .log_message(MessageType::ERROR, "unable to determine filetype, file has no extension")
                .await;
            return Ok(None);
        };

        let body = self.body.lock().await.to_string();
        let mut range = params.range;
        let selected_text = string_range_index(&body, range);

        let Some(comment) = ParsedAction::new(selected_text) else {
            return Ok(None);
        };

        let action_response = match comment.action {
            Action::Generate => {
                range.start = range.end;
                v1::api::search(&lang, comment.description, 1, &self.appstate)
                    .map(|v| v.into_iter().map(|s| format!("{s}\n")).collect())
                    .map_err(|e| e.to_string())
            }
            Action::Refactor => {
                v2::api::search(&lang, comment.description, selected_text, 1, &self.appstate)
                    .map_err(|e| e.to_string())
            }
        };

        let closest_matches = match action_response {
            Ok(v) => v,
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, e.to_string())
                    .await;
                return Ok(None);
            }
        };

        let Some(new_text) = closest_matches.into_iter().next() else {
            return Ok(None);
        };
        let text_edit = TextEdit { range, new_text };
        let changes: HashMap<Url, _> = [(uri, vec![text_edit])].into_iter().collect();
        let edit = Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        });
        let actions = vec![CodeActionOrCommand::CodeAction(CodeAction {
            title: "ask silos".to_string(),
            edit,
            ..Default::default()
        })];
        Ok(Some(actions))
    }
}

pub struct ParsedAction<'a> {
    action: Action,
    description: &'a str,
}

pub enum Action {
    Generate,
    Refactor,
}

impl<'a> ParsedAction<'a> {
    fn new(comment: &'a str) -> Option<ParsedAction<'a>> {
        let upto_newline = match comment.rsplit_once("\n") {
            Some((upto_newline, _discard)) => upto_newline,
            None => comment,
        };
        let maybe_generate =
            upto_newline
                .split_once("generate: ")
                .map(|(_discard, description)| ParsedAction {
                    action: Action::Generate,
                    description,
                });
        let maybe_refactor =
            upto_newline
                .split_once("refactor: ")
                .map(|(_discard, description)| ParsedAction {
                    action: Action::Refactor,
                    description,
                });
        maybe_generate.or(maybe_refactor)
    }
}

fn url_extension(u: &Url) -> Option<String> {
    let file_path = u.to_file_path().ok()?;

    let extension = file_path.extension()?;
    let extension = extension.to_str()?;
    Some(extension.to_string())
}
