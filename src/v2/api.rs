use hora::{core::ann_index::ANNIndex, index::hnsw_idx::HNSWIndex};
use std::collections::HashMap;
use tracing::{error, info};
use tree_sitter::Parser;

use super::{errors::Error, mutation};
use actix_web::{Responder, post, web};
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub struct State {
    pub dict: HashMap<String, HNSWIndex<f32, usize>>,
    pub mutations_collection: Vec<mutation::MutationCollection>,
}

#[derive(Deserialize)]
pub struct SnippetRequest {
    desc: String,
    body: String,
    lang: String,
    top_k: Option<usize>,
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

fn get_lang(s: &str) -> Result<tree_sitter::Language, Error> {
    Ok(match s {
        "go" => tree_sitter_go::LANGUAGE,
        "c" => tree_sitter_c::LANGUAGE,
        "rs" => tree_sitter_rust::LANGUAGE,
        _ => return Err(Error::UnknownLang),
    }
    .into())
}

#[post("/api/v2/get")]
pub(crate) async fn get_snippet(
    data: web::Data<crate::state::StateWrapper>,
    snippet_request: web::Json<SnippetRequest>,
) -> Result<impl Responder, Error> {
    let closest = search(
        &snippet_request.lang,
        &snippet_request.desc,
        snippet_request.body.as_str(),
        snippet_request.top_k.unwrap_or(1),
        &data,
    )?;
    Ok(web::Json(closest))
}

pub fn search(
    lang: &str,
    prompt: &str,
    body: &str,
    top_k: usize,
    data: &web::Data<crate::state::StateWrapper>,
) -> Result<Vec<String>, Error> {
    let langfn = get_lang(lang)?;

    info!(prompt = prompt, language = lang, "v2 request");

    let mut appstate = data.inner.lock().map_err(|_| Error::Busy)?;
    let target = appstate
        .embed
        .embed(prompt)
        .map_err(|_| Error::EmbedFailed)?;
    let mut parser = Parser::new();
    parser
        .set_language(&langfn)
        .map_err(|_| Error::UnknownLang)?;

    let source_code = body;
    let source_bytes = source_code.as_bytes();
    let tree = parser
        .parse(source_code, None)
        .ok_or(Error::SnippetParsing)?;
    let root_node = tree.root_node();

    // search for k nearest neighbors
    let collected = appstate.v2.dict[lang]
        .search(&target, top_k)
        .iter()
        .filter_map(|&index| {
            let applied = mutation::apply(
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
    Ok(collected)
}
