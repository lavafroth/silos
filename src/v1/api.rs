use hora::core::ann_index::ANNIndex;
use std::{collections::HashMap, sync::Mutex};

use hora::index::hnsw_idx::HNSWIndex;
use serde::{Deserialize, Serialize};

use crate::embed;

use super::errors::GetError;

use actix_web::{Responder, post, web};

use anyhow::Result;
#[derive(Deserialize)]
pub struct SnippetRequest {
    desc: String,
    top_k: Option<usize>,
}

#[derive(Deserialize, Debug)]
pub struct SnippetOnDisk {
    pub body: String,
    pub desc: String,
}

pub struct AppStateWrapper {
    inner: Mutex<AppState>,
}

pub struct AppState {
    pub dict: HashMap<String, HNSWIndex<f32, String>>,
    pub embed: embed::Embed,
}

impl AppState {
    pub fn build(self) -> AppStateWrapper {
        AppStateWrapper {
            inner: Mutex::new(self),
        }
    }
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
    data: web::Data<AppStateWrapper>,
    snippet_request: web::Json<SnippetRequest>,
) -> Result<impl Responder, GetError> {
    let Some((prompt, lang)) = snippet_request.desc.rsplit_once(" in ") else {
        return Err(GetError::MissingSuffix);
    };

    let Ok(mut appstate) = data.inner.lock() else {
        return Err(GetError::Busy);
    };

    let Ok(target) = appstate.embed.embed(prompt) else {
        return Err(GetError::EmbedFailed);
    };

    // search for k nearest neighbors
    let closest: Vec<String> =
        appstate.dict[lang].search(&target, snippet_request.top_k.unwrap_or(1));
    Ok(web::Json(closest))
}

#[post("/api/v1/add")]
pub(crate) async fn add_snippet(
    data: web::Data<AppStateWrapper>,
    snippet: web::Json<Snippet>,
) -> Result<impl Responder, GetError> {
    let Ok(mut appstate) = data.inner.lock() else {
        return Err(GetError::Busy);
    };
    let Ok(embedding) = appstate.embed.embed(&snippet.desc) else {
        return Err(GetError::EmbedFailed);
    };
    let index = appstate
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
