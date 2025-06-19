use crate::v1::api::AppStateWrapper;
use hora::core::ann_index::ANNIndex;
use tree_sitter::Parser;

use serde::{Deserialize, Serialize};

use crate::{embed, v2::mutation};

use super::{errors::GetError, mutation::Mutation};

use actix_web::{Responder, post, web};

use anyhow::Result;
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

fn get_lang(s: &str) -> tree_sitter::Language {
    match s {
        "go" => tree_sitter_go::LANGUAGE,
        "rust" => tree_sitter_rust::LANGUAGE,
        _ => unreachable!(),
    }
    .into()
}

#[post("/api/v2/get")]
pub(crate) async fn get_snippet(
    data: web::Data<AppStateWrapper>,
    snippet_request: web::Json<SnippetRequest>,
) -> Result<impl Responder, GetError> {
    let Some((prompt, lang)) = snippet_request.desc.rsplit_once(" in ") else {
        return Err(GetError::MissingSuffix);
    };

    let langfn = get_lang(lang);

    println!("{prompt:?}");

    let Ok(mut appstate) = data.inner.lock() else {
        return Err(GetError::Busy);
    };

    let Ok(target) = appstate.embed.embed(prompt) else {
        return Err(GetError::EmbedFailed);
    };

    let mut parser = Parser::new();
    parser.set_language(&langfn).unwrap();

    let source_code = std::fs::read_to_string("./example.go").unwrap();
    let source_bytes = source_code.as_bytes();
    let tree = parser.parse(&source_code, None).unwrap();
    let root_node = tree.root_node();

    // search for k nearest neighbors
    let closest: Vec<String> = appstate.v2_dict[lang]
        .search(&target, snippet_request.top_k.unwrap_or(1))
        .iter()
        .map(|v| {
            mutation::apply(
                langfn.clone(),
                snippet_request.body.as_bytes(),
                root_node,
                &appstate.v2_mutations_collection[*v],
            )
            .expect(&format!("failed to apply mutations from collection {v}"))
        })
        .collect();
    Ok(web::Json(closest))
}

// #[post("/api/v2/add")]
// pub(crate) async fn add_snippet(
//     data: web::Data<AppStateWrapper>,
//     snippet: web::Json<Snippet>,
// ) -> Result<impl Responder, GetError> {
//     let Ok(mut appstate) = data.inner.lock() else {
//         return Err(GetError::Busy);
//     };
//     let Ok(embedding) = appstate.embed.embed(&snippet.desc) else {
//         return Err(GetError::EmbedFailed);
//     };
//     let index = appstate
//         .dict
//         .entry(snippet.lang.clone())
//         .or_insert_with(|| {
//             let dimension = 384;
//             let params = hora::index::hnsw_params::HNSWParams::<f32>::default();

//             HNSWIndex::<f32, String>::new(dimension, &params)
//         });
//     index.add(&embedding, snippet.body.clone()).unwrap();
//     index.build(hora::core::metrics::Metric::Euclidean).unwrap();

//     Ok(format!(
//         "{} {} {}",
//         snippet.body, snippet.lang, snippet.desc
//     ))
// }
