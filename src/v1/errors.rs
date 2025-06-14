use actix_web::{
    HttpResponse, error,
    http::{StatusCode, header::ContentType},
};
use derive_more::derive::{Display, Error};

#[derive(Debug, Display, Error)]
pub enum GetError {
    #[display("the server is busy. come back later.")]
    Busy,
    #[display("end your request with \" in somelang\".")]
    MissingSuffix,
    #[display("failed to embed your prompt.")]
    EmbedFailed,
}

impl error::ResponseError for GetError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::html())
            .body(self.to_string())
    }

    fn status_code(&self) -> StatusCode {
        match *self {
            Self::EmbedFailed => StatusCode::INTERNAL_SERVER_ERROR,
            Self::MissingSuffix => StatusCode::BAD_REQUEST,
            Self::Busy => StatusCode::GATEWAY_TIMEOUT,
        }
    }
}
