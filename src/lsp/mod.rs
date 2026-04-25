use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::lsp_types::{
    CodeActionOptions, CodeActionParams, CodeActionProviderCapability, CodeActionResponse,
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, InitializeParams, InitializeResult,
    MessageType, Range, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer, jsonrpc::Result};
mod action;
mod code_action;

pub struct Backend {
    pub client: Client,
    pub body: Arc<Mutex<HashMap<Url, String>>>,
    pub appstate: crate::State,
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
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
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

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.body
            .lock()
            .await
            .insert(params.text_document.uri, params.text_document.text);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(body) = params.content_changes.into_iter().next() {
            self.body
                .lock()
                .await
                .insert(params.text_document.uri, body.text);
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        Ok(self.code_action_internal(params).await)
    }
}

fn url_extension(u: &Url) -> Option<String> {
    let file_path = u.to_file_path().ok()?;

    let extension = file_path.extension()?;
    let extension = extension.to_str()?;
    Some(extension.to_string())
}
