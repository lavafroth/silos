use hora::core::ann_index::ANNIndex;
use std::collections::HashMap;

use super::errors::Error;
use actix_web::{Responder, post, web};
use anyhow::Result;
use hora::index::hnsw_idx::HNSWIndex;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct SnippetRequest {
    desc: String,
    top_k: Option<usize>,
}

pub struct State {
    pub dict: HashMap<String, HNSWIndex<f32, String>>,
}

#[derive(Serialize)]
pub struct SnippetResponse {
    id: usize,
    snippet: Snippet,
}

#[derive(Serialize, Deserialize)]
pub struct Snippet {
    lang: String,
    desc: String,
    body: String,
}

#[post("/api/v1/get")]
pub(crate) async fn get_snippet(
    data: web::Data<crate::state::StateWrapper>,
    snippet_request: web::Json<SnippetRequest>,
) -> Result<impl Responder, Error> {
    let Some((prompt, lang)) = snippet_request.desc.rsplit_once(" in ") else {
        return Err(Error::MissingSuffix);
    };

    let Ok(mut appstate) = data.inner.lock() else {
        return Err(Error::Busy);
    };

    let Ok(target) = appstate.embed.embed(prompt) else {
        return Err(Error::EmbedFailed);
    };

    let Some(snippets_for_lang) = appstate.v1.dict.get(lang) else {
        return Err(Error::UnknownLang);
    };
    // search for k nearest neighbors
    let closest = snippets_for_lang.search(&target, snippet_request.top_k.unwrap_or(1));
    Ok(web::Json(closest))
}

#[post("/api/v1/add")]
pub(crate) async fn add_snippet(
    data: web::Data<crate::state::StateWrapper>,
    snippet: web::Json<Snippet>,
) -> Result<impl Responder, Error> {
    let Ok(mut appstate) = data.inner.lock() else {
        return Err(Error::Busy);
    };
    let Ok(embedding) = appstate.embed.embed(&snippet.desc) else {
        return Err(Error::EmbedFailed);
    };
    let index = appstate
        .v1
        .dict
        .entry(snippet.lang.clone())
        .or_insert_with(|| {
            let dimension = 384;
            let params = hora::index::hnsw_params::HNSWParams::<f32>::default();

            HNSWIndex::<f32, String>::new(dimension, &params)
        });
    index.add(&embedding, snippet.body.clone()).unwrap();
    index.build(hora::core::metrics::Metric::Euclidean).unwrap();

    Ok(format!(
        "{} {} {}",
        snippet.body, snippet.lang, snippet.desc
    ))
}
