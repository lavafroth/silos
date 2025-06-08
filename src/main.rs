use std::collections::HashMap;

use actix_web::{App, HttpResponse, HttpServer, Responder, get, post, web};
use anyhow::{Error as E, Result};
use clap::Parser;
use hora::core::ann_index::ANNIndex;
use hora::index::hnsw_idx::HNSWIndex;
use serde::{Deserialize, Serialize};
mod embed;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Run on CPU rather than on GPU.
    #[arg(long)]
    cpu: bool,

    /// The model to use, check out available models: https://huggingface.co/models?library=sentence-transformers&sort=trending
    #[arg(long)]
    model_id: Option<String>,

    /// Revision or branch
    #[arg(long)]
    revision: Option<String>,
}

impl Args {
    fn resolve_model_and_revision(&self) -> (String, String) {
        let default_model = "sentence-transformers/all-MiniLM-L6-v2".to_string();
        let default_revision = "refs/pr/21".to_string();

        match (self.model_id.clone(), self.revision.clone()) {
            (Some(model_id), Some(revision)) => (model_id, revision),
            (Some(model_id), None) => (model_id, "main".to_owned()),
            (None, Some(revision)) => (default_model, revision),
            (None, None) => (default_model, default_revision),
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

#[derive(Deserialize)]
pub struct SnippetRequest {
    desc: String,
}

#[derive(Deserialize, Debug)]
pub struct SnippetOnDisk {
    body: String,
    desc: String,
}

pub struct LanguageMap {
    inner: HashMap<String, HNSWIndex<f32, String>>,
}

#[post("/api/v1/get")]
async fn v1_get_snippet(snippet_request: web::Json<SnippetRequest>) -> impl Responder {
    format!("bruh, you asked for {}", snippet_request.desc)
}

#[post("/api/v1/add")]
async fn v1_add_snippet(snippet: web::Json<Snippet>) -> impl Responder {
    format!("{} {} {}", snippet.body, snippet.lang, snippet.desc)
}

#[actix_web::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let mut embed = embed::Embed::new(args)?;

    let mut lang_map = LanguageMap {
        inner: HashMap::default(),
    };

    let paths = glob::glob("./snippets/v1/**/*.json")?;
    for path in paths {
        let path = path?;
        let parent = path
            .components()
            .rev()
            .nth(1)
            .unwrap()
            .as_os_str()
            .to_string_lossy()
            .to_string();

        let current_lang_index = lang_map.inner.entry(parent).or_insert_with(|| {
            let dimension = 384;
            let params = hora::index::hnsw_params::HNSWParams::<f32>::default();
            let index = HNSWIndex::<f32, String>::new(dimension, &params);
            index
        });

        let snippet: SnippetOnDisk = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        current_lang_index
            .add(&embed.embed(&snippet.desc)?, snippet.body)
            .map_err(E::msg)?;
    }

    for index in lang_map.inner.values_mut() {
        index
            .build(hora::core::metrics::Metric::Euclidean)
            .map_err(E::msg)?;
    }

    let prompt = "channel worker in go";

    let Some((prompt, lang)) = prompt.rsplit_once(" in ") else {
        return Ok(());
    };

    let target = embed.embed(&prompt)?;
    let k = 1;

    // search for k nearest neighbors
    let nn: Vec<String> = lang_map.inner[lang].search(&target, k);
    println!("target has neighbors");
    for n in nn {
        println!("{n}");
    }

    HttpServer::new(|| App::new().service(v1_get_snippet).service(v1_add_snippet))
        .bind(("127.0.0.1", 8000))?
        .run()
        .await
        .map_err(E::from)
}
