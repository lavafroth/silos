use tower_lsp::lsp_types::{
    CodeAction, CodeActionOrCommand, CodeActionParams, CodeActionResponse, MessageType, TextEdit,
    WorkspaceEdit,
};

use super::Backend;
use super::action::Action;
use super::string_range_index;

impl Backend {
    pub(crate) async fn code_action_internal(
        &self,
        params: CodeActionParams,
    ) -> Option<CodeActionResponse> {
        let uri = params.text_document.uri;
        let mut range = params.range;
        let Some(lang) = super::url_extension(&uri) else {
            self.client
                .log_message(
                    MessageType::ERROR,
                    "unable to determine filetype, file has no extension",
                )
                .await;
            return None;
        };

        let body_locked = self.body.lock().await;
        let body = body_locked.get(&uri)?;
        let selected_text = string_range_index(body, range);

        let response = match Action::new(selected_text)? {
            Action::Generate(description) => {
                range.start = range.end;
                self.appstate
                    .generate(&lang, description, 1)
                    .map(|v| v.into_iter().map(|s| format!("{s}\n")).collect())
            }
            Action::Refactor(description) => {
                self.appstate.refactor(&lang, description, selected_text, 1)
            }
        };

        let closest_matches = match response {
            Ok(v) => v,
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, e.to_string())
                    .await;
                return None;
            }
        };

        let new_text = closest_matches.into_iter().next()?;
        let text_edit = TextEdit { range, new_text };
        let changes = [(uri, vec![text_edit])].into_iter().collect();
        let edit = Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        });
        Some(vec![CodeActionOrCommand::CodeAction(CodeAction {
            title: "ask silos".to_string(),
            edit,
            ..Default::default()
        })])
    }
}
