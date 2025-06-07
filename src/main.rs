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

    /// When set, compute embeddings for this prompt.
    #[arg(long)]
    prompt: String,
}

impl Args {
    fn resolve_model_and_revision(&self) -> (String, String) {
        let default_model = "distilbert-base-uncased".to_string();
        let default_revision = "main".to_string();

        match (self.model_id.clone(), self.revision.clone()) {
            (Some(model_id), Some(revision)) => (model_id, revision),
            (Some(model_id), None) => (model_id, default_revision),
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

#[get("/api/v1/get")]
async fn hello() -> impl Responder {
    HttpResponse::Ok().body("bob")
}

#[post("/api/v1/add")]
async fn add_snippet(snippet: web::Json<Snippet>) -> impl Responder {
    HttpResponse::Ok().body(format!(
        "{} {} {}",
        snippet.body, snippet.lang, snippet.desc
    ))
}

#[actix_web::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let prompt = args.prompt.clone();
    let embed = embed::Embed::new(args)?;

    let dimension = 768;
    let params = hora::index::hnsw_params::HNSWParams::<f32>::default();
    let mut index = HNSWIndex::<f32, usize>::new(dimension, &params);

    let strings = ["lol"];

    for (i, s) in strings.iter().enumerate() {
        index.add(&embed.embed(s)?, i).map_err(E::msg)?;
    }
    index
        .build(hora::core::metrics::Metric::Euclidean)
        .map_err(E::msg)?;

    let target = embed.embed(&prompt)?;

    let k = 2;

    // search for k nearest neighbors
    let nn: Vec<&str> = index
        .search(&target, k)
        .into_iter()
        .map(|i| strings[i])
        .collect();
    println!("target has neighbors: {:?}", nn);

    HttpServer::new(|| App::new().service(hello).service(add_snippet))
        .bind(("127.0.0.1", 8000))?
        .run()
        .await
        .map_err(E::from)
}
