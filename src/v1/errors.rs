use actix_web::{
    HttpResponse, error,
    http::{StatusCode, header::ContentType},
};
use derive_more::derive::{Display, Error};
use serde_json::json;

#[derive(Debug, Display, Error)]
pub enum Error {
    #[display("the server is busy. come back later.")]
    Busy,
    #[display("failed to embed your prompt.")]
    EmbedFailed,
    #[display("snippets were requested for an unknown language")]
    UnknownLang,
}

impl error::ResponseError for Error {
    fn error_response(&self) -> HttpResponse {
        let message = json!({
            "message": self.to_string(),
        })
        .to_string();
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::json())
            .body(message)
    }

    fn status_code(&self) -> StatusCode {
        match *self {
            Self::EmbedFailed => StatusCode::INTERNAL_SERVER_ERROR,
            Self::UnknownLang => StatusCode::BAD_REQUEST,
            Self::Busy => StatusCode::GATEWAY_TIMEOUT,
        }
    }
}
