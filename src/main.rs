use std::{
    collections::{BTreeMap, HashMap},
    sync::Mutex,
};

use actix_web::{App, HttpServer, Responder, post, web};
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

struct AppStateWrapper {
    inner: Mutex<AppState>,
}

struct AppState {
    dict: HashMap<String, HNSWIndex<f32, String>>,
    embed: embed::Embed,
}

#[post("/api/v1/get")]
async fn v1_get_snippet(
    data: web::Data<AppStateWrapper>,
    snippet_request: web::Json<SnippetRequest>,
) -> impl Responder {
    let Some((prompt, lang)) = snippet_request.desc.rsplit_once(" in ") else {
        return format!("end your request with \" in somelang\".");
    };

    let Ok(mut appstate) = data.inner.lock() else {
        return format!("the server is busy.");
    };

    let Ok(target) = appstate.embed.embed(prompt) else {
        return format!("failed to embed your proompt. come back later.");
    };

    // search for k nearest neighbors
    let k = 1;
    let nn: Vec<String> = appstate.dict[lang].search(&target, k);
    for n in nn {
        // basically returns the first one and dies
        return format!("{n}");
    }
    format!("bruh, you asked for {}", snippet_request.desc)
}

#[post("/api/v1/add")]
async fn v1_add_snippet(data: web::Data<AppState>, snippet: web::Json<Snippet>) -> impl Responder {
    format!("{} {} {}", snippet.body, snippet.lang, snippet.desc)
}

#[actix_web::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let mut embed = embed::Embed::new(args)?;

    let mut lang_map = BTreeMap::default();

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

        let current_lang_index = lang_map.entry(parent).or_insert_with(|| {
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

    let mut appstate = HashMap::default();

    for (k, mut index) in lang_map.into_iter() {
        index
            .build(hora::core::metrics::Metric::Euclidean)
            .map_err(E::msg)?;
        appstate.insert(k, index);
    }
    let appstate = AppState {
        dict: appstate,
        embed,
    };

    let appstate_wrapped = web::Data::new(AppStateWrapper {
        inner: Mutex::new(appstate),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(appstate_wrapped.clone())
            .service(v1_get_snippet)
            .service(v1_add_snippet)
    })
    .bind(("127.0.0.1", 8000))?
    .run()
    .await
    .map_err(E::from)
}
