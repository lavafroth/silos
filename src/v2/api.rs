use hora::{core::ann_index::ANNIndex, index::hnsw_idx::HNSWIndex};
use std::collections::HashMap;
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
        "rust" => tree_sitter_rust::LANGUAGE,
        _ => return Err(Error::UnknownLang),
    }
    .into())
}

#[post("/api/v2/get")]
pub(crate) async fn get_snippet(
    data: web::Data<crate::state::StateWrapper>,
    snippet_request: web::Json<SnippetRequest>,
) -> Result<impl Responder, Error> {
    let Some((prompt, lang)) = snippet_request.desc.rsplit_once(" in ") else {
        return Err(Error::MissingSuffix);
    };

    let langfn = get_lang(lang)?;

    println!("{prompt:?}");

    let Ok(mut appstate) = data.inner.lock() else {
        return Err(Error::Busy);
    };

    let Ok(target) = appstate.embed.embed(prompt) else {
        return Err(Error::EmbedFailed);
    };

    let mut parser = Parser::new();
    parser.set_language(&langfn).unwrap();

    let source_code = snippet_request.body.as_str();
    let source_bytes = source_code.as_bytes();
    let tree = parser
        .parse(&source_code, None)
        .ok_or(Error::SnippetParsing)?;
    let root_node = tree.root_node();

    // search for k nearest neighbors
    let closest: Vec<String> = appstate.v2.dict[lang]
        .search(&target, snippet_request.top_k.unwrap_or(1))
        .iter()
        .map(|v| {
            mutation::apply(
                langfn.clone(),
                source_bytes,
                root_node,
                &appstate.v2.mutations_collection[*v],
            )
            .expect(&format!("failed to apply mutations from collection {v}"))
            // TODO: change the expect to a log
        })
        .collect();
    Ok(web::Json(closest))
}
